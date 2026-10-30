#!/usr/bin/env python3
"""Opt-in real Marmot command validation with disposable private identities."""
import argparse
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace

from mdk_smoke import SmokeError, require
from marmish_app import Client, Profile, exit_code
from marmish_protocol import b64, encode, envelope, load_json, save_json, remove_engine_state, EngineBridge
from marmish_rpc import RpcError, MdkRpc, strict_json
from marmish_keys import KeySession, NativeHelper, protected_read
from marmish_native_validation import (active_context, native_environment, scan_at_rest,
                                       scan_process, audit_tree, audit_helper)
from marmish_transport_sandbox import SandboxError
from marmish_platform import continuous_time, execution_marker
from marmish_ci_bwrap import environment as bwrap_environment, provenance as bwrap_provenance


class Harness:
    def __init__(self, args, root, report):
        self.args, self.root, self.report = args, root, report
        self.deadline = continuous_time() + args.timeout
        self.server = None
        self.inhibitor = None
        self.clock_start = continuous_time()
        self.monotonic_start = time.monotonic()
        self.cwd = root / 'commands'
        self.cwd.mkdir(mode=0o700)
        self.server_identity = None
        self.client_identity = None
        self.storage = 'files' if getattr(args, 'migrate_acceptance', False) else args.key_storage
        self.native = active_context()
        self.provider_locked = False
        require(self.storage == 'files' or self.native is not None, 'owned_native_context_required')
        require(not getattr(args, 'migrate_acceptance', False) or self.native is not None,
                'owned_native_context_required')

    def profile_args(self, name):
        return SimpleNamespace(identity=self.root / name, relay=self.args.relay, key_storage=self.storage,
                               key_name=(self.args.key_name_prefix + '-' + name
                                         if self.args.key_name_prefix and self.storage != 'files' else None),
                               keychain_helper=self.args.keychain_helper, key_catalog=self.args.key_catalog,
                               wnd=self.args.wnd, wnd_sha256=self.args.wnd_sha256,
                               wnd_manifest=getattr(self.args, 'wnd_manifest', None))

    def common_args(self, name):
        args = ['--key-storage', self.storage, '-i', str(self.root / name), '--wnd', str(self.args.wnd),
                '--wnd-sha256', self.args.wnd_sha256]
        if self.args.key_name_prefix and self.storage != 'files':
            args += ['--key-name', self.args.key_name_prefix + '-' + name]
        if self.args.keychain_helper:
            args += ['--keychain-helper', str(self.args.keychain_helper)]
        if self.args.key_catalog:
            args += ['--key-catalog', str(self.args.key_catalog)]
        for relay in self.args.relay:
            args += ['--relay', relay]
        if getattr(self.args, 'wnd_manifest', None):
            args += ['--wnd-manifest', str(self.args.wnd_manifest)]
        return args

    def check_locked_restarts(self):
        self.checkpoint('locked_frontend_refusal')
        for name in ('server', 'client'):
            try:
                with Profile(self.profile_args(name)):
                    raise SmokeError('locked_frontend_started')
            except SmokeError as error:
                require(str(error) == 'locked_or_denied', 'locked_frontend_wrong_error')
            require(load_json(self.root / name / 'profile.json')['keys']['backend'] == 'native',
                    'locked_frontend_fell_back')
        self.passed('locked_native_provider_refuses_both_frontends_without_file_fallback')

    def migration_args(self, name):
        args = self.profile_args(name)
        args.migrate_keys_to_native = True
        args.server_mode = name == 'server'
        args.allow = [self.client_identity]
        args.worker, args.bridge, args.cwd = self.args.worker, self.args.bridge, self.cwd
        args.allow_shell = True
        args.loaded_command_policy = load_json(self.root / 'command-policy.json')
        return args

    def refuse_active_migration(self):
        self.checkpoint('active_worker_migration_refusal')
        root = self.root / 'server'
        saved = {path: path.read_bytes() for path in (root / 'profile.json',
            root / 'engine/engine.key',
            root / 'mdk/accounts' / self.server_identity / 'secret.json')}
        references = self.native.references()
        storage = self.storage
        self.storage = 'native'
        try:
            try:
                with Profile(self.migration_args('server')):
                    raise SmokeError('active_worker_migration_accepted')
            except SmokeError as error:
                require(str(error) in ('busy', 'recovery_uncertain'),
                        'active_worker_migration_wrong_error')
                self.report['active_migration_refusal'] = str(error)
        finally:
            self.storage = storage
        require(all(path.read_bytes() == data for path, data in saved.items()),
                'active_worker_migration_changed_legacy_keys')
        require(self.native.references() == references, 'active_worker_migration_created_keys')
        pending = load_json(root / 'keys.pending.json')
        self.migration_reserved = {kind: pending[kind] for kind in ('nostr', 'master')}
        self.passed('active_worker_refuses_migration_preserving_legacy_keys_and_native_inventory')

    def encrypted_snapshot(self):
        # The migration guard proved quiescence. Compare the SQLCipher database
        # and retained job files; WAL/SHM and MDK housekeeping are not this check.
        result = {}
        directory = self.root / 'server/engine'
        for path in directory.rglob('*'):
            if path.is_file() and (path.name == 'engine.sqlite3'
                                   or 'jobs' in path.relative_to(directory).parts):
                result[str(path.relative_to(self.root))] = hashlib.sha256(path.read_bytes()).hexdigest()
        require(result, 'migration_snapshot_empty')
        return result

    def quiesce_migration(self):
        """Authenticate and prove job/IO quiescence before ciphertext fingerprinting."""
        self.checkpoint('migration_quiescence')
        require(self.server is None, 'migration_frontend_still_running')
        target = self.migration_reserved['master']
        raw = protected_read(self.root / 'server/engine/engine.key', 40)
        require(len(raw) == 40 and raw[:8] == b'MRMSKEY1', 'invalid_legacy_secret')
        policy = {'version': 2, 'state_root': str(self.root / 'server/engine'),
            'worker_executable': str(self.args.worker.absolute()), 'server': self.server_identity,
            'allowed_principals': [self.client_identity], 'cwd': str(self.cwd),
            'allow_shell_command': True,
            'command_policy': load_json(self.root / 'command-policy.json'),
            'storage': {'mode': 'migrate_native', 'root_id': target['root_id'],
                        'reference': target['reference'], 'master': b64(raw[8:])}}
        started = continuous_time()
        until = min(self.deadline, started + 20)
        prior_timer = signal.getitimer(signal.ITIMER_REAL)
        prior_handler = signal.getsignal(signal.SIGALRM)
        def expired(_sig, _frame):
            raise SmokeError('migration_quiescence_timeout')
        signal.signal(signal.SIGALRM, expired)
        signal.setitimer(signal.ITIMER_REAL,
                         min(until - started, prior_timer[0]) if prior_timer[0] else until - started)
        try:
            while True:
                self.checkpoint('migration_quiescence')
                require(continuous_time() < until, 'migration_quiescence_timeout')
                guard = None
                try:
                    guard = EngineBridge(self.args.bridge, policy)
                    return
                except SmokeError as error:
                    if str(error) not in ('busy', 'recovery_uncertain'):
                        raise
                    require(continuous_time() < until, 'migration_quiescence_timeout')
                    time.sleep(min(.1, max(0, until - continuous_time())))
                finally:
                    if guard is not None:
                        guard.close()
        finally:
            signal.setitimer(signal.ITIMER_REAL, 0)
            signal.signal(signal.SIGALRM, prior_handler)
            if prior_timer[0]:
                signal.setitimer(signal.ITIMER_REAL,
                                 max(.001, prior_timer[0] - (continuous_time() - started)), prior_timer[1])

    def migrate_profiles(self):
        self.quiesce_migration()
        self.checkpoint('native_migration')
        before = self.encrypted_snapshot()
        public = {name: load_json(self.root / name / 'profile.json')
                  for name in ('server', 'client', 'intruder')}
        self.storage = 'native'
        for name in ('server', 'client', 'intruder'):
            with Profile(self.migration_args(name)) as profile:
                require(profile.migrated and profile.account == public[name]['account'],
                        'migration_identity_changed')
                require(profile.value['sessions'] == public[name]['sessions']
                        and profile.value['server_groups'] == public[name]['server_groups'],
                        'migration_room_state_changed')
        require(self.encrypted_snapshot() == before, 'migration_ciphertext_changed')
        committed = load_json(self.root / 'server/profile.json')['keys']
        require(all(committed[kind] == binding for kind, binding in self.migration_reserved.items()),
                'migration_retry_changed_reserved_references')
        self.check_native_names()
        self.check_native_storage()
        self.passed('file_to_native_migration_preserves_identity_rooms_and_encrypted_state')

    def check_native_storage(self):
        if self.storage == 'files':
            return
        references = []
        for name in ('server', 'client', 'intruder', 'unnamed'):
            root = self.root / name
            value = load_json(root / 'profile.json')
            require(value['keys']['backend'] == 'native', 'native_profile_missing')
            references.append(value['keys']['nostr']['reference'])
            require(not list((root / 'mdk').rglob('secret.json'))
                    and not (root / 'engine/engine.key').exists(), 'native_secret_file_present')
            master = value['keys']['master']
            if master is not None:
                references.append(master['reference'])
        require(len({item['id'] for item in references}) == len(references),
                'native_reference_reused')
        catalog = load_json(self.args.key_catalog or self.native.catalog)
        require(sorted((entry['reference'] for entry in catalog['entries']), key=lambda item: item['id']) ==
                sorted((item for item in references if item['purpose'] == 'nostr'),
                       key=lambda item: item['id']), 'native_catalog_inventory_mismatch')
        observed = self.native.references()
        require(sorted(observed, key=lambda item: item['id']) ==
                sorted(references, key=lambda item: item['id']), 'native_reference_inventory_mismatch')
        self.report['native_references'] = references
        secrets_to_scan = [NativeHelper(self.args.keychain_helper or self.native.helper).load(reference)
                           for reference in references]
        self.native.remember_secrets(secrets_to_scan)
        scan_at_rest([self.root, self.args.key_catalog or self.native.catalog], secrets_to_scan)
        self.report['native_helper_audit'] = audit_helper(
            self.args.keychain_helper or self.native.helper, self.native.env, secrets_to_scan)
        if sys.platform.startswith('linux') and self.server is not None:
            scan_process(self.server.pid, secrets_to_scan)
        self.passed('native_reference_inventory_catalog_and_secret_at_rest_process_checks')

    def audit_running_native_worker(self, profile=None):
        references = self.native.references()
        helper = NativeHelper(self.args.keychain_helper or self.native.helper)
        private = [helper.load(reference) for reference in references]
        self.native.remember_secrets(private)
        self.report['native_process_audit'] = audit_tree(
            self.server.pid, private, require_worker=True)
        if profile is not None:
            self.report['native_client_process_audit'] = audit_tree(
                profile.transport.process.pid, private)
        self.passed('running_native_daemon_worker_have_no_secret_arguments_environment_or_bootstrap_stdin')

    def server_rpc(self):
        home = self.root / 'server/mdk'
        options = {'secret_store': 'MarmishStdin' if self.storage != 'files' else 'File'}
        if sys.platform == 'darwin':
            from marmish_transport_macos import read_running_identity
            options.update(rpc_home=str(home), rpc_socket=str(home / 's'),
                           peer_identity=read_running_identity(home))
        return MdkRpc(home / 's', timeout=20, **options)

    def check_native_identities(self, profile):
        if self.storage == 'files':
            return
        for rpc, expected in ((self.server_rpc(), self.server_identity),
                              (profile.rpc, self.client_identity)):
            require(rpc.capabilities() == {'version': 1, 'secret_store': 'MarmishStdin',
                                           'public_key': expected}
                    and rpc.accounts() == [expected], 'native_endpoint_identity_capability_mismatch')

    def audit_after_migration(self, client, profile):
        """Inspect a live native worker and both daemons after the file migration."""
        self.checkpoint('post_migration_native_audit')
        gate = self.cwd / 'native-audit-release'
        invocation, _ = client.submit(self.command(
            'import pathlib,time; p=pathlib.Path("native-audit-release"); t=time.monotonic()+45\n'
            'while not p.exists() and time.monotonic()<t: time.sleep(.05)\nprint("native audit")'))
        try:
            client.send_input(invocation, io.BytesIO())
            until = continuous_time() + 20
            while client.call('status', invocation=invocation)['state'] != 'running':
                require(continuous_time() < until, 'native_audit_worker_not_running')
                time.sleep(.1)
            self.audit_running_native_worker(profile)
        finally:
            gate.touch()
        out = io.BytesIO()
        status = client.wait(invocation, out, io.BytesIO())
        require(out.getvalue() == b'native audit\n' and exit_code(status) == 0,
                'native_audit_command_failed')
        self.passed('post_migration_native_command_and_process_audit')

    def check_native_names(self):
        if self.storage == 'files':
            return
        options = {'storage': 'auto', 'helper': self.args.keychain_helper,
                   'catalog': self.args.key_catalog}
        with KeySession(self.root / 'unnamed', **options) as keys:
            binding = keys.prepare_nostr(pristine=True)
            keys.commit({'version': 2, 'account': binding.account, 'initialized': False,
                         'relays': sorted(self.args.relay), 'sessions': {}, 'server_groups': {}})
        if self.args.key_name_prefix:
            for root, name, expected in (
                    (self.root / 'client', self.args.key_name_prefix + '-different', 'key_name_mismatch'),
                    (self.root / 'adoption', self.args.key_name_prefix + '-client', 'key_name_bound_elsewhere')):
                try:
                    with KeySession(root, key_name=name, **options) as keys:
                        keys.prepare_nostr(existing_account=(self.client_identity
                            if root.name == 'client' else None), pristine=root.name != 'client')
                    raise SmokeError('native_name_refusal_missing')
                except SmokeError as error:
                    require(str(error) == expected, 'native_name_refusal_wrong_error')
        self.passed('unnamed_auto_native_profile_and_named_binding_refusals')

    def remove_engine(self, policy):
        root = self.root / 'server'
        value = load_json(root / 'profile.json')
        if value.get('keys', {}).get('backend') != 'native':
            return remove_engine_state(self.args.bridge, policy)
        with KeySession(root, storage='native', helper=self.args.keychain_helper,
                        catalog=self.args.key_catalog) as keys:
            keys.prepare_nostr(existing_account=value['account'])
            master = keys.prepare_master(root / 'engine')
            native_policy = dict(policy, version=2, storage={'mode': 'native',
                'root_id': master.root_id, 'reference': master.reference, 'master': b64(master.secret)})
            remove_engine_state(self.args.bridge, native_policy)

    def cleanup(self):
        """Quiesce owned work before deleting either data or provider credentials."""
        try:
            for name in ('cancel-release', 'disconnect-release', 'native-audit-release'):
                (self.cwd / name).touch()
            self.stop_server()
            if self.inhibitor:
                self.inhibitor.terminate()
                self.inhibitor.wait(timeout=5)
            policy = {'version': 1, 'state_root': str(self.root / 'server/engine'),
                'worker_executable': str(self.args.worker.absolute()), 'server': self.server_identity,
                'allowed_principals': [self.client_identity], 'cwd': str(self.cwd),
                'allow_shell_command': True,
                'command_policy': load_json(self.root / 'command-policy.json', None)}
            until = continuous_time() + 65
            while True:
                try:
                    if self.provider_locked:
                        self.native.unlock()
                        self.provider_locked = False
                    if (self.root / 'server/engine').exists():
                        self.remove_engine(policy)
                    if sys.platform == 'darwin':
                        from marmish_transport_macos import cleanup_control, control_dir
                        for name in ('server', 'client', 'intruder'):
                            home = self.root / name / 'mdk'
                            if os.path.lexists(control_dir(home)):
                                cleanup_control(home)
                    shutil.rmtree(self.root)
                    self.report['local_cleanup'] = 'complete'
                    return
                except (SmokeError, SandboxError, OSError, ValueError):
                    if continuous_time() >= until:
                        raise SmokeError('cleanup_incomplete') from None
                    time.sleep(.5)
        except (SmokeError, SandboxError, OSError, ValueError, subprocess.TimeoutExpired):
            self.report.update(outcome='failed', local_cleanup='incomplete',
                               private_state_retained=True)
            if self.native is not None:
                self.native.retain_private_state()

    def checkpoint(self, stage):
        require(continuous_time() < self.deadline, 'global_timeout')
        require(abs((continuous_time() - self.clock_start) -
                    (time.monotonic() - self.monotonic_start)) <= 5, 'host_suspended')
        require(not self.inhibitor or self.inhibitor.poll() is None, 'sleep_assertion_failed')
        self.report['stage'] = stage
        print('e2e: ' + stage, file=sys.stderr, flush=True)

    def passed(self, assertion):
        self.report['assertions'].append(assertion)

    def start_server(self):
        self.checkpoint('server_start')
        script = Path(__file__).with_name('marmishd.py')
        policy = self.root / 'command-policy.json'
        save_json(policy, {'version': 1, 'defaults': {'allow_exec': ['/**'],
                  'deny_exec': [str(Path('/usr/bin/false').resolve())]}, 'keys': [
                  {'principal': self.client_identity,
                   'restrictions': {'deny_exec': [str(Path('/usr/bin/true').resolve())]}}]})
        self.server = subprocess.Popen([sys.executable, str(script), *self.common_args('server'),
            '--allow', self.client_identity, '--bridge', str(self.args.bridge),
            '--worker', str(self.args.worker), '--cwd', str(self.cwd),
            '--command-policy', str(policy), '--allow-shell'],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            start_new_session=True, env={'PATH': '/usr/bin:/usr/sbin:/bin:/sbin', 'LANG': 'C', **execution_marker(), **bwrap_environment(), **native_environment(),
                **({key: os.environ[key] for key in ('RUNNER_TRACKING_ID',) if key in os.environ})})
        require(select.select([self.server.stdout], [], [], 25)[0], 'server_start_timeout')
        ready = self.server.stdout.readline(1024)
        try:
            value = json.loads(ready)
        except ValueError:
            category = 'server_start_failed'
            if self.server.poll() is not None:
                error = self.server.stderr.read(256).decode('ascii', errors='ignore').strip()
                if error.startswith('marmish: ') and error[9:].replace('_', '').isalnum():
                    category = error[9:]
            raise SmokeError(category) from None
        require(value == {'ready': True, 'account': self.server_identity}, 'server_start_failed')

    def stop_server(self):
        if self.server:
            self.server.terminate()
            try:
                self.server.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.server.kill()
                self.server.wait(timeout=3)
            self.server.stdout.close()
            self.server.stderr.close()
            self.server = None

    def client(self, profile, **options):
        return Client(profile, self.server_identity,
                      timeout=min(900, max(1, self.deadline - continuous_time())), **options)

    @staticmethod
    def command(script, *args):
        return {'mode': 'exec', 'argv': [b64(x) for x in
                [os.fsencode(Path(sys.executable).resolve()), b'-c', script.encode('ascii'), *args]]}

    def check_command_policy(self, client):
        self.checkpoint('command_policy')
        policy_token = client.call('token')['token']
        policy_id = secrets.token_hex(16)
        for command in (
                {'mode': 'exec', 'argv': [b64(os.fsencode(Path('/usr/bin/false').resolve()))]},
                {'mode': 'exec', 'argv': [b64(os.fsencode(Path('/usr/bin/true').resolve()))]},
                {'mode': 'exec', 'argv': [b64(b'true')]},
                {'mode': 'shell', 'command': b64(b'touch forbidden-policy')}):
            try:
                client.call('submit', invocation=policy_id, token=policy_token, command=command)
                raise SmokeError('policy_denial_missing')
            except SmokeError as error:
                require(str(error) == 'command_denied', 'policy_wrong_error')
            try:
                client.call('status', invocation=policy_id)
                raise SmokeError('denied_job_created')
            except SmokeError as error:
                require(str(error) == 'not_found', 'denied_job_wrong_error')
        require(not (self.cwd / 'forbidden-policy').exists(), 'policy_shell_side_effect')
        admitted = client.call('submit', invocation=policy_id, token=policy_token,
                               command=self.command('print("policy allowed")'))
        require(admitted['disposition'] == 'accepted', 'denial_consumed_token')
        client.send_input(policy_id, io.BytesIO())
        policy_out = io.BytesIO()
        policy_status = client.wait(policy_id, policy_out, io.BytesIO())
        require(policy_out.getvalue() == b'policy allowed\n' and exit_code(policy_status) == 0,
                'policy_allowed_execution_failed')
        self.passed('live_additive_default_and_per_key_denials_path_and_shell_refusal')
        self.passed('policy_denials_create_no_job_and_preserve_token_for_allowed_execution')

    def run(self):
        if sys.platform == 'darwin':
            self.inhibitor = subprocess.Popen(['/usr/bin/caffeinate', '-i', '-s'],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                env={'PATH': '/usr/bin:/bin', **execution_marker()})
        self.checkpoint('identity_setup')
        with Profile(self.profile_args('server')) as profile:
            self.server_identity = profile.account
        with Profile(self.profile_args('client')) as profile:
            self.client_identity = profile.account
        self.check_native_names()
        self.report['public_identities'] = {'server': self.server_identity, 'client': self.client_identity}
        self.start_server()
        with Profile(self.profile_args('client')) as profile:
            self.checkpoint('literal_arguments')
            client = self.client(profile)
            initial_group = client.group
            self.report['groups'] = [initial_group]
            self.check_command_policy(client)
            special = [b'with spaces', 'marmotte \U0001f9ab'.encode(), b"'double\"quote", b'line\nline',
                       b'-leading', b'$HOME', b'`id`', b'$(id)', b'\\backslash', b'', b'\xff']
            script = ('import sys,os,json,base64; '
                      'open("executions","ab").write(b"once\\n"); '
                      'print(json.dumps([base64.b64encode(os.fsencode(x)).decode() for x in sys.argv[1:]])); '
                      'sys.stderr.buffer.write(b"stderr\\x00\\xff"); sys.exit(7)')
            command = self.command(script, *special)
            invocation, _ = client.submit(command)
            client.send_input(invocation, io.BytesIO())
            out, err = io.BytesIO(), io.BytesIO()
            status = client.wait(invocation, out, err, scrub=False)
            require(json.loads(out.getvalue()) == [b64(x) for x in special]
                    and err.getvalue() == b'stderr\x00\xff' and exit_code(status) == 7,
                    'literal_argv_or_exit_mismatch')
            self.passed('literal_argv_spaces_unicode_quotes_newline_dollar_backticks_leading_dash_nonutf8')
            self.passed('separate_binary_stderr_and_nonzero_exit')
            journal = load_json(client.journal_path(invocation))
            self.checkpoint('durable_retry')
            duplicate = client.call('submit', invocation=invocation, token=journal['token'], command=command)
            require(duplicate['disposition'] == 'existing', 'duplicate_not_existing')
            client.retry(invocation)
            require((self.cwd / 'executions').read_bytes() == b'once\n', 'duplicate_execution')
            bad_command = self.command('print("conflict")')
            try:
                client.call('submit', invocation=invocation, token=journal['token'], command=bad_command)
                raise SmokeError('conflict_accepted')
            except SmokeError as error:
                require(str(error) == 'invocation_conflict', 'conflict_wrong_error')
            self.passed('exact_admitted_retry_and_status_first_retry_execute_once_conflict_rejected')
            client.scrub(invocation)
            self.checkpoint('binary_streams')
            data = bytes(range(256)) * 2
            binary, _ = client.submit(self.command(
                'import sys; d=sys.stdin.buffer.read(); sys.stdout.buffer.write(d); sys.stderr.buffer.write(d[::-1])'))
            client.send_input(binary, io.BytesIO(data))
            out, err = io.BytesIO(), io.BytesIO()
            status = client.wait(binary, out, err)
            require(out.getvalue() == data and err.getvalue() == data[::-1] and exit_code(status) == 0,
                    'binary_stream_mismatch')
            self.passed('byte_exact_binary_stdin_stdout_stderr_eof')
            self.checkpoint('cancellation')
            cancelled, _ = client.submit(self.command('import pathlib,time; p=pathlib.Path("cancel-release"); t=time.monotonic()+120\nwhile not p.exists() and time.monotonic()<t: time.sleep(.05)'))
            client.send_input(cancelled, io.BytesIO())
            client.call('cancel', invocation=cancelled)
            cancelled_status = client.wait(cancelled, io.BytesIO(), io.BytesIO())
            require(cancelled_status['cancel_requested'] is True
                    and cancelled_status['outcome']['kind'] == 'signal', 'cancellation_failed')
            self.passed('live_cancellation_reports_signal_and_remains_queryable')
            self.checkpoint('negative_frames')
            rejected = secrets.token_hex(16)
            token = client.call('token')['token']
            malicious_body = {'invocation': rejected, 'token': token,
                              'command': self.command('open("forbidden","w").write("bad")')}
            for challenge, group in [('00' * 32, client.group), (client.challenge, 'aa' * 32)]:
                bad = envelope(client.sid, group, profile.account, self.server_identity,
                               secrets.token_hex(16), 'submit', malicious_body, challenge)
                profile.rpc.send(profile.account, client.group, encode(bad).decode())
            profile.rpc.send(profile.account, client.group, encode({'text': 'ordinary chat',
                'op': 'submit', 'body': malicious_body}).decode())
            try:
                client.call('status', invocation=rejected)
                raise SmokeError('forbidden_invocation_found')
            except SmokeError as error:
                require(str(error) == 'not_found', 'negative_wrong_error')
            require(not (self.cwd / 'forbidden').exists(), 'forbidden_side_effect')
            self.passed('live_stale_challenge_wrong_group_and_chat_payload_never_execute')
            self.checkpoint('unauthorized_and_ordinary_invitations')
            ordinary_sid = secrets.token_hex(16)
            ordinary = profile.rpc.execute({'Groups': {'command': {'Create': {
                'name': 'ordinary conversation', 'members': [self.server_identity],
                'description': None, 'retention': '1d', 'image': None, 'image_media_type': None,
            }}}}, account=profile.account)['group_id']
            bad = envelope(ordinary_sid, ordinary, profile.account, self.server_identity,
                           secrets.token_hex(16), 'submit', malicious_body, client.challenge)
            profile.rpc.send(profile.account, ordinary, encode(bad).decode())
            with Profile(self.profile_args('intruder')) as intruder:
                intruder.rpc.fetch_keys(intruder.account, self.server_identity, intruder.relays)
                unallowed = intruder.rpc.create_group(intruder.account, self.server_identity,
                                                     secrets.token_hex(16))['group_id']
                server_rpc = self.server_rpc()
                if self.storage != 'files':
                    self.check_native_identities(profile)
                    self.passed('native_daemon_capabilities_and_exact_server_client_identities')
                observed = set()
                while observed != {ordinary, unallowed}:
                    client.remaining()
                    client.call('status', invocation=invocation)  # server-loop barrier
                    invites = server_rpc.invites(self.server_identity)
                    observed = {x['group_id'] for x in invites if x['group_id'] in {ordinary, unallowed}
                                and x['pending_confirmation'] is True}
                    if observed != {ordinary, unallowed}:
                        time.sleep(.5)
            require(not (self.cwd / 'forbidden').exists(), 'ordinary_room_side_effect')
            self.passed('live_ordinary_room_and_unallowlisted_identity_invites_remain_unaccepted')
            replay_id = secrets.token_hex(16)
            replay_token = client.call('token')['token']
            replay = envelope(client.sid, client.group, profile.account, self.server_identity,
                secrets.token_hex(16), 'submit', {'invocation': replay_id, 'token': replay_token,
                'command': self.command('open("forbidden","w").write("replay")')}, client.challenge)
            self.checkpoint('disconnect_submit')
            detached, _ = client.submit(self.command(
                'import pathlib,time; p=pathlib.Path("disconnect-release"); t=time.monotonic()+120\nwhile not p.exists() and time.monotonic()<t: time.sleep(.05)\nopen("survived","a").write("yes"); print("survived")'))
            client.send_input(detached, io.BytesIO())
            running = client.call('status', invocation=detached)
            require(running['state'] in ('accepted', 'launching', 'running'), 'job_finished_before_disconnect')
            until_running = continuous_time() + 20
            while running['state'] != 'running':
                require(continuous_time() < until_running, 'detached_worker_not_running')
                time.sleep(.1)
                running = client.call('status', invocation=detached)
                require(running['state'] in ('accepted', 'launching', 'running'),
                        'job_finished_before_disconnect')
            if self.storage != 'files':
                self.audit_running_native_worker(profile)
        if self.storage != 'files':
            self.native.lock()
            self.provider_locked = True
        # Both client MDK and server/bridge frontends exit while the worker runs.
        self.stop_server()
        if self.args.migrate_acceptance:
            self.refuse_active_migration()
        require(not (self.cwd / 'survived').exists(), 'job_finished_before_frontend_stop')
        (self.cwd / 'disconnect-release').touch()
        until = continuous_time() + 8
        while not (self.cwd / 'survived').exists():
            require(continuous_time() < until, 'detached_worker_did_not_finish')
            time.sleep(.1)
        self.passed('frontend_disconnect_does_not_cancel_running_worker')
        if self.storage != 'files':
            self.check_locked_restarts()
            self.native.unlock()
            self.provider_locked = False
        if self.args.migrate_acceptance:
            self.migrate_profiles()
        self.start_server()
        with Profile(self.profile_args('client')) as profile:
            self.checkpoint('reattach_after_restart')
            client = self.client(profile)
            require(client.group == initial_group, 'session_not_reused')
            if self.storage != 'files':
                self.check_native_identities(profile)
                self.passed('native_restart_capability_identity_preserved')
            if self.args.migrate_acceptance:
                self.audit_after_migration(client, profile)
            out, err = io.BytesIO(), io.BytesIO()
            status = client.wait(detached, out, err)
            require(out.getvalue() == b'survived\n' and exit_code(status) == 0
                    and (self.cwd / 'survived').read_text() == 'yes', 'disconnect_recovery_failed')
            self.passed('persisted_mls_room_and_command_output_recovered_after_frontend_restart')
            profile.rpc.send(profile.account, client.group, encode(replay).decode())
            try:
                client.call('status', invocation=replay_id)
                raise SmokeError('historical_replay_executed')
            except SmokeError as error:
                require(str(error) == 'not_found', 'historical_replay_wrong_error')
            require(not (self.cwd / 'forbidden').exists(), 'historical_replay_side_effect')
            self.passed('previous_boot_valid_control_frame_rejected_after_restart')
            self.checkpoint('fresh_session')
            fresh = self.client(profile, new_session=True, retention=3600)
            require(fresh.group != initial_group and fresh.sid != client.sid, 'session_not_fresh')
            self.report['groups'].append(fresh.group)
            retention = profile.rpc.retention(profile.account, fresh.group)
            require(retention['disappearing_message_secs'] == 3600, 'retention_not_configurable')
            require(fresh.call('status', invocation=invocation)['state'] == 'exited', 'cross_session_status_failed')
            self.passed('fresh_independent_mls_group_configurable_retention_same_principal_reattach')
        self.checkpoint('cli_process')
        target = self.server_identity + '@' + self.args.relay[0].removeprefix('wss://').rstrip('/')
        result = subprocess.run([sys.executable, str(Path(__file__).with_name('marmish.py')),
            *self.common_args('client'), '--timeout', '120', target, '--',
            '/usr/bin/printf', '%s\n', 'marmish cli e2e'],
            input=b'', stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=min(150, max(1, self.deadline - continuous_time())),
            env={'PATH': '/usr/bin:/usr/sbin:/bin:/sbin', 'LANG': 'C', **execution_marker(), **bwrap_environment(), **native_environment(),
                **({key: os.environ[key] for key in ('RUNNER_TRACKING_ID',) if key in os.environ})})
        require(result.returncode == 0 and result.stdout == b'marmish cli e2e\n' and result.stderr == b'',
                'cli_process_failed')
        self.passed('actual_client_and_server_cli_processes_execute_command_via_marmot')
        self.check_native_storage()
        self.report.update(outcome='passed', stage='complete')


