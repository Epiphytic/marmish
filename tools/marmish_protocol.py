"""Bounded application protocol for the Linux noninteractive prototype."""
import base64
from collections import OrderedDict
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import select
import stat
import subprocess
import time

from mdk_smoke import SmokeError, public_key, identifier, require
from marmish_rpc import strict_json, RpcError

PROTOCOL = 'marmish/1'
PURPOSE = 'marmish/v1 command transport'
FRAME_LIMIT = 32768
OPS = {
    'token': set(), 'submit': {'invocation', 'token', 'command'},
    'status': {'invocation'}, 'cancel': {'invocation'},
    'begin_input': {'invocation'},
    'write_stdin': {'invocation', 'generation', 'offset', 'bytes', 'fresh_ms'},
    'stdin_eof': {'invocation', 'generation', 'final_offset'},
    'poll': {'invocation', 'stdout_cursor', 'stderr_cursor'},
}


def hex_id(value, size=16):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{%d}' % (size * 2), value),
            'invalid_identifier')
    return value


def encode(value):
    try:
        return json.dumps(value, ensure_ascii=True, allow_nan=False, sort_keys=True,
                          separators=(',', ':')).encode('ascii')
    except (ValueError, TypeError, UnicodeError, RecursionError):
        raise SmokeError('invalid_json') from None


def decode(data, limit=FRAME_LIMIT):
    require(isinstance(data, (str, bytes)) and len(data) <= limit, 'frame_limit')
    try:
        value = strict_json(data)
        # Exponent overflow (e.g. 1e999) is legal JSON syntax but becomes inf.
        # Check the complete tree before it reaches frame validation or hashing.
        encode(value)
    except (ValueError, UnicodeError, RecursionError, RpcError):
        raise SmokeError('invalid_json') from None
    require(isinstance(value, dict), 'invalid_frame')
    return value


def private_directory(path):
    path = Path(path).absolute()
    # Reject symlink components even when a later resolve would hide them.
    for ancestor in [*reversed(path.parents), path]:
        if ancestor.exists() or ancestor.is_symlink():
            require(not ancestor.is_symlink(), 'unsafe_state_path')
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.lstat()
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.geteuid()
            and info.st_mode & 0o077 == 0, 'unsafe_state_path')
    return path


def save_json(path, value):
    path = Path(path)
    private_directory(path.parent)
    data = encode(value)
    require(len(data) <= 1024 * 1024, 'state_limit')
    temporary = path.parent / ('.pending-' + secrets.token_hex(12))
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'wb') as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def load_json(path, default=None):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return default
    with os.fdopen(fd, 'rb') as handle:
        info = os.fstat(handle.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and info.st_mode & 0o077 == 0 and info.st_size <= 1024 * 1024,
                'unsafe_state_file')
        return decode(handle.read(1024 * 1024 + 1), 1024 * 1024)


def group_session(group, peer=None):
    require(isinstance(group, dict) and isinstance(group.get('profile'), dict), 'wrong_room')
    profile = group['profile']
    name = profile.get('name')
    require(isinstance(name, str) and name.startswith('marmish/v1/')
            and profile.get('description') == PURPOSE, 'wrong_room')
    sid = hex_id(name[len('marmish/v1/'):])
    if peer is not None:
        require(public_key(group.get('welcomer_account_id')) == peer, 'wrong_inviter')
    return sid


def check_members(rows, local, peer):
    require(isinstance(rows, list) and len(rows) == 2, 'membership_mismatch')
    keys = []
    for row in rows:
        require(isinstance(row, dict), 'membership_mismatch')
        key = public_key(row.get('member_id'))
        require(public_key(row.get('npub')) == key and row.get('local') is (key == local),
                'membership_mismatch')
        keys.append(key)
    require(len(set(keys)) == 2 and set(keys) == {local, peer}, 'membership_mismatch')


def envelope(session, group, client, server, request_id, op, body, challenge=None,
             response=False):
    return {'protocol': PROTOCOL, 'session': session, 'group': group,
            'client': client, 'server': server, 'id': request_id,
            'type': 'response' if response else 'request', 'op': op,
            'challenge': challenge, 'body': body}


