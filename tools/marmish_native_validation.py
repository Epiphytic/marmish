"""Explicit disposable Linux native-provider acceptance context; never ambient.

No default-keychain operations run on macOS. Reports contain fixed categories,
public references and booleans only; neither credentials nor runtime paths.
"""
import argparse
import base64
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import secrets
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from xml.sax.saxutils import escape

from marmish_keys import CATEGORIES, _reference, _trusted_path, encode_nsec
from marmish_platform import continuous_time, execution_marker, process_identity
from marmish_protocol import decode, encode
from mdk_smoke import SmokeError, require

_ACTIVE = None
_DISCOVERY = ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME',
              'XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS', 'GNOME_KEYRING_CONTROL')


def _bounded(argv, env, payload=b'', timeout=12, limit=131072, accepted=(0,), inspect=None):
    """Owned process group, bounded private pipes, no diagnostic passthrough."""
    child = subprocess.Popen(argv, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL, start_new_session=True, close_fds=True)
    output = bytearray()
    reaped = False
    try:
        if inspect is not None:
            inspect(child.pid)
        with selectors.DefaultSelector() as selector:
            for stream in (child.stdin, child.stdout):
                os.set_blocking(stream.fileno(), False)
            selector.register(child.stdout, selectors.EVENT_READ)
            if payload:
                selector.register(child.stdin, selectors.EVENT_WRITE)
            else:
                child.stdin.close()
            offset = 0
            deadline = continuous_time() + timeout
            while selector.get_map():
                require(continuous_time() < deadline, 'native_operation_timeout')
                for key, _ in selector.select(min(.1, max(0, deadline - continuous_time()))):
                    if key.fileobj is child.stdin:
                        offset += os.write(child.stdin.fileno(), payload[offset:offset + 4096])
                        if offset == len(payload):
                            selector.unregister(child.stdin)
                            child.stdin.close()
                    else:
                        chunk = os.read(child.stdout.fileno(), 4096)
                        if chunk:
                            output.extend(chunk)
                            require(len(output) <= limit, 'native_output_limit')
                        else:
                            selector.unregister(child.stdout)
            require(continuous_time() < deadline, 'native_operation_timeout')
            try:
                status = child.wait(timeout=max(.001, deadline - continuous_time()))
            except subprocess.TimeoutExpired:
                raise SmokeError('native_operation_timeout') from None
            reaped = True
            require(status in accepted, 'native_operation_failed')
        return bytes(output)
    finally:
        if not reaped:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
        for stream in (child.stdin, child.stdout):
            stream.close()
        output[:] = b'\0' * len(output)


@contextmanager
def _blocked_cleanup_signals():
    previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM})
    try:
        yield
    finally:
        signal.pthread_sigmask(signal.SIG_SETMASK, previous)


def _interrupt(_number, _frame):
    raise SmokeError('native_interrupted')


def active_context():
    return _ACTIVE


def native_environment():
    if _ACTIVE is None:
        return {}
    _ACTIVE.validate()
    return {key: _ACTIVE.env[key] for key in _DISCOVERY}


