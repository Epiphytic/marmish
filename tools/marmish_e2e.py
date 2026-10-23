#!/usr/bin/env python3
"""Opt-in real Marmot command validation with disposable private identities."""
import argparse
import contextlib
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
from marmish_protocol import b64, encode, envelope, load_json, save_json, remove_engine_state
from marmish_rpc import RpcError, MdkRpc, strict_json
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

    def profile_args(self, name):
        return SimpleNamespace(identity=self.root / name, relay=self.args.relay,
                               wnd=self.args.wnd, wnd_sha256=self.args.wnd_sha256,
                               wnd_manifest=getattr(self.args, 'wnd_manifest', None))

    def common_args(self, name):
        args = ['-i', str(self.root / name), '--wnd', str(self.args.wnd),
                '--wnd-sha256', self.args.wnd_sha256]
        for relay in self.args.relay:
            args += ['--relay', relay]
        if getattr(self.args, 'wnd_manifest', None):
            args += ['--wnd-manifest', str(self.args.wnd_manifest)]
        return args

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
            start_new_session=True, env={'PATH': '/usr/bin:/usr/sbin:/bin:/sbin', 'LANG': 'C', **execution_marker(), **bwrap_environment(),
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
                home = self.root / 'server' / 'mdk'
                options = {}
                if sys.platform == 'darwin':
                    from marmish_transport_macos import read_running_identity
                    options = {'rpc_home': str(home), 'rpc_socket': str(home / 's'),
                               'peer_identity': read_running_identity(home)}
                server_rpc = MdkRpc(home / 's', timeout=20, **options)
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
                'import pathlib,time; p=pathlib.Path("disconnect-release"); t=time.monotonic()+120\nwhile not p.exists() and time.monotonic()<t: time.sleep(.05)\nopen("survived","w").write("yes"); print("survived")'))
            client.send_input(detached, io.BytesIO())
            running = client.call('status', invocation=detached)
            require(running['state'] in ('accepted', 'launching', 'running'), 'job_finished_before_disconnect')
        # Both client MDK and server/bridge frontends exit while the worker runs.
        self.stop_server()
        require(not (self.cwd / 'survived').exists(), 'job_finished_before_frontend_stop')
        (self.cwd / 'disconnect-release').touch()
        until = continuous_time() + 8
        while not (self.cwd / 'survived').exists():
            require(continuous_time() < until, 'detached_worker_did_not_finish')
            time.sleep(.1)
        self.passed('frontend_disconnect_does_not_cancel_running_worker')
        self.start_server()
        with Profile(self.profile_args('client')) as profile:
            self.checkpoint('reattach_after_restart')
            client = self.client(profile)
            require(client.group == initial_group, 'session_not_reused')
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
            env={'PATH': '/usr/bin:/usr/sbin:/bin:/sbin', 'LANG': 'C', **execution_marker(), **bwrap_environment(),
                **({key: os.environ[key] for key in ('RUNNER_TRACKING_ID',) if key in os.environ})})
        require(result.returncode == 0 and result.stdout == b'marmish cli e2e\n' and result.stderr == b'',
                'cli_process_failed')
        self.passed('actual_client_and_server_cli_processes_execute_command_via_marmot')
        self.report.update(outcome='passed', stage='complete')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--live', action='store_true', required=True)
    parser.add_argument('--relay', action='append', required=True)
    parser.add_argument('--wnd', type=Path, required=True)
    parser.add_argument('--wnd-sha256', required=True)
    parser.add_argument('--wnd-manifest', type=Path)
    parser.add_argument('--bridge', type=Path, required=True)
    parser.add_argument('--worker', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=900)
    args = parser.parse_args(argv)
    require(60 <= args.timeout <= 900, 'invalid_timeout')
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
        harness.stop_server()
        if harness.inhibitor:
            harness.inhibitor.terminate()
            harness.inhibitor.wait(timeout=5)
        # Release bounded test workers even when an assertion fails, then let
        # detached workers finish before removing their encrypted state.
        for name in ('cancel-release', 'disconnect-release'):
            (harness.cwd / name).touch()
        engine_policy = {'version': 1, 'state_root': str(root / 'server/engine'),
            'worker_executable': str(args.worker.absolute()), 'server': harness.server_identity,
            'allowed_principals': [harness.client_identity], 'cwd': str(harness.cwd),
            'allow_shell_command': True, 'command_policy': load_json(root / 'command-policy.json')}
        until = continuous_time() + 65
        while True:
            try:
                if (root / 'server/engine').exists():
                    remove_engine_state(args.bridge, engine_policy)
                if sys.platform == 'darwin':
                    from marmish_transport_macos import cleanup_control, control_dir
                    for name in ('server', 'client'):
                        home = root / name / 'mdk'
                        if os.path.lexists(control_dir(home)):
                            cleanup_control(home)
                shutil.rmtree(root)
                report['local_cleanup'] = 'complete'
                break
            except (SmokeError, SandboxError, OSError, ValueError):
                if continuous_time() >= until:
                    report.update(outcome='failed', local_cleanup='incomplete',
                                  private_state_retained=True)
                    break
                time.sleep(.5)
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
    print(json.dumps(report, sort_keys=True))
    return 0 if report['outcome'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