def main(argv=None, *, report_sink=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--live', action='store_true', required=True)
    parser.add_argument('--relay', action='append', required=True)
    parser.add_argument('--wnd', type=Path, required=True)
    parser.add_argument('--wnd-sha256', required=True)
    parser.add_argument('--wnd-manifest', type=Path)
    parser.add_argument('--bridge', type=Path, required=True)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=900)
    parser.add_argument('--key-storage', choices=('files', 'auto', 'native'), default='files')
    parser.add_argument('--keychain-helper', type=Path)
    parser.add_argument('--key-catalog', type=Path)
    parser.add_argument('--key-name-prefix')
    parser.add_argument('--migrate-acceptance', action='store_true')
    args = parser.parse_args(argv)
    require(60 <= args.timeout <= 900, 'invalid_timeout')
    require((args.key_storage == 'files' and not args.migrate_acceptance) or
            active_context() is not None, 'owned_native_context_required')
    os.umask(0o077)
    root = Path(tempfile.mkdtemp(prefix='me-', dir='/tmp')).resolve()
    report = {'outcome': 'failed', 'stage': 'setup', 'assertions': [], 'relays': args.relay,
              'wnd_sha256': args.wnd_sha256, 'limitations': [
                  'prototype_single_device',
                  'relay_retention_is_not_guaranteed_erasure', 'same_principal_can_reattach_across_sessions']}
    harness = Harness(args, root, report)
    started = continuous_time()
    handlers = {}
    def interrupted(_sig, _frame):
        raise SmokeError('interrupted')
    try:
        for sig in (signal.SIGINT, signal.SIGTERM):
            handlers[sig] = signal.signal(sig, interrupted)
        handlers[signal.SIGALRM] = signal.signal(signal.SIGALRM, interrupted)
        signal.alarm(args.timeout)
        harness.run()
    except (SmokeError, RpcError, SandboxError) as error:
        category = str(error)
        report['error'] = category if category.replace('_', '').isalnum() and len(category) < 80 else 'transport_failure'
    except (OSError, ValueError, TypeError, KeyError, AttributeError, subprocess.TimeoutExpired):
        report['error'] = 'local_io_or_schema_failure'
    finally:
        signal.alarm(0)
        for sig in handlers:
            signal.signal(sig, signal.SIG_IGN)
        harness.cleanup()
        report['duration_seconds'] = round(continuous_time() - started, 3)
        for sig, handler in handlers.items():
            signal.signal(sig, handler)
    report.update(platform=sys.platform, python=sys.version.split()[0])
    try:
        report['bubblewrap'] = bwrap_provenance()
    except (SandboxError, OSError, ValueError, KeyError):
        report.update(outcome='failed', error='bwrap_provenance_unavailable')
    if args.wnd_manifest:
        report['wnd_manifest'] = strict_json(args.wnd_manifest.read_bytes())
    if report_sink is None:
        print(json.dumps(report, sort_keys=True))
    else:
        report_sink.append(report)
    return 0 if report['outcome'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