class NativeLinuxContext:
    """One private session bus and one owned GNOME secrets service."""
    def __init__(self, helper, parent=None):
        self.helper = _trusted_path(Path(helper).absolute(), executable=True)
        self.parent = parent
        self.root = None
        self.children = []
        self.env = {}
        self.password = secrets.token_hex(32).encode() + b'\n'
        self.cleanup_complete = False
        self.private_state_retained = False
        self.secret_values = []

    def _spawn(self, argv, password=False):
        with _blocked_cleanup_signals():
            return self._spawn_registered(argv, password)

    def _spawn_registered(self, argv, password=False):
        child = subprocess.Popen(argv, env=self.env,
                                 stdin=subprocess.PIPE if password else subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                 start_new_session=True, close_fds=True)
        try:
            identity = process_identity(child.pid)
            self.children.append((child, identity))
            if password:
                child.stdin.write(self.password)
                child.stdin.close()
            return child
        except BaseException:
            # Unreaped Popen owns this PID; it cannot have been reused. Popen
            # returned after exec, so start_new_session established this group.
            with _blocked_cleanup_signals():
                try:
                    os.killpg(child.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                child.wait(timeout=5)
                if child.stdin is not None:
                    child.stdin.close()
                self.children = [(item, ident) for item, ident in self.children if item is not child]
            raise

    def __enter__(self):
        global _ACTIVE
        require(_ACTIVE is None and sys.platform.startswith('linux'), 'native_context_unavailable')
        for executable in ('dbus-daemon', 'gnome-keyring-daemon', 'gdbus'):
            _trusted_path(Path('/usr/bin') / executable, executable=True)
        self.root = Path(tempfile.mkdtemp(prefix='marmish-native-', dir=self.parent)).resolve()
        try:
            os.chmod(self.root, 0o700)
            _trusted_path(self.root)
            for name in ('home', 'config', 'data', 'cache', 'run', 'control'):
                (self.root / name).mkdir(mode=0o700)
            self.catalog = self.root / 'config/keys.json'
            self.key_name_prefix = 'native-' + secrets.token_hex(8)
            self.env = {'PATH': '/usr/bin:/usr/sbin:/bin:/sbin', **execution_marker()}
            for key, name in (('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                              ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache'),
                              ('XDG_RUNTIME_DIR', 'run'), ('GNOME_KEYRING_CONTROL', 'control')):
                self.env[key] = str(self.root / name)
            self.env['DBUS_SESSION_BUS_ADDRESS'] = 'unix:path=' + str(self.root / 'run/session-bus')
            # No servicedir or include: D-Bus activation cannot reach desktop services.
            config = ('<busconfig><type>session</type><listen>' +
                      escape(self.env['DBUS_SESSION_BUS_ADDRESS']) +
                      '</listen><auth>EXTERNAL</auth><policy context="default">'
                      '<allow send_destination="*"/><allow receive_sender="*"/>'
                      '<allow own="*"/></policy></busconfig>')
            (self.root / 'bus.conf').write_text(config)
            os.chmod(self.root / 'bus.conf', 0o600)
            self._spawn(['/usr/bin/dbus-daemon', '--nofork', '--config-file=' + str(self.root / 'bus.conf')])
            self._await(lambda: (self.root / 'run/session-bus').exists())
            self.validate()
            self._spawn(['/usr/bin/gnome-keyring-daemon', '--foreground', '--components=secrets',
                         '--control-directory', str(self.root / 'control'), '--unlock'], password=True)
            self._await(self._ready)
            _ACTIVE = self
            return self
        except BaseException:
            self.close()
            raise

    def _await(self, predicate):
        deadline = continuous_time() + 15
        while continuous_time() < deadline:
            if predicate():
                return
            time.sleep(.05)
        raise SmokeError('native_startup_timeout')

    def validate(self):
        require(self.root is not None and self.root.resolve() == self.root, 'native_context_invalid')
        for name in ('', 'home', 'config', 'data', 'cache', 'run', 'control'):
            info = (self.root / name).lstat()
            require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.geteuid()
                    and stat.S_IMODE(info.st_mode) == 0o700, 'native_context_invalid')
        expected = 'unix:path=' + str(self.root / 'run/session-bus')
        require(self.env.get('DBUS_SESSION_BUS_ADDRESS') == expected, 'native_context_invalid')
        socket_info = (self.root / 'run/session-bus').lstat()
        require(stat.S_ISSOCK(socket_info.st_mode) and socket_info.st_uid == os.geteuid(),
                'native_context_invalid')
        for child, identity in self.children:
            require(process_identity(child.pid) == identity, 'native_process_changed')

    def provider(self, action, **fields):
        self.validate()
        data = _bounded([str(self.helper)], self.env, encode({'version': 1, 'action': action, **fields}), accepted=(0, 1))
        response = decode(data, 131072)
        if (isinstance(response, dict) and set(response) == {'version', 'ok', 'error'}
                and type(response['version']) is int and response['version'] == 1
                and response['ok'] is False and isinstance(response['error'], str)
                and response['error'] in CATEGORIES):
            raise SmokeError(response['error'])
        require(isinstance(response, dict) and set(response) == {'version', 'ok', 'result'}
                and type(response['version']) is int and response['version'] == 1
                and response['ok'] is True and isinstance(response['result'], dict),
                'native_provider_failed')
        return response['result']

    def _ready(self):
        try:
            return self.provider('probe') == {}
        except SmokeError:
            return False

    def references(self):
        result = self.provider('list')
        require(set(result) == {'items'} and isinstance(result['items'], list)
                and len(result['items']) <= 128, 'native_metadata_invalid')
        refs = []
        for item in result['items']:
            require(isinstance(item, dict) and set(item) == {'reference', 'label'},
                    'native_metadata_invalid')
            ref = item['reference']
            require(isinstance(ref, dict) and ref.get('purpose') in ('nostr', 'sqlcipher'),
                    'native_metadata_invalid')
            refs.append(_reference(ref, ref['purpose']))
        require(len({(x['purpose'], x['id']) for x in refs}) == len(refs), 'native_metadata_invalid')
        return refs

    def _bus(self, method, *args):
        self.validate()
        return _bounded(['/usr/bin/gdbus', 'call', '--address', self.env['DBUS_SESSION_BUS_ADDRESS'],
                         '--dest', 'org.freedesktop.secrets', '--object-path',
                         '/org/freedesktop/secrets', '--method', method, *args], self.env)

    def lock(self):
        result = self._bus('org.freedesktop.Secret.Service.ReadAlias', 'default')
        match = re.fullmatch(rb"\(objectpath '(/org/freedesktop/secrets/collection/[A-Za-z0-9_]+)',\)\n", result)
        require(match is not None, 'native_collection_invalid')
        collection = match.group(1).decode('ascii')
        result = self._bus('org.freedesktop.Secret.Service.Lock', "[objectpath '" + collection + "']")
        require(b"objectpath '/'" in result, 'native_lock_prompt_refused')
        try:
            self.provider('probe')
        except SmokeError as error:
            require(str(error) == 'locked_or_denied', 'native_lock_failed')
        else:
            raise SmokeError('native_lock_failed')

    def unlock(self):
        """Restart only our verified secrets daemon with the original password.

        This GNOME fixture mechanism preserves persistent entries; it is not an
        implementation of unlocking an ambient desktop service.
        """
        self.validate()
        try:
            self.provider('probe')
        except SmokeError as error:
            require(str(error) == 'locked_or_denied', 'native_unlock_state_invalid')
        else:
            raise SmokeError('native_unlock_state_invalid')
        child, identity = self.children[-1]
        require(process_identity(child.pid) == identity, 'native_process_changed')
        os.killpg(child.pid, signal.SIGKILL)
        child.wait(timeout=5)
        self.children.pop()
        self._spawn(['/usr/bin/gnome-keyring-daemon', '--foreground', '--components=secrets',
                     '--control-directory', str(self.root / 'control'), '--unlock'], password=True)
        self._await(self._ready)

    def e2e_args(self):
        return ['--key-storage', 'auto', '--keychain-helper', str(self.helper),
                '--key-catalog', str(self.catalog), '--key-name-prefix', self.key_name_prefix]

    def retain_private_state(self):
        """Keep provider credentials when another owned component is not quiescent."""
        self.private_state_retained = True

    def remember_secrets(self, values):
        _needles(values)  # Validate without including values in diagnostics.
        self.secret_values.extend(value for value in values if value not in self.secret_values)

    def close(self):
        with _blocked_cleanup_signals():
            self._close()

    def _close(self):
        global _ACTIVE
        if _ACTIVE is self:
            _ACTIVE = None
        safe = True
        for child, identity in reversed(self.children):
            try:
                require(process_identity(child.pid) == identity, 'native_process_changed')
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=5)
            except (SmokeError, OSError, subprocess.TimeoutExpired):
                safe = False
        self.children.clear()
        if safe and self.root is not None and not self.private_state_retained:
            shutil.rmtree(self.root)
            self.cleanup_complete = True
        self.password = b''
        require(safe, 'native_cleanup_incomplete')

    def __exit__(self, *_args):
        self.close()