def validate_frame(value, session, group, client, server, response=False):
    require(set(value) == {'protocol', 'session', 'group', 'client', 'server', 'id',
                           'type', 'op', 'challenge', 'body'}, 'invalid_frame')
    require(value['protocol'] == PROTOCOL and value['session'] == session
            and value['group'] == group and value['client'] == client
            and value['server'] == server
            and value['type'] == ('response' if response else 'request'), 'wrong_binding')
    hex_id(value['id'])
    require(isinstance(value['op'], str) and value['op'] in {*OPS, 'hello'}, 'unsupported_operation')
    require(isinstance(value['body'], dict), 'invalid_frame')
    if value['challenge'] is not None:
        hex_id(value['challenge'], 32)
    require(len(encode(value)) <= FRAME_LIMIT, 'frame_limit')
    return value


class History:
    """Overlap detection; timestamps never become a permanent cursor."""
    def __init__(self):
        self.seen = OrderedDict()
        self.baselined = False

    def scan(self, rows):
        require(isinstance(rows, list) and len(rows) <= 100, 'invalid_history')
        ids = [identifier(row.get('message_id')) for row in rows]
        require(len(ids) == len(set(ids)), 'invalid_history')
        # MDK reverses its latest-N query: rows are oldest first within the window.
        if self.baselined and len(ids) == 100:
            require(ids[0] in self.seen, 'history_gap')
        fresh = [row for row, mid in zip(rows, ids) if mid not in self.seen]
        for mid in ids:
            self.seen[mid] = None
            self.seen.move_to_end(mid)
        while len(self.seen) > 2048:
            self.seen.popitem(last=False)
        if not self.baselined:
            self.baselined = True
            return []
        return fresh


def incoming(row, group, peer):
    return (isinstance(row, dict) and row.get('group_id') == group
            and row.get('direction') == 'received' and row.get('from') == peer
            and type(row.get('kind')) is int and row['kind'] == 30100
            and isinstance(row.get('tags'), list) and ['marmish', 'v1'] in row['tags']
            and isinstance(row.get('plaintext'), str))


class EngineBridge:
    def __init__(self, executable, policy):
        self.child = subprocess.Popen([str(executable)], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                      env={'PATH': '/usr/bin:/bin', 'LANG': 'C'},
                                      bufsize=0, start_new_session=True)
        self.pending = bytearray()
        self.broken = False
        try:
            self.call('init', **policy)
        except BaseException:
            self.close()
            raise

    def call(self, op, **body):
        require(not self.broken, 'bridge_broken')
        request_id = secrets.token_hex(16)
        data = encode({'id': request_id, 'op': op, **body}) + b'\n'
        require(len(data) <= 131072, 'bridge_frame_limit')
        deadline = time.monotonic() + 20
        try:
            os.set_blocking(self.child.stdin.fileno(), False)
            offset = 0
            while offset < len(data):
                remaining = deadline - time.monotonic()
                require(remaining > 0, 'bridge_timeout')
                require(select.select([], [self.child.stdin], [], remaining)[1], 'bridge_timeout')
                written = os.write(self.child.stdin.fileno(), data[offset:])
                require(written > 0, 'bridge_closed')
                offset += written
            while b'\n' not in self.pending:
                remaining = deadline - time.monotonic()
                require(remaining > 0, 'bridge_timeout')
                require(select.select([self.child.stdout], [], [], remaining)[0], 'bridge_timeout')
                part = os.read(self.child.stdout.fileno(), 65536)
                require(part, 'bridge_closed')
                self.pending.extend(part)
                require(len(self.pending) <= 131072, 'bridge_frame_limit')
            line, _, tail = self.pending.partition(b'\n')
            # This bridge is strictly stop-and-wait: unsolicited bytes cannot
            # be a response to a request that has not yet been sent.
            require(not tail, 'bridge_schema')
            self.pending.clear()
            value = decode(bytes(line), 131072)
            require(value.get('id') == request_id and type(value.get('ok')) is bool,
                    'bridge_schema')
            if not value['ok']:
                require(set(value) == {'id', 'ok', 'error'}
                        and isinstance(value['error'], dict)
                        and set(value['error']) == {'code'}, 'bridge_schema')
                error = value['error']['code']
                require(isinstance(error, str) and re.fullmatch('[a-z_]{1,48}', error), 'bridge_schema')
            else:
                require(set(value) == {'id', 'ok', 'result'}
                        and isinstance(value['result'], dict), 'bridge_schema')
        except BaseException as failure:
            # A failed exchange may have sent an operation or left a late reply
            # in flight. Never reuse that stream or risk another invocation.
            self.close()
            if isinstance(failure, (OSError, ValueError)):
                raise SmokeError('bridge_io_failure') from None
            raise
        if not value['ok']:
            raise SmokeError(error)
        return value['result']

    def close(self):
        self.broken = True
        self.pending.clear()
        if self.child.poll() is None:
            self.child.terminate()
            try:
                self.child.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait(timeout=2)
        self.child.stdin.close()
        self.child.stdout.close()


