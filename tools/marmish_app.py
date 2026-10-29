"""Client/server frontends for the reviewed noninteractive Marmish prototype."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import stat
import sys
import time

from mdk_smoke import SmokeError, identifier, public_key, relay_url, require
from marmish_protocol import (Dispatcher, EngineBridge, History, b64, check_members,
                              decode, encode, envelope, group_session, hex_id,
                              incoming, load_json, private_directory, save_json,
                              unb64, validate_frame)
from marmish_rpc import MdkRpc, RpcError
from marmish_policy import load_policy
from marmish_keys import KeySession, encode_nsec, protected_read, protected_unlink
from marmish_transport import ConfinedDaemon, SandboxError, acquire_profile_home_lock
from marmish_platform import continuous_time


def _pristine(path):
    return not path.exists() or (path.is_dir() and not path.is_symlink()
                                 and not any(path.iterdir()))


def _read_public_metadata(path):
    # MDK public metadata may be 0644; secret readers remain strictly 0600.
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and not info.st_mode & 0o022 and info.st_nlink == 1
                and info.st_size <= 1048576, 'unsafe_account_metadata')
        return decode(os.read(fd, 1048577), 1048576)
    finally:
        os.close(fd)


def _check_mdk_account(home, account, *, initialized=False, migration=False):
    """Return (secret path, pending setup) after validating the exact identity."""
    accounts = home / 'accounts'
    if not accounts.exists() and not accounts.is_symlink():
        require(not initialized, 'missing_account_state')
        return None, False
    private_directory(accounts)
    entries = list(accounts.iterdir())
    require(len(entries) <= 1 and all(item.name == account for item in entries),
            'account_mismatch')
    if not entries:
        require(not initialized, 'missing_account_state')
        return None, False
    directory = private_directory(entries[0])
    metadata = directory / 'account.json'
    if metadata.exists() or metadata.is_symlink():
        value = _read_public_metadata(metadata)
        require(value.get('label') == value.get('account_id_hex') == account
                and value.get('local_signing') is True
                and value.get('external_signing') is False
                and value.get('signed_out') is False, 'account_mismatch')
    else:
        require(not initialized, 'missing_account_state')
    setup = directory / '.account-setup.json'
    setup_pending = setup.exists() or setup.is_symlink()
    if setup_pending:
        require(not migration, 'pending_account_setup')
        value = _read_public_metadata(setup)
        require(set(value) == {'account_id_hex', 'reused_account_id_credential', 'kind', 'phase'}
                and value['account_id_hex'] == account
                and type(value['reused_account_id_credential']) is bool
                and value['kind'] == 'imported_identity'
                and value['phase'] in ('local_state_created', 'local_ready',
                    'bootstrap_publication_started', 'bootstrap_publication_confirmed',
                    'key_package_publication_started', 'key_package_publication_confirmed'),
                'invalid_account_setup')
    secret = directory / 'secret.json'
    if not migration:
        require(not secret.exists() and not secret.is_symlink(), 'mixed_key_storage')
    return secret, setup_pending


class Profile:
    def __init__(self, args):
        self.args = args
        self.root = private_directory(args.identity)
        self.path = self.root / 'profile.json'
        self.value = load_json(self.path, {})
        self.relays = sorted({relay_url(x) for x in args.relay})
        require(1 <= len(self.relays) <= 8, 'relay_count_limit')
        self._check_relays()
        self.transport = self.rpc = self.account = self.keys = None
        self.master = None
        self.migrated = False

    def _check_relays(self):
        if self.value:
            require(self.value.get('relays') == self.relays, 'profile_relay_mismatch')
            public_key(self.value.get('account'))

    def __enter__(self):
        args = self.args
        self.keys = KeySession(self.root, storage=getattr(args, 'key_storage', 'auto'),
                               key_name=getattr(args, 'key_name', None),
                               helper=getattr(args, 'keychain_helper', None),
                               catalog=getattr(args, 'key_catalog', None))
        self.keys.__enter__()
        try:
            self.value = self.keys.value
            self._check_relays()
            if getattr(args, 'migrate_keys_to_native', False):
                self._migrate()
                self.migrated = True
                return self
            require(not self.keys.migration, 'key_migration_required')
            require(bool(self.value) or _pristine(self.root / 'mdk'), 'nonpristine_key_state')
            binding = self.keys.prepare_nostr(existing_account=self.value.get('account'),
                                               pristine=_pristine(self.root / 'mdk'))
            native = binding.backend == 'native'
            bootstrap = None
            if native:
                bootstrap, setup_pending = self._prepare_native(binding)
            self.transport = ConfinedDaemon(self.root / 'mdk', Path(args.wnd), self.relays,
                args.wnd_sha256, manifest=getattr(args, 'wnd_manifest', None), bootstrap=bootstrap)
            self.transport.start()
            self.rpc = MdkRpc(self.transport.socket, timeout=60,
                rpc_home=self.transport.rpc_home, rpc_socket=self.transport.rpc_socket,
                peer_identity=self.transport.peer_identity,
                secret_store='MarmishStdin' if native else 'File')
            if native:
                self._authenticate_native(binding, setup_pending)
            elif not self.value:
                account = self.rpc.create_identity(self.relays)
                self.value = {'version': 1, 'account': account, 'relays': self.relays,
                              'sessions': {}, 'server_groups': {}}
                self.account = public_key(account)
                self.save()
            self.account = public_key(self.value['account'])
            return self
        except BaseException:
            self.__exit__()
            raise

    def _prepare_native(self, binding):
        """Publish recoverable bindings before passing a secret to the daemon."""
        self.account = public_key(binding.account)
        _, setup_pending = _check_mdk_account(self.root / 'mdk', self.account,
                                             initialized=self.value.get('initialized', False))
        if not self.value:
            self.value = {'version': 2, 'account': self.account, 'initialized': False,
                          'relays': self.relays, 'sessions': {}, 'server_groups': {}}
        if setup_pending:
            self.value['initialized'] = False
        self._prepare_master()
        self.save()
        frame = bytearray(b'MRMNST01' + binding.secret + bytes.fromhex(self.account))
        return frame, setup_pending

    def _authenticate_native(self, binding, setup_pending):
        """Complete interrupted Login before marking the exact account ready."""
        require(self.rpc.capabilities()['public_key'] == self.account, 'account_mismatch')
        accounts = self.rpc.accounts()
        if accounts:
            require(accounts == [self.account], 'account_mismatch')
        if setup_pending or not accounts:
            require(self.value.get('initialized') is False, 'missing_account_state')
            require(self.rpc.login(encode_nsec(binding.secret), self.relays) == self.account,
                    'account_mismatch')
        self.value['initialized'] = True
        self.save()

    def _migrate(self):
        require(bool(self.value), 'missing_profile')
        self.account = public_key(self.value['account'])
        home, engine = self.root / 'mdk', self.root / 'engine'
        has_engine = not _pristine(engine)
        require(not has_engine or getattr(self.args, 'server_mode', False),
                'server_migration_required')
        guard = None
        self.keys.lock_catalog()
        with acquire_profile_home_lock(home):
            secret_path, _ = _check_mdk_account(home, self.account, initialized=True, migration=True)
            require(secret_path is not None, 'missing_account_state')
            legacy_nsec = legacy_master = None
            legacy_secret_bytes = None
            if secret_path.exists() or secret_path.is_symlink():
                legacy_secret_bytes = protected_read(secret_path)
                value = decode(legacy_secret_bytes, 1048576)
                require(set(value) == {'version', 'backend', 'secret_key_hex'}
                        and type(value['version']) is int and value['version'] == 1
                        and value['backend'] == 'local-dev-file', 'invalid_legacy_secret')
                legacy_nsec = bytes.fromhex(hex_id(value['secret_key_hex'], 32))
            if has_engine and ((engine / 'engine.key').exists()
                               or (engine / 'engine.key').is_symlink()):
                raw = protected_read(engine / 'engine.key', 40)
                require(len(raw) == 40 and raw[:8] == b'MRMSKEY1', 'invalid_legacy_secret')
                legacy_master = raw[8:]
            self.keys.begin_migration(self.account)
            targets = self.keys.migration_targets(engine if has_engine else None)
            try:
                if has_engine:
                    target = targets['master']
                    master = legacy_master if legacy_master is not None else (
                        self.keys.load_migration_master())
                    storage = {'mode': 'migrate_native', 'root_id': target['root_id'],
                               'reference': target['reference'], 'master': b64(master)}
                    policy = _server_policy(self, self.args.allow, self.args.worker,
                        self.args.cwd, self.args.allow_shell, self.args.loaded_command_policy)
                    policy.update(version=2, storage=storage)
                    guard = EngineBridge(self.args.bridge, policy)
                self.keys.prepare_nostr(existing_account=self.account,
                                        migration_secret=legacy_nsec)
                if has_engine:
                    self.master = self.keys.prepare_master(engine, migration_secret=legacy_master)
                self.value.update(version=2, initialized=True)
                self.save()
                if guard is not None:
                    guard.call('migration_commit')
                if legacy_secret_bytes is not None:
                    protected_unlink(secret_path, legacy_secret_bytes)
                self.keys.mark_cleaned()
            finally:
                if guard is not None:
                    guard.close()

    def _prepare_master(self):
        if getattr(self.args, 'server_mode', False):
            root = self.root / 'engine'
            self.master = self.keys.prepare_master(root, pristine=_pristine(root))

    def engine_storage(self):
        if self.master is None or self.master.backend == 'files':
            return None
        return {'mode': 'native', 'root_id': self.master.root_id,
                'reference': self.master.reference, 'master': b64(self.master.secret)}

    def save(self):
        self.keys.commit(self.value)
        self.value = self.keys.value

    def __exit__(self, *_args):
        if self.transport is not None:
            self.transport.close()
        if self.keys is not None:
            self.keys.__exit__()
        self.master = None



def destination(value, user, relays):
    require(isinstance(value, str), 'invalid_destination')
    if '@' in value:
        remote, host = value.rsplit('@', 1)
        require(user is None, 'conflicting_remote_identity')
    else:
        remote, host = user, value
    peer = public_key(remote)
    origin = relay_url(host if host.startswith('wss://') else 'wss://' + host)
    require(origin in relays, 'destination_relay_not_configured')
    return peer


def verify_room(profile, group, peer, sid):
    shown = profile.rpc.show_group(profile.account, group)
    require(group_session(shown.get('group')) == sid, 'wrong_room')
    check_members(profile.rpc.members(profile.account, group), profile.account, peer)


class Client:
    def __init__(self, profile, peer, *, timeout=180, new_session=False, retention=86400):
        self.profile, self.rpc, self.peer = profile, profile.rpc, peer
        self.deadline = continuous_time() + timeout
        self.challenge = None
        self.challenge_until = 0
        self.history = History()
        key = hashlib.sha256(encode({'peer': peer, 'relays': profile.relays})).hexdigest()
        saved = profile.value['sessions'].get(key)
        if saved and not new_session:
            self.group = identifier(saved['group'])
            self.sid = hex_id(saved['session'])
        else:
            self.sid = secrets.token_hex(16)
            self.rpc.fetch_keys(profile.account, peer, profile.relays)
            created = self.rpc.create_group(profile.account, peer, self.sid,
                                            retention=f'{retention}s')
            self.group = identifier(created['group_id'])
            profile.value['sessions'][key] = {'group': self.group, 'session': self.sid}
            profile.save()
        verify_room(profile, self.group, peer, self.sid)
        self.history.scan(self.rpc.history(profile.account, self.group))
        self.hello()

    def remaining(self):
        require(continuous_time() < self.deadline, 'client_timeout')
        return self.deadline - continuous_time()

    def hello(self):
        nonce = secrets.token_hex(32)
        self.challenge = None
        result = self._exchange('hello', {'nonce': nonce})
        require(result.get('nonce') == nonce and result.get('lifetime') == 60, 'hello_mismatch')
        self.challenge = hex_id(result.get('challenge'), 32)
        self.challenge_until = continuous_time() + 40

    def _exchange(self, op, body):
        rid = secrets.token_hex(16)
        request = envelope(self.sid, self.group, self.profile.account, self.peer,
                           rid, op, body, self.challenge)
        text = encode(request).decode('ascii')
        next_send = 0
        while True:
            self.remaining()
            if continuous_time() >= next_send:
                # Retransmission preserves both the application request ID and invocation ID.
                self.rpc.send(self.profile.account, self.group, text)
                next_send = continuous_time() + 8
            rows = self.rpc.history(self.profile.account, self.group)
            self.remaining()
            for row in self.history.scan(rows):
                if not incoming(row, self.group, self.peer):
                    continue
                try:
                    reply = validate_frame(decode(row['plaintext']), self.sid, self.group,
                                           self.profile.account, self.peer, response=True)
                except SmokeError:
                    continue
                if reply['id'] != rid or reply['op'] != op or reply['challenge'] != request['challenge']:
                    continue
                result = reply['body']
                require(type(result.get('ok')) is bool, 'invalid_response')
                if not result['ok']:
                    error = result.get('error')
                    require(isinstance(error, str) and error.isascii() and error.replace('_', '').isalpha()
                            and len(error) <= 48, 'invalid_response')
                    raise SmokeError(error)
                require(isinstance(result.get('result'), dict), 'invalid_response')
                return result['result']
            time.sleep(min(.5, self.remaining()))

    def call(self, op, **body):
        if continuous_time() >= self.challenge_until:
            self.hello()
        return self._exchange(op, body)

    def journal_path(self, invocation):
        return private_directory(self.profile.root / 'invocations') / (hex_id(invocation) + '.json')

    def submit(self, command, invocation=None):
        self.submitted_new_input = False
        invocation = hex_id(invocation) if invocation else secrets.token_hex(16)
        path = self.journal_path(invocation)
        old = load_json(path)
        if old:
            require(old['server'] == self.peer and old.get('command') == command, 'invocation_conflict')
            return invocation, self.retry(invocation)
        token = self.call('token')['token']
        journal = {'server': self.peer, 'session': self.sid, 'group': self.group,
                   'invocation': invocation, 'command': command, 'token': token}
        save_json(path, journal)  # fsync BEFORE first command send
        submitted = self.call('submit', invocation=invocation, token=token, command=command)
        self.submitted_new_input = submitted['disposition'] == 'accepted'
        return invocation, submitted['status']

    def retry(self, invocation):
        self.submitted_new_input = False
        journal = load_json(self.journal_path(invocation))
        require(journal and journal.get('server') == self.peer, 'missing_invocation_journal')
        try:
            return self.call('status', invocation=invocation)
        except SmokeError as error:
            if str(error) != 'not_found':
                raise
        require(isinstance(journal.get('command'), dict), 'missing_command_journal')
        journal['token'] = self.call('token')['token']
        save_json(self.journal_path(invocation), journal)
        try:
            submitted = self.call('submit', invocation=invocation, token=journal['token'],
                                  command=journal['command'])
            self.submitted_new_input = submitted['disposition'] == 'accepted'
            return submitted['status']
        except SmokeError as error:
            if str(error) != 'invalid_token':
                raise
            return self.call('status', invocation=invocation)

    def send_input(self, invocation, stream):
        data = stream.read(4096)
        try:
            status = self.call('begin_input', invocation=invocation)
        except SmokeError as error:
            if str(error) != 'input_closed':
                raise
            return  # Empty stdin is already closed by a completed worker.
        generation = status['generation']
        offset = 0
        while data:
            self.remaining()
            try:
                self.call('write_stdin', invocation=invocation, generation=generation,
                          offset=offset, bytes=b64(data), fresh_ms=60000)
            except SmokeError as error:
                if str(error) != 'input_closed':
                    raise
                return  # A command may finish without consuming all stdin.
            offset += len(data)
            data = stream.read(4096)
        try:
            self.call('stdin_eof', invocation=invocation, generation=generation, final_offset=offset)
        except SmokeError as error:
            if str(error) != 'input_closed':
                raise

    def wait(self, invocation, stdout, stderr, *, scrub=True):
        cursors = {'stdout': 0, 'stderr': 0}
        while True:
            result = self.call('poll', invocation=invocation, stdout_cursor=cursors['stdout'],
                               stderr_cursor=cursors['stderr'])
            done = True
            for stream, output in (('stdout', stdout), ('stderr', stderr)):
                chunk = result[stream]
                require(chunk['status'].get('gaps') == [], 'output_gap')
                data = unb64(chunk['bytes'])
                require(type(chunk.get('next_cursor')) is int
                        and chunk['next_cursor'] == cursors[stream] + len(data), 'invalid_cursor')
                output.write(data)
                output.flush()
                cursors[stream] = chunk['next_cursor']
                done &= chunk['status']['closed'] and cursors[stream] >= chunk['status']['produced']
            status = result['status']
            if status['state'] in ('unknown_launch', 'lost'):
                raise SmokeError('execution_uncertain')
            if status['state'] == 'exited' and done:
                if scrub:
                    self.scrub(invocation)
                return status
            time.sleep(min(.5, self.remaining()))

    def scrub(self, invocation):
        path = self.journal_path(invocation)
        journal = load_json(path)
        if journal:
            for key in ('token', 'command'):
                journal.pop(key, None)
            save_json(path, journal)


def _server_policy(profile, allow, worker, cwd, allow_shell, command_policy):
    return {'version': 1, 'state_root': str(profile.root / 'engine'),
            'worker_executable': str(Path(worker).absolute()), 'server': profile.account,
            'allowed_principals': sorted(allow), 'cwd': str(Path(cwd).absolute()),
            'allow_shell_command': allow_shell, 'command_policy': command_policy}


class Server:
    def __init__(self, profile, allow, bridge, worker, cwd, allow_shell=False,
                 *, command_policy=None):
        self.profile = profile
        self.allow = set(allow)
        require(1 <= len(self.allow) <= 32 and profile.account not in self.allow, 'invalid_allowlist')
        policy = _server_policy(profile, self.allow, worker, cwd, allow_shell, command_policy)
        storage = profile.engine_storage() if isinstance(profile, Profile) else None
        if storage is not None:
            policy.update(version=2, storage=storage)
        self.engine = EngineBridge(bridge, policy)
        self.dispatcher = Dispatcher(self.engine, profile.account)
        self.groups = {}
        self.histories = {}
        self.last_maintenance = 0
        try:
            self.quarantine = profile.value.setdefault('server_quarantine', {})
            require(isinstance(self.quarantine, dict) and len(self.quarantine) <= 256,
                    'invalid_quarantine_state')
            self.pending = profile.value.setdefault('server_pending', {})
            require(isinstance(self.pending, dict) and len(self.pending) <= 32,
                    'invalid_pending_state')
            for group, binding in list(profile.value.get('server_groups', {}).items()):
                if group in self.quarantine:
                    continue
                try:
                    require(isinstance(binding, dict), 'invalid_room_binding')
                    peer = binding.get('peer')
                    if peer in self.allow:
                        self.enroll(group, peer, binding.get('session'))
                except RpcError:
                    self.pending[group] = binding
                    self.profile.save()
                except SmokeError:
                    self._quarantine(group)
        except BaseException:
            self.engine.close()
            raise

    def enroll(self, group, peer, sid):
        require(len(self.groups) < 32 or group in self.groups, 'session_quota')
        require(group not in self.quarantine, 'room_quarantined')
        identifier(group)
        hex_id(sid)
        verify_room(self.profile, group, peer, sid)
        history = History()
        history.scan(self.profile.rpc.history(self.profile.account, group))
        self.groups[group] = {'peer': peer, 'session': sid}
        self.histories[group] = history
        self.pending.pop(group, None)
        self.profile.value['server_groups'][group] = self.groups[group]
        self.profile.save()

    def _quarantine(self, group):
        self.groups.pop(group, None)
        self.histories.pop(group, None)
        self.pending.pop(group, None)
        self.profile.value.setdefault('server_groups', {}).pop(group, None)
        if len(self.quarantine) < 256:
            self.quarantine[group] = 'room_fault'
        else:
            self.profile.value['server_quarantine_full'] = True
        self.profile.save()

    def invitations(self):
        try:
            invites = self.profile.rpc.invites(self.profile.account)
        except RpcError:
            invites = []
        for invite in invites:
            peer = invite.get('welcomer_account_id')
            if peer not in self.allow or invite.get('pending_confirmation') is not True:
                continue
            try:
                group = identifier(invite.get('group_id'))
                sid = group_session(invite, peer)
            except SmokeError:
                continue
            if group in self.groups or group in self.quarantine or group in self.pending:
                continue
            if len(set(self.groups) | set(self.pending)) >= 32 or self.profile.value.get('server_quarantine_full'):
                continue
            self.pending[group] = {'peer': peer, 'session': sid}
            self.profile.save()
        self._reconcile_pending()

    def _reconcile_pending(self):
        for group, binding in list(self.pending.items()):
            if group in self.quarantine:
                continue
            try:
                require(isinstance(binding, dict) and binding.get('peer') in self.allow,
                        'invalid_room_binding')
                shown = self.profile.rpc.show_group(self.profile.account, group)
                room = shown.get('group')
                require(isinstance(room, dict) and type(room.get('pending_confirmation')) is bool,
                        'invalid_room_confirmation')
                if room['pending_confirmation']:
                    accepted = self.profile.rpc.accept(self.profile.account, group)
                    require(accepted.get('accepted') is True, 'invite_not_accepted')
                self.enroll(group, binding['peer'], binding.get('session'))
            except RpcError:
                continue
            except SmokeError:
                self._quarantine(group)

    def tick(self):
        self.invitations()
        for group, binding in list(self.groups.items()):
            try:
                self._tick_group(group, binding)
            except RpcError:
                continue
            except SmokeError:
                self._quarantine(group)
        if continuous_time() - self.last_maintenance >= 60:
            self.engine.call('maintain', limit=128)
            try:
                sweep = self.profile.rpc.expiry(self.profile.account)
            except RpcError:
                return
            for group in sweep.get('groups', []):
                if group.get('status') == 'failed' and group.get('group_id') in self.groups:
                    self._quarantine(group['group_id'])
            self.last_maintenance = continuous_time()

    def _tick_group(self, group, binding):
        peer, sid = binding['peer'], binding['session']
        verify_room(self.profile, group, peer, sid)
        rows = self.profile.rpc.history(self.profile.account, group)
        fresh = self.histories[group].scan(rows)
        for row in fresh:
            if not incoming(row, group, peer):
                continue
            try:
                frame = decode(row['plaintext'])
                reply = self.dispatcher.dispatch(frame, sid, group, peer)
            except SmokeError:
                continue
            self.profile.rpc.send(self.profile.account, group, encode(reply).decode('ascii'))
        if fresh:
            self.profile.rpc.mark_read(self.profile.account, group, fresh[-1]['message_id'])

    def close(self):
        self.engine.close()


def common(parser):
    parser.add_argument('-i', '--identity', required=True, type=Path,
                        help='private persistent local identity directory')
    parser.add_argument('--relay', action='append', required=True, help='explicit wss root URL')
    parser.add_argument('--key-storage', choices=('auto', 'native', 'files'), default='auto')
    parser.add_argument('--key-name', help='local named Nostr key bound to this profile')
    parser.add_argument('--keychain-helper', type=Path, help='absolute native helper executable')
    parser.add_argument('--key-catalog', type=Path, help='private named-key catalog path')
    parser.add_argument('--migrate-keys-to-native', action='store_true',
                        help='migrate existing keys, preserving identity and encrypted state')
    parser.add_argument('--wnd', required=True, type=Path)
    parser.add_argument('--wnd-sha256', required=True)
    parser.add_argument('--wnd-manifest', type=Path,
                        help='daemon build manifest (macOS defaults to <wnd>.manifest.json)')


def exit_code(status):
    outcome = status.get('outcome')
    require(isinstance(outcome, dict), 'missing_exit_status')
    if outcome.get('kind') == 'code':
        return outcome['value'] if 0 <= outcome['value'] <= 255 else 1
    if outcome.get('kind') == 'signal':
        return min(255, 128 + outcome['value'])
    return 1


def client_main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    command = []
    if '--' in argv:
        divider = argv.index('--')
        command, argv = argv[divider + 1:], argv[:divider]
    parser = argparse.ArgumentParser(description=__doc__)
    common(parser)
    parser.add_argument('destination', nargs='?', help='npub@relay or relay with -u npub')
    parser.add_argument('-u', '--user', help='remote Nostr identity')
    parser.add_argument('--init', action='store_true', help='generate/persist local identity and print public key')
    parser.add_argument('--new-session', action='store_true')
    parser.add_argument('--retention', type=int, default=86400, help='application message lifetime in seconds')
    parser.add_argument('--timeout', type=int, default=180)
    parser.add_argument('--detach', action='store_true')
    parser.add_argument('--invocation', help='explicit 16-byte hex invocation ID')
    actions = parser.add_mutually_exclusive_group()
    actions.add_argument('--status', metavar='INVOCATION')
    actions.add_argument('--wait', metavar='INVOCATION')
    actions.add_argument('--cancel', metavar='INVOCATION')
    actions.add_argument('--retry', metavar='INVOCATION')
    actions.add_argument('--shell-command', help='explicit shell interpretation; requires server permission')
    args = parser.parse_args(argv)
    os.umask(0o077)
    with Profile(args) as profile:
        if args.init or profile.migrated:
            print(json.dumps({'account': profile.account}))
            return 0
        peer = destination(args.destination, args.user, profile.relays)
        require(1 <= args.timeout <= 900 and 1 <= args.retention <= 604800, 'invalid_configuration')
        client = Client(profile, peer, timeout=args.timeout, new_session=args.new_session,
                        retention=args.retention)
        if args.status or args.cancel:
            invocation = hex_id(args.status or args.cancel)
            status = client.call('status' if args.status else 'cancel', invocation=invocation)
            if status['state'] == 'exited':
                client.scrub(invocation)
            print(json.dumps(status))
            return 0
        if args.wait or args.retry:
            invocation = hex_id(args.wait or args.retry)
            if args.retry:
                client.retry(invocation)
        else:
            require(bool(command) != (args.shell_command is not None), 'missing_or_conflicting_command')
            request = ({'mode': 'shell', 'command': b64(os.fsencode(args.shell_command))}
                       if args.shell_command is not None else
                       {'mode': 'exec', 'argv': [b64(os.fsencode(arg)) for arg in command]})
            invocation, _ = client.submit(request, args.invocation)
        if getattr(client, 'submitted_new_input', False):
            if sys.stdin.isatty():
                import io
                source = io.BytesIO()
            else:
                source = sys.stdin.buffer
            client.send_input(invocation, source)
        if args.detach:
            print(json.dumps({'invocation': invocation, 'group': client.group}))
            return 0
        return exit_code(client.wait(invocation, sys.stdout.buffer, sys.stderr.buffer))


def server_main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    common(parser)
    parser.add_argument('--allow', action='append', required=True)
    parser.add_argument('--bridge', required=True, type=Path)
    parser.add_argument('--worker', required=True, type=Path)
    parser.add_argument('--cwd', required=True, type=Path)
    parser.add_argument('--allow-shell', action='store_true')
    parser.add_argument('--command-policy', type=Path,
                        help='trusted JSON file with additive default/per-key executable restrictions')
    args = parser.parse_args(argv)
    os.umask(0o077)
    require(os.geteuid() != 0, 'root_forbidden')
    args.server_mode = True
    allow = [public_key(key) for key in args.allow]
    command_policy = load_policy(args.command_policy, allow)
    args.allow = allow
    args.loaded_command_policy = command_policy
    running = True
    def stop(_sig, _frame):
        nonlocal running
        running = False
    handlers = {sig: signal.signal(sig, stop) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        with Profile(args) as profile:
            if profile.migrated:
                print(json.dumps({'migrated': True, 'account': profile.account}))
                return 0
            server = Server(profile, allow, args.bridge, args.worker, args.cwd, args.allow_shell,
                            command_policy=command_policy)
            try:
                print(json.dumps({'ready': True, 'account': profile.account}), flush=True)
                while running:
                    server.tick()
                    time.sleep(.5)
            finally:
                server.close()
    finally:
        for sig, handler in handlers.items():
            signal.signal(sig, handler)
    return 0


def guarded_main(main):
    try:
        return main()
    except (SmokeError, RpcError, SandboxError) as error:
        # Categories only; never print MDK stdout, command contents, token or key material.
        category = str(error)
        if not category.isascii() or not category.replace('_', '').isalnum() or len(category) > 80:
            category = 'transport_failure'
        print('marmish: ' + category, file=sys.stderr)
        return 125
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        print('marmish: local_io_or_schema_failure', file=sys.stderr)
        return 125