def _needles(secret_values):
    forms = []
    for secret in secret_values:
        require(isinstance(secret, bytes) and len(secret) == 32, 'native_scan_secret_invalid')
        forms.extend((secret, secret.hex().encode(), base64.b64encode(secret), encode_nsec(secret).encode()))
    return forms


def scan_at_rest(paths, secret_values, *, exclude=(), byte_limit=268435456):
    """Stream all regular files; exact provider-storage exclusion only by caller."""
    needles = _needles(secret_values)
    excluded = {Path(path).resolve() for path in exclude}
    total = 0
    for root in map(Path, paths):
        require(not root.is_symlink(), 'native_scan_unsafe_file')
        candidates = [root] if not root.is_dir() else root.rglob('*')
        for path in candidates:
            if any(path == item or item in path.parents for item in excluded):
                continue
            info = path.lstat()
            require(not stat.S_ISLNK(info.st_mode), 'native_scan_unsafe_file')
            if not stat.S_ISREG(info.st_mode):
                continue
            with path.open('rb') as source:
                tail = b''
                while True:
                    chunk = source.read(65536)
                    if not chunk:
                        break
                    total += len(chunk)
                    require(total <= byte_limit, 'native_scan_limit')
                    data = tail + chunk
                    require(not any(needle in data for needle in needles), 'native_secret_at_rest')
                    tail = data[-128:]
    return {'secret_at_rest': False}