class Dispatcher:
    """Only verified group/sender bindings can reach this engine gateway."""
    def __init__(self, engine, server):
        self.engine = engine
        self.server = server
        self.boot = secrets.token_hex(32)
        self.challenges = OrderedDict()
        self.replies = OrderedDict()

    def _engine(self, op, peer, group, body):
        endpoint = hashlib.sha256(bytes.fromhex(group)).hexdigest()
        deadline = time.monotonic() + 5
        while True:
            try:
                return self.engine.call(op, principal=peer, endpoint=endpoint, **body)
            except SmokeError as error:
                if str(error) not in ('busy', 'already_open') or time.monotonic() >= deadline:
                    raise
                time.sleep(.05)

    def dispatch(self, frame, session, group, peer):
        validate_frame(frame, session, group, peer, self.server)
        op, body = frame['op'], frame['body']
        key = (group, peer, frame['id'])
        digest = hashlib.sha256(encode(frame)).digest()
        now = time.monotonic()
        if op == 'hello':
            require(set(body) == {'nonce'} and frame['challenge'] is None, 'invalid_frame')
            hex_id(body['nonce'], 32)
        else:
            require(set(body) == OPS[op], 'invalid_frame')
            binding = self.challenges.get(frame['challenge'])
            if not (binding is not None and binding[:3] == (session, group, peer)
                    and binding[4] == self.boot and now < binding[5]):
                # Keep authentication failures out of the successful retry cache.
                return envelope(session, group, peer, self.server, frame['id'], op,
                                {'ok': False, 'error': 'stale_challenge'},
                                frame['challenge'], response=True)
        if key in self.replies:
            old_digest, reply = self.replies[key]
            require(old_digest == digest, 'request_conflict')
            return reply
        try:
            if op == 'hello':
                challenge = secrets.token_hex(32)
                self.challenges[challenge] = (session, group, peer, body['nonce'], self.boot, now + 60)
                # Never evict a valid challenge to admit another one.
                for token, binding in list(self.challenges.items()):
                    if now >= binding[5]:
                        del self.challenges[token]
                if len(self.challenges) > 256:
                    del self.challenges[challenge]
                    raise SmokeError('challenge_quota')
                result = {'nonce': body['nonce'], 'challenge': challenge, 'lifetime': 60}
            elif op == 'poll':
                invocation = body['invocation']
                hex_id(invocation)
                for cursor in (body['stdout_cursor'], body['stderr_cursor']):
                    require(type(cursor) is int and 0 <= cursor < 2**63, 'invalid_cursor')
                result = {'status': self._engine('status', peer, group, {'invocation': invocation})}
                for stream in ('stdout', 'stderr'):
                    result[stream] = self._engine('read_output', peer, group, {
                        'invocation': invocation, 'stream': stream,
                        'cursor': body[stream + '_cursor'], 'limit': 4096})
            else:
                result = self._engine(op, peer, group, body)
            reply_body = {'ok': True, 'result': result}
        except SmokeError as error:
            category = str(error)
            require(re.fullmatch('[a-z_]{1,48}', category), 'internal_failure')
            reply_body = {'ok': False, 'error': category}
        reply = envelope(session, group, peer, self.server, frame['id'], op, reply_body,
                         frame['challenge'], response=True)
        require(len(encode(reply)) <= FRAME_LIMIT, 'frame_limit')
        self.replies[key] = (digest, reply)
        while len(self.replies) > 512:
            self.replies.popitem(last=False)
        return reply


def b64(data):
    return base64.b64encode(data).decode('ascii')


def unb64(data):
    try:
        require(isinstance(data, str) and len(data) <= 8192, 'invalid_bytes')
        value = base64.b64decode(data, validate=True)
        require(len(value) <= 4096 and b64(value) == data, 'invalid_bytes')
        return value
    except ValueError:
        raise SmokeError('invalid_bytes') from None
