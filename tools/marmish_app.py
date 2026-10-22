"""Client/server frontends for the reviewed noninteractive Marmish prototype."""
import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import sys
import time

from mdk_smoke import SmokeError, identifier, public_key, relay_url, require
from marmish_protocol import (Dispatcher, EngineBridge, History, b64, check_members,
                              decode, encode, envelope, group_session, hex_id,
                              incoming, load_json, private_directory, save_json,
                              unb64, validate_frame)
from marmish_rpc import MdkRpc, RpcError
from marmish_transport_sandbox import ConfinedDaemon, SandboxError


class Profile:
    def __init__(self, args):
        self.root = private_directory(args.identity)
        self.path = self.root / 'profile.json'
        self.value = load_json(self.path, {})
        configured = sorted({relay_url(x) for x in args.relay})
        require(1 <= len(configured) <= 8, 'relay_count_limit')
        if self.value:
            require(self.value.get('relays') == configured, 'profile_relay_mismatch')
            public_key(self.value.get('account'))
        self.relays = configured
        self.transport = ConfinedDaemon(self.root / 'mdk', Path(args.wnd), configured,
                                        args.wnd_sha256)
        self.rpc = None
        self.account = None

    def __enter__(self):
        self.transport.start()
        self.rpc = MdkRpc(self.transport.socket, timeout=60)
        try:
            if not self.value:
                account = self.rpc.create_identity(self.relays)
                self.value = {'version': 1, 'account': account, 'relays': self.relays,
                              'sessions': {}, 'server_groups': {}}
                self.save()
            self.account = public_key(self.value['account'])
            return self
        except BaseException:
            self.transport.close()
            raise

    def save(self):
        save_json(self.path, self.value)

    def __exit__(self, *_args):
        self.transport.close()


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
        self.deadline = time.monotonic() + timeout
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
        require(time.monotonic() < self.deadline, 'client_timeout')
        return self.deadline - time.monotonic()

    def hello(self):
        nonce = secrets.token_hex(32)
        self.challenge = None
        result = self._exchange('hello', {'nonce': nonce})
        require(result.get('nonce') == nonce and result.get('lifetime') == 60, 'hello_mismatch')
        self.challenge = hex_id(result.get('challenge'), 32)
        self.challenge_until = time.monotonic() + 40

    def _exchange(self, op, body):
        rid = secrets.token_hex(16)
        request = envelope(self.sid, self.group, self.profile.account, self.peer,
                           rid, op, body, self.challenge)
        text = encode(request).decode('ascii')
        next_send = 0
        while True:
            self.remaining()
            if time.monotonic() >= next_send:
                # Retransmission preserves both the application request ID and invocation ID.
                self.rpc.send(self.profile.account, self.group, text)
                next_send = time.monotonic() + 8
            rows = self.rpc.history(self.profile.account, self.group)
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
        if time.monotonic() >= self.challenge_until:
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


class Server:
    def __init__(self, profile, allow, bridge, worker, cwd, allow_shell=False):
        self.profile = profile
        self.allow = set(allow)
        require(1 <= len(self.allow) <= 32 and profile.account not in self.allow, 'invalid_allowlist')
        self.engine = EngineBridge(bridge, {'version': 1,
            'state_root': str(profile.root / 'engine'), 'worker_executable': str(Path(worker).absolute()),
            'server': profile.account, 'allowed_principals': sorted(self.allow),
            'cwd': str(Path(cwd).absolute()), 'allow_shell_command': allow_shell})
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
        if time.monotonic() - self.last_maintenance >= 60:
            self.engine.call('maintain', limit=128)
            try:
                sweep = self.profile.rpc.expiry(self.profile.account)
            except RpcError:
                return
            for group in sweep.get('groups', []):
                if group.get('status') == 'failed' and group.get('group_id') in self.groups:
                    self._quarantine(group['group_id'])
            self.last_maintenance = time.monotonic()

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
    parser.add_argument('--wnd', required=True, type=Path)
    parser.add_argument('--wnd-sha256', required=True)


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
        if args.init:
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
    args = parser.parse_args(argv)
    os.umask(0o077)
    require(os.geteuid() != 0, 'root_forbidden')
    allow = [public_key(key) for key in args.allow]
    running = True
    def stop(_sig, _frame):
        nonlocal running
        running = False
    handlers = {sig: signal.signal(sig, stop) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        with Profile(args) as profile:
            server = Server(profile, allow, args.bridge, args.worker, args.cwd, args.allow_shell)
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