def scan_process(pid, secret_values, *, expected_env_absent=()):
    identity = process_identity(pid)
    base = Path('/proc') / str(pid)
    require(base.stat().st_uid == os.geteuid(), 'native_scan_process_owner')
    argv, env = (base / 'cmdline').read_bytes(), (base / 'environ').read_bytes()
    require(process_identity(pid) == identity, 'native_scan_process_changed')
    needles = _needles(secret_values)
    require(not any(needle in argv or needle in env for needle in needles), 'native_secret_in_process')
    names = {item.split(b'=', 1)[0] for item in env.split(b'\0') if item}
    require(not any(name.encode() in names for name in expected_env_absent), 'native_environment_leak')
    return {'secret_in_process': False, 'environment_restricted': True}



_HOST_DISCOVERY = ('DBUS_SESSION_BUS_ADDRESS', 'GNOME_KEYRING_CONTROL', 'XDG_RUNTIME_DIR')


def _proc_record(pid):
    base = Path('/proc') / str(pid)
    if base.stat().st_uid != os.geteuid():
        return None
    raw = (base / 'stat').read_text()
    fields = raw[raw.rfind(')') + 2:].split()
    require(len(fields) > 19 and raw.split(' ', 1)[0] == str(pid), 'native_audit_invalid_process')
    return {'pid': pid, 'parent': int(fields[1]), 'start': int(fields[19])}


def _audit_process(record, secret_values, role=None):
    pid = record['pid']
    identity = process_identity(pid)
    require(identity['start'] == record['start'] * 1000000 // os.sysconf('SC_CLK_TCK'),
            'native_scan_process_changed')
    base = Path('/proc') / str(pid)
    executable = Path(os.readlink(base / 'exe')).name
    actual_role = {'wnd': 'daemon', 'marmish-worker': 'worker',
                   'marmish-engine-bridge': 'bridge', 'marmish-keychain': 'helper'}.get(executable)
    if role is not None:
        require(actual_role == role, 'native_audit_wrong_executable')
    scan_process(pid, secret_values,
                 expected_env_absent=_HOST_DISCOVERY if actual_role in ('daemon', 'worker', 'bridge') else ())
    if actual_role is not None:
        argv = (base / 'cmdline').read_bytes().split(b'\0')
        require(argv and Path(os.fsdecode(argv[0])).name == executable,
                'native_audit_wrong_executable')
        if actual_role in ('daemon', 'worker'):
            info = (base / 'fd/0').stat()
            null = Path('/dev/null').stat()
            require(stat.S_ISCHR(info.st_mode) and info.st_rdev == null.st_rdev,
                    'native_bootstrap_fd_open')
        else:
            require(all(stat.S_ISFIFO((base / ('fd/' + str(fd))).stat().st_mode)
                        for fd in (0, 1)), 'native_private_pipe_required')
    require(process_identity(pid) == identity, 'native_scan_process_changed')
    return actual_role


def audit_native_process(pid, secret_values, *, role):
    """Audit a recorded detached worker/daemon without reading unrelated argv."""
    require(sys.platform.startswith('linux') and role in ('daemon', 'worker'),
            'native_audit_unsupported')
    record = _proc_record(pid)
    require(record is not None, 'native_scan_process_owner')
    _audit_process(record, secret_values, role)
    return {'secret_in_process': False, 'bootstrap_fd0_closed': True,
            'host_discovery_absent': True}


def audit_tree(pid, secret_values, *, require_daemon=True, require_worker=False):
    """Bounded same-UID ancestry snapshot, then private descendant inspection.

    Optional children that disappear are skipped. A changed live identity fails;
    required daemon/worker presence is checked after successful inspection.
    Detached workers must additionally be audited by their recorded PID.
    """
    require(sys.platform.startswith('linux'), 'native_audit_unsupported')
    root_identity = process_identity(pid)
    records = {}
    entries = [item for item in Path('/proc').iterdir() if item.name.isdecimal()]
    require(len(entries) <= 4096, 'native_audit_process_limit')
    for entry in entries:
        try:
            record = _proc_record(int(entry.name))
        except FileNotFoundError:
            continue
        if record is not None:
            records[record['pid']] = record
    require(pid in records and process_identity(pid) == root_identity,
            'native_scan_process_changed')
    selected = {pid}
    for _ in range(len(records)):
        expanded = selected | {item['pid'] for item in records.values() if item['parent'] in selected}
        if expanded == selected:
            break
        require(len(expanded) <= 256, 'native_audit_process_limit')
        selected = expanded
    result = {'processes': 0, 'daemons': 0, 'workers': 0, 'bridges': 0, 'helpers': 0,
              'secret_in_process': False, 'bootstrap_fd0_closed': True,
              'host_discovery_absent': True}
    for child_pid in sorted(selected):
        try:
            role = _audit_process(records[child_pid], secret_values)
        except FileNotFoundError:
            require(child_pid != pid and not (Path('/proc') / str(child_pid)).exists(),
                    'native_scan_process_changed')
            continue
        except SmokeError as exc:
            # process_identity normalizes ENOENT; only a fresh /proc absence
            # permits skipping identity failures. Observed leaks remain failures
            # even if the inspected process exits immediately afterwards.
            if (str(exc) in ('process_identity_failed', 'native_scan_process_changed') and
                    child_pid != pid and not (Path('/proc') / str(child_pid)).exists()):
                continue
            raise
        result['processes'] += 1
        if role is not None:
            result[{'daemon': 'daemons', 'worker': 'workers', 'bridge': 'bridges',
                    'helper': 'helpers'}[role]] += 1
    require(process_identity(pid) == root_identity, 'native_scan_process_changed')
    require(not require_daemon or result['daemons'] >= 1, 'native_audit_missing_daemon')
    require(not require_worker or result['workers'] >= 1, 'native_audit_missing_worker')
    return result

def audit_helper(helper, env, secret_values):
    """Inspect one owned helper waiting on private protocol pipes, then probe."""
    executable = _trusted_path(Path(helper).absolute(), executable=True)
    def inspect(pid):
        record = _proc_record(pid)
        require(record is not None, 'native_scan_process_owner')
        _audit_process(record, secret_values, 'helper')
    output = _bounded([str(executable)], env, encode({'version': 1, 'action': 'probe'}),
                      timeout=5, inspect=inspect)
    require(decode(output, 131072) == {'version': 1, 'ok': True, 'result': {}},
            'native_provider_failed')
    return {'secret_in_process': False, 'private_pipes': True}


def scan_owned_stderr(secret_values, *, byte_limit=16777216):
    """Scan only a private, regular owned stderr; never consume pipes/terminals."""
    info = os.fstat(2)
    if not (stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
            and stat.S_IMODE(info.st_mode) & 0o077 == 0):
        return {'private_stderr_scanned': False}
    require(info.st_size <= byte_limit, 'native_scan_limit')
    fd = os.open('/proc/self/fd/2', os.O_RDONLY | os.O_CLOEXEC)
    try:
        opened = os.fstat(fd)
        require((opened.st_dev, opened.st_ino) == (info.st_dev, info.st_ino),
                'native_scan_unsafe_file')
        needles = _needles(secret_values)
        offset, tail = 0, b''
        while True:
            chunk = os.pread(fd, min(65536, byte_limit + 1 - offset), offset)
            if not chunk:
                break
            offset += len(chunk)
            require(offset <= byte_limit, 'native_scan_limit')
            data = tail + chunk
            require(not any(value in data for value in needles), 'native_secret_in_log')
            tail = data[-128:]
        return {'private_stderr_scanned': True}
    finally:
        os.close(fd)


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument('--helper', required=True, type=Path)
    args, rest = parser.parse_known_args(argv)
    if rest[:1] == ['--']:
        rest = rest[1:]
    if not sys.platform.startswith('linux'):
        print(json.dumps({'outcome': 'pending', 'platform': sys.platform,
                          'scope': 'native_default_keychain', 'category': 'dedicated_account_required'}))
        return 2
    reports = []
    context = None
    result = 1
    failure = None
    saved = dict(os.environ)
    handlers = {number: signal.getsignal(number) for number in (signal.SIGINT, signal.SIGTERM)}
    try:
        for number in handlers:
            signal.signal(number, _interrupt)
        context = NativeLinuxContext(args.helper)
        with context:
            env = dict(context.env)
            for key in ('MARMISH_BWRAP', 'MARMISH_BWRAP_SHA256'):
                if key in saved:
                    env[key] = saved[key]
            os.environ.clear()
            os.environ.update(env)
            import marmish_e2e
            try:
                result = marmish_e2e.main([*rest, *context.e2e_args()], report_sink=reports)
                require(len(reports) == 1 and isinstance(reports[0], dict), 'native_report_invalid')
                serialized = json.dumps(reports[0], sort_keys=True).encode()
                if any(needle in serialized for needle in _needles(context.secret_values)):
                    reports.clear()
                    raise SmokeError('native_secret_in_report')
                reports[0].update(scan_owned_stderr(context.secret_values))
            except BaseException:
                context.retain_private_state()
                raise
    except (SmokeError, OSError, ValueError):
        failure = 'native_cleanup_incomplete' if context is not None and not context.cleanup_complete else 'native_context_failed'
    finally:
        os.environ.clear()
        os.environ.update(saved)
        for number, handler in handlers.items():
            signal.signal(number, handler)
    report = reports[0] if len(reports) == 1 and isinstance(reports[0], dict) else {
        'platform': sys.platform, 'scope': 'native_provider_context'}
    report['provider_cleanup'] = ('complete' if context is not None
                                  and context.cleanup_complete
                                  and not context.private_state_retained else 'incomplete')
    if context is not None and (context.private_state_retained or not context.cleanup_complete):
        failure = 'native_cleanup_incomplete'
        report.update(cleanup='incomplete', private_state_retained=True)
    if failure is not None:
        report.update(outcome='failed', category=failure)
        result = 1
    # This is the sole report emission, after provider teardown has completed.
    print(json.dumps(report, sort_keys=True))
    return result


if __name__ == '__main__':
    sys.modules.setdefault('marmish_native_validation', sys.modules[__name__])
    raise SystemExit(main())
