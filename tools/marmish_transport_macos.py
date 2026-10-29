"""Fail-closed native macOS MDK confinement and bounded SOCKS domain broker.

Native readiness includes the maintained-patch acknowledgement and empirical
Seatbelt probes. Linux tests exercise parsing/lifecycle without claiming Darwin
kernel behavior; the actual runner must pass these same probes.
"""
from __future__ import annotations

import contextlib
import fcntl
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
from urllib.parse import urlsplit

from marmish_mdk_build import verify_manifest, digest, strict_object
from marmish_platform import continuous_time, process_identity, execution_marker, inspect_process
from marmish_transport_sandbox import SandboxError, canonical_relays, _private

MAX_CONNECTIONS = 32
BUFFER = 16384
CONNECT_SECONDS = 5
IDLE_SECONDS = 660


def _read_exact(stream, count, deadline):
    value = bytearray()
    while len(value) < count:
        remaining = deadline - continuous_time()
        if remaining <= 0:
            raise OSError('socks_timeout')
        stream.settimeout(min(.2, remaining))
        try:
            data = stream.recv(count - len(value))
        except socket.timeout:
            continue
        if not data:
            raise OSError('socks_eof')
        value.extend(data)
    return bytes(value)


def socks_target(stream, allowed, deadline):
    """Parse at most 518 bytes; reject before resolver or upstream creation."""
    version, count = _read_exact(stream, 2, deadline)
    if version != 5 or not 1 <= count <= 255:
        raise OSError('socks_greeting')
    methods = _read_exact(stream, count, deadline)
    if 0 not in methods:
        raise OSError('socks_method')
    stream.sendall(b'\x05\x00')
    if _read_exact(stream, 4, deadline) != b'\x05\x01\x00\x03':
        raise OSError('socks_domain_required')
    length = _read_exact(stream, 1, deadline)[0]
    if not 1 <= length <= 253:
        raise OSError('socks_name')
    raw = _read_exact(stream, length, deadline)
    try:
        host = raw.decode('ascii').lower()
        ipaddress.ip_address(host)
    except UnicodeError:
        raise OSError('socks_name') from None
    except ValueError:
        pass
    else:
        raise OSError('socks_domain_required')
    port = int.from_bytes(_read_exact(stream, 2, deadline), 'big')
    target = (host, port)
    if target not in allowed:
        raise OSError('socks_unselected')
    return target


def _pump(left, right, stop):
    queues = {left: bytearray(), right: bytearray()}
    peers = {left: right, right: left}
    readable = {left, right}
    pending = set()
    deadline = continuous_time() + IDLE_SECONDS
    for stream in peers:
        stream.setblocking(False)
    with selectors.DefaultSelector() as poll:
        while not stop.is_set() and (readable or any(queues.values())):
            remaining = deadline - continuous_time()
            if remaining <= 0:
                return
            for stream in peers:
                with contextlib.suppress(KeyError):
                    poll.unregister(stream)
                events = (selectors.EVENT_READ if stream in readable and len(queues[peers[stream]]) < BUFFER else 0)
                if queues[stream]:
                    events |= selectors.EVENT_WRITE
                if events:
                    poll.register(stream, events)
            for key, events in poll.select(min(.2, remaining)):
                # Sleep may occur inside select: enforce expiry before forwarding.
                if continuous_time() >= deadline:
                    return
                stream = key.fileobj
                if events & selectors.EVENT_READ:
                    try:
                        data = stream.recv(BUFFER - len(queues[peers[stream]]))
                    except BlockingIOError:
                        data = None
                    if data:
                        queues[peers[stream]].extend(data)
                        deadline = continuous_time() + IDLE_SECONDS
                    elif data == b'':
                        readable.discard(stream)
                        pending.add(peers[stream])
                if events & selectors.EVENT_WRITE:
                    try:
                        sent = stream.send(queues[stream])
                    except BlockingIOError:
                        sent = 0
                    if sent:
                        del queues[stream][:sent]
                        deadline = continuous_time() + IDLE_SECONDS
                for target in tuple(pending):
                    if not queues[target]:
                        with contextlib.suppress(OSError):
                            target.shutdown(socket.SHUT_WR)
                        pending.remove(target)


class SocksBroker:
    def __init__(self, relays):
        self.allowed = {(urlsplit(v).hostname, urlsplit(v).port or 443) for v in canonical_relays(relays)}
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.streams = set()
        self.resolvers = set()
        self.threads = []
        self.limit = threading.BoundedSemaphore(MAX_CONNECTIONS)
        self.local_limits = {v: threading.BoundedSemaphore(8) for v in self.allowed}
        self.listener = None

    def start(self):
        self.listener = socket.socket()
        self.listener.bind(('127.0.0.1', 0))
        self.listener.listen(8)
        self.listener.settimeout(.2)
        self.address = self.listener.getsockname()
        thread = threading.Thread(target=self._accept, daemon=True)
        self.threads.append(thread)
        thread.start()
        return self

    def _accept(self):
        while not self.stop.is_set():
            try:
                stream, _ = self.listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            if not self.limit.acquire(False):
                stream.close()
                continue
            with self.lock:
                self.streams.add(stream)
                self.threads = [t for t in self.threads if t.is_alive()]
                thread = threading.Thread(target=self._serve, args=(stream,), daemon=True)
                self.threads.append(thread)
                thread.start()

    def _resolve(self, target, deadline):
        code = ('import socket,json,sys; rows=socket.getaddrinfo(sys.argv[1],int(sys.argv[2]),type=socket.SOCK_STREAM); '
                'print(json.dumps([(r[0],r[4][0]) for r in rows[:32]]))')
        environment = {'PATH': '/usr/bin:/bin'}
        environment.update(_execution_marker())
        child = subprocess.Popen([sys.executable, '-I', '-c', code, target[0], str(target[1])],
                                 stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                 stdin=subprocess.DEVNULL, close_fds=True, env=environment)
        with self.lock:
            self.resolvers.add(child)
            if self.stop.is_set():
                child.kill()
        try:
            while child.poll() is None:
                if self.stop.is_set() or continuous_time() >= deadline:
                    child.kill()
                    raise OSError('resolver_timeout')
                self.stop.wait(.02)
            output, _ = child.communicate()
            if child.returncode or len(output) > 4096:
                raise OSError('resolver_failed')
            rows = json.loads(output)
            if not isinstance(rows, list) or not 1 <= len(rows) <= 32:
                raise OSError('resolver_failed')
            return [(family, str(ipaddress.ip_address(address))) for family, address in rows]
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=1)
            with self.lock:
                self.resolvers.discard(child)

    def _connect(self, target, deadline):
        for family, address in self._resolve(target, deadline):
            if self.stop.is_set() or continuous_time() >= deadline:
                break
            if family not in (socket.AF_INET, socket.AF_INET6):
                continue
            upstream = socket.socket(family)
            with self.lock:
                self.streams.add(upstream)
            try:
                upstream.settimeout(max(.001, deadline - continuous_time()))
                upstream.connect((address, target[1]))
                return upstream
            except OSError:
                with self.lock:
                    self.streams.discard(upstream)
                upstream.close()
        raise OSError('connect_failed')

    def _serve(self, stream):
        upstream = None
        local = None
        try:
            deadline = continuous_time() + CONNECT_SECONDS
            target = socks_target(stream, self.allowed, deadline)
            local = self.local_limits[target]
            if not local.acquire(False):
                local = None
                raise OSError('destination_busy')
            upstream = self._connect(target, deadline)
            stream.sendall(b'\x05\x00\x00\x01' + b'\x00' * 6)
            _pump(stream, upstream, self.stop)
        except (OSError, ValueError, TypeError):
            pass
        finally:
            for endpoint in (stream, upstream):
                if endpoint is not None:
                    with self.lock:
                        self.streams.discard(endpoint)
                    endpoint.close()
            if local:
                local.release()
            self.limit.release()

    def close(self):
        self.stop.set()
        if self.listener:
            self.listener.close()
        with self.lock:
            streams = tuple(self.streams)
            children = tuple(self.resolvers)
            threads = tuple(self.threads)
        for child in children:
            with contextlib.suppress(ProcessLookupError):
                child.kill()
        for stream in streams:
            with contextlib.suppress(OSError):
                stream.shutdown(socket.SHUT_RDWR)
            stream.close()
        deadline = continuous_time() + 2
        for thread in threads:
            thread.join(max(0, deadline - continuous_time()))


def _execution_marker():
    try:
        return execution_marker()
    except RuntimeError:
        raise SandboxError('invalid_execution_marker') from None


# The bootstrap uses only inherited anonymous fd0, not a filesystem endpoint.
# Do not broaden vnode/network permissions for it. Native injected startup and
# lifetime tests remain the acceptance gate for the inherited-pipe path.
PROFILE = '''(version 1)
(deny default)
(allow process-exec (literal (param "EXEC")))
(allow process-info* (target self))
(allow signal (target self))
(allow sysctl-read)
(allow file-read-metadata)
(allow file-read* (subpath "/System") (subpath "/usr/lib")
 (literal (param "EXEC")) (literal (param "CONFIG")) (subpath (param "HOME"))
 (literal "/dev/null") (literal "/dev/random") (literal "/dev/urandom"))
(allow file-write* (subpath (param "HOME")) (literal "/dev/null")
 (literal (param "ACK")) (literal (param "ACKTMP")))
(allow network-outbound (remote ip (param "BROKER")))
(allow network-bind (local unix-socket (subpath (param "HOME"))))
(allow network-inbound (local unix-socket (subpath (param "HOME"))))
'''


def socket_path_fits(home, name='s'):
    return len(os.fsencode(home)) + len('/.sock.') + 10 + 1 + len(os.fsencode(name)) * 2 + 1 + 1 <= 104


def control_dir(home):
    """The sole derivation of host-only supervisor state, outside writable HOME."""
    home = Path(home).absolute()
    if home.resolve() != home:
        raise SandboxError('noncanonical_daemon_home')
    return home.parent / ('.marmish-supervisor-' + hashlib.sha256(os.fsencode(home)).hexdigest())


def _ancestor(meta):
    if (meta.st_uid not in (0, os.geteuid()) or not stat.S_ISDIR(meta.st_mode)
            or (meta.st_mode & 0o022 and not (meta.st_uid == 0 and meta.st_mode & stat.S_ISVTX))):
        raise SandboxError('unsafe_control_ancestor')


def _open_control(home):
    path = control_dir(home)
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        _ancestor(os.fstat(fd))
        for component in path.parent.parts[1:]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            os.close(fd)
            fd = child
            _ancestor(os.fstat(fd))
        with contextlib.suppress(FileExistsError):
            os.mkdir(path.name, 0o700, dir_fd=fd)
        child = os.open(path.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
        try:
            meta = os.fstat(child)
            if meta.st_uid != os.geteuid() or stat.S_IMODE(meta.st_mode) != 0o700:
                raise SandboxError('unsafe_control_directory')
            return child
        except BaseException:
            os.close(child)
            raise
    finally:
        os.close(fd)


def _verify_control(path, fd):
    a, b = path.lstat(), os.fstat(fd)
    if not stat.S_ISDIR(a.st_mode) or (a.st_dev, a.st_ino) != (b.st_dev, b.st_ino):
        raise SandboxError('control_directory_changed')


def _json_private(path, value):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, 'w') as stream:
        json.dump(value, stream, sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())


def _atomic_record(fd, value):
    name = '.record-' + os.urandom(16).hex() + '.tmp'
    output = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                     0o600, dir_fd=fd)
    try:
        with os.fdopen(output, 'w') as stream:
            json.dump(value, stream, sort_keys=True)
            stream.flush()
            os.fsync(stream.fileno())
        os.rename(name, '.marmish-run.json', src_dir_fd=fd, dst_dir_fd=fd)
        os.fsync(fd)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(name, dir_fd=fd)


def _private_json(path, *, dir_fd=None):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=dir_fd)
    with os.fdopen(fd, 'rb') as stream:
        meta = os.fstat(stream.fileno())
        if (not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid()
                or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_nlink != 1):
            raise SandboxError('unsafe_private_record')
        raw = stream.read(16385)
    if len(raw) > 16384:
        raise SandboxError('invalid_private_record')
    return json.loads(raw, object_pairs_hook=strict_object)


def _gc_control(path, fd):
    _verify_control(path, fd)
    entries = os.listdir(fd)
    if len(entries) > 256:
        raise SandboxError('control_gc_limit')
    for name in entries:
        if name in ('.marmish-transport.lock', '.marmish-run.json'):
            continue
        meta = os.stat(name, dir_fd=fd, follow_symlinks=False)
        if name.startswith('.marmish-native-') and stat.S_ISDIR(meta.st_mode):
            _private(path / name, True)
            shutil.rmtree(path / name)
        elif name == '.marmish-probe-record' or re.fullmatch(r'\.record-[0-9a-f]{32}\.tmp', name):
            _private(path / name, False)
            os.unlink(name, dir_fd=fd)
        else:
            raise SandboxError('unrecognized_control_entry')
    _verify_control(path, fd)


def _lock_control(fd):
    lock = os.open('.marmish-transport.lock', os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC,
                   0o600, dir_fd=fd)
    try:
        meta = os.fstat(lock)
        if (not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid()
                or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_nlink != 1):
            raise SandboxError('unsafe_control_lock')
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SandboxError('daemon_home_in_use') from None
        return lock
    except BaseException:
        os.close(lock)
        raise


def cleanup_control(home):
    """Locked owned-test cleanup; retain directory/lock inode. Errors fail closed."""
    path = control_dir(home)
    fd = _open_control(home)
    lock = None
    try:
        lock = _lock_control(fd)
        if os.path.lexists(path / '.marmish-run.json'):
            raise SandboxError('control_run_record_remaining')
        _gc_control(path, fd)
    finally:
        if lock is not None:
            os.close(lock)
        os.close(fd)


def read_running_identity(home):
    """Read only local private control data, never a public rendezvous identity."""
    fd = _open_control(home)
    try:
        identity = _private_json('.marmish-run.json', dir_fd=fd).get('daemon')
        if not isinstance(identity, dict) or process_identity(identity.get('pid')) != identity:
            raise SandboxError('daemon_identity_changed')
        return identity
    finally:
        os.close(fd)


def _valid_identity(identity):
    if (not isinstance(identity, dict) or set(identity) != {'pid', 'boot_id', 'start'}
            or type(identity['pid']) is not int or not 0 < identity['pid'] < (1 << 31)
            or type(identity['start']) is not int or identity['start'] <= 0
            or not isinstance(identity['boot_id'], str)
            or not re.fullmatch(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', identity['boot_id'])):
        raise SandboxError('invalid_run_identity')
    return identity


def _current_identity(identity):
    try:
        value = inspect_process(identity['pid'])
    except Exception:
        raise SandboxError('orphan_identity_unverified') from None
    if value is None:
        return None
    if (value['uid'] != os.geteuid() or value['ruid'] != os.geteuid()
            or any(value[key] != identity[key] for key in ('pid', 'boot_id', 'start'))):
        return None  # A successfully inspected replacement must never be signaled.
    return {key: value[key] for key in ('pid', 'boot_id', 'start')}


class DarwinConfinedDaemon:
    def __init__(self, home, wnd, relays, wnd_sha256, *, manifest=None, bootstrap=None):
        self._bootstrap = bootstrap
        self._native_key = bootstrap is not None
        self.home = Path(home).absolute()
        self.wnd = Path(wnd).absolute()
        self.relays = canonical_relays(relays)
        self.wnd_sha256 = wnd_sha256
        self.manifest = manifest
        self.socket = self.home / 's'
        self.rpc_home = str(self.home)
        self.rpc_socket = str(self.socket)
        self.peer_identity = None
        self.process = None
        self._lock = None
        self._directory = None
        self._broker = None
        self._writer = None
        self._socket_inode = None
        self._own_record = False
        self._control = control_dir(self.home)
        self._control_fd = None
        self._record = self._control / '.marmish-run.json'

    def __enter__(self):
        return self.start()

    def __exit__(self, *_):
        self.close()

    def _command(self, executable, *arguments):
        return ['/usr/bin/sandbox-exec', '-D', 'EXEC=' + str(executable),
                '-D', 'HOME=' + str(self.home), '-D', 'SOCKET=' + str(self.socket),
                '-D', 'BROKER=localhost:' + str(self._broker.address[1]),
                '-D', 'CONFIG=' + str(self._directory / 'restricted.json'),
                '-D', 'ACK=' + str(self._directory / 'restricted.ack'),
                '-D', 'ACKTMP=' + str(self._directory / 'restricted.ack.tmp'),
                '-f', str(self._directory / 'profile.sb'), str(executable), *map(str, arguments)]

    def _daemon_command(self):
        relays = ','.join(self.relays)
        return self._command(self._binary, '--home', self.home, '--socket', self.socket,
                             '--secret-store', 'marmish-stdin' if self._native_key else 'file', '--discovery-relays', relays,
                             '--default-account-relays', relays)

    def _prepare(self):
        if sys.platform != 'darwin' or not Path('/usr/bin/sandbox-exec').is_file():
            raise SandboxError('missing_native_confinement')
        if os.geteuid() == 0:
            raise SandboxError('root_not_supported')
        self.home.parent.mkdir(parents=True, exist_ok=True)
        if not self.home.exists():
            self.home.mkdir(mode=0o700)
        _private(self.home, True)
        if self.home.resolve() != self.home or not socket_path_fits(self.home):
            raise SandboxError('noncanonical_daemon_home')
        self._control_fd = _open_control(self.home)
        self._lock = _lock_control(self._control_fd)
        self._recover()
        _gc_control(self._control, self._control_fd)
        value = verify_manifest(self.wnd, self.wnd_sha256, self.manifest, 'darwin')
        self._directory = Path(tempfile.mkdtemp(prefix='.marmish-native-', dir=self._control))
        _verify_control(self._control, self._control_fd)
        for name, expected in ((self.wnd.name, self.wnd_sha256), *value['helpers'].items()):
            source = self.wnd if name == self.wnd.name else self.wnd.parent / name
            destination = self._directory / name
            fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            with os.fdopen(fd, 'rb') as stream:
                meta = os.fstat(stream.fileno())
                if not stat.S_ISREG(meta.st_mode) or meta.st_uid not in (0, os.geteuid()) or meta.st_mode & 0o022:
                    raise SandboxError('unsafe_native_artifact')
                with destination.open('xb') as target:
                    shutil.copyfileobj(stream, target)
            destination.chmod(0o700)
            if digest(destination) != expected:
                raise SandboxError('native_copy_hash_mismatch')
            result = subprocess.run(['/usr/bin/codesign', '--verify', '--strict', str(destination)],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
            if result.returncode:
                raise SandboxError('invalid_native_signature')
        _verify_control(self._control, self._control_fd)
        self._binary = self._directory / self.wnd.name
        (self._directory / 'profile.sb').write_text(PROFILE)
        (self._directory / 'profile.sb').chmod(0o600)
        self._broker = SocksBroker(self.relays).start()
        self._preflight()
        self._config = self._directory / 'restricted.json'
        self._nonce = os.urandom(32).hex()
        _json_private(self._config, {'version': 1, 'nonce': self._nonce, 'relays': self.relays,
                                   'proxy': '127.0.0.1:' + str(self._broker.address[1])})

    def _recover(self):
        if os.path.lexists(self._record):
            record = _private_json('.marmish-run.json', dir_fd=self._control_fd)
            if not isinstance(record, dict) or not re.fullmatch(r'\.marmish-native-[a-zA-Z0-9_-]{1,64}', record.get('runtime', '')):
                raise SandboxError('invalid_run_manifest')
            identities = [_valid_identity(record.get(name)) for name in ('daemon', 'shim')]
            if identities[0]['boot_id'] != identities[1]['boot_id']:
                raise SandboxError('mixed_run_boot')
            old_boot = identities[0]['boot_id'] != process_identity(os.getpid())['boot_id']
            for name in ('daemon', 'shim'):
                identity = record.get(name)
                if not isinstance(identity, dict):
                    raise SandboxError('invalid_run_manifest')
                current = None if old_boot else _current_identity(identity)
                if current is None:
                    continue
                try:
                    group = os.getpgid(identity['pid'])
                except ProcessLookupError:
                    if _current_identity(identity) is None:
                        continue
                    raise SandboxError('orphan_identity_unverified') from None
                except OSError:
                    raise SandboxError('orphan_identity_unverified') from None
                if current != identity or group != identity['pid']:
                    raise SandboxError('orphan_identity_mismatch')
                # Executable path/digest verification is native, avoiding ps output.
                import ctypes
                library = ctypes.CDLL('/usr/lib/libproc.dylib', use_errno=True)
                library.proc_pidpath.argtypes = [ctypes.c_int, ctypes.c_void_p, ctypes.c_uint32]
                library.proc_pidpath.restype = ctypes.c_int
                path = ctypes.create_string_buffer(4096)
                if library.proc_pidpath(identity['pid'], path, len(path)) <= 0:
                    if _current_identity(identity) is None:
                        continue
                    raise SandboxError('orphan_executable_unverified')
                try:
                    executable_digest = digest(Path(os.fsdecode(path.value)))
                except (OSError, ValueError):
                    raise SandboxError('orphan_executable_unverified') from None
                if executable_digest != record.get(name + '_sha256'):
                    raise SandboxError('orphan_executable_mismatch')
                if _current_identity(identity) is None:
                    continue
                try:
                    os.killpg(identity['pid'], signal.SIGTERM)
                except ProcessLookupError:
                    if _current_identity(identity) is None:
                        continue
                    raise SandboxError('orphan_identity_unverified') from None
                except OSError:
                    raise SandboxError('orphan_identity_unverified') from None
                deadline = continuous_time() + 2
                while continuous_time() < deadline:
                    current = _current_identity(identity)
                    if current is None:
                        break
                    if current != identity:
                        raise SandboxError('orphan_identity_mismatch')
                    time.sleep(.02)
                else:
                    if _current_identity(identity) == identity:
                        try:
                            os.killpg(identity['pid'], signal.SIGKILL)
                        except ProcessLookupError:
                            if _current_identity(identity) is not None:
                                raise SandboxError('orphan_identity_unverified') from None
                        except OSError:
                            raise SandboxError('orphan_identity_unverified') from None
                    deadline = continuous_time() + 2
                    while continuous_time() < deadline and _current_identity(identity) == identity:
                        time.sleep(.02)
                    if _current_identity(identity) is not None:
                        raise SandboxError('orphan_cleanup_timeout')
            os.unlink('.marmish-run.json', dir_fd=self._control_fd)
            os.fsync(self._control_fd)
        if self.socket.is_symlink():
            raise SandboxError('unsafe_daemon_socket')
        if self.socket.exists():
            meta = self.socket.lstat()
            if not stat.S_ISSOCK(meta.st_mode) or meta.st_uid != os.geteuid():
                raise SandboxError('unsafe_daemon_socket')
            with socket.socket(socket.AF_UNIX) as probe:
                probe.settimeout(.2)
                try:
                    probe.connect(str(self.socket))
                except ConnectionRefusedError:
                    self.socket.unlink()
                else:
                    raise SandboxError('daemon_home_in_use')

    def _preflight(self):
        probe = self._directory / 'marmish-probe'
        def invoke(arguments, confined):
            command = self._command(probe, *arguments) if confined else [str(probe), *map(str, arguments)]
            return subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                  stderr=subprocess.DEVNULL, timeout=5, close_fds=True,
                                  env={'PATH': '/usr/bin:/bin'}).returncode
        def check(label, allowed, *arguments, reset=None):
            if invoke(arguments, False) != 0:
                raise SandboxError('native_control_' + label)
            if reset:
                reset()
            if invoke(arguments, True) != (0 if allowed else 2):
                raise SandboxError('native_probe_' + label)
        check('broker', True, 'tcp', '127.0.0.1', self._broker.address[1])
        private = self.home / 'private-probe'
        private.write_bytes(b'owned confinement probe')
        check('home_read', True, 'read', private)
        check('home_write', True, 'write', private)
        check('rpc_staging', True, 'unix-staged', self.home, 's')
        with tempfile.TemporaryDirectory(prefix='md-', dir='/private/tmp') as outside:
            sentinel = Path(outside).resolve() / 'sentinel'
            sentinel.write_bytes(b'owned confinement probe')
            check('outside_read', False, 'read', sentinel)
            check('outside_write', False, 'write', sentinel)
            with socket.socket() as denied:
                denied.bind(('127.0.0.1', 0))
                denied.listen(4)
                check('other_tcp', False, 'tcp', '127.0.0.1', denied.getsockname()[1])
                check('udp', False, 'udp', '127.0.0.1', denied.getsockname()[1])
            with socket.socket(socket.AF_UNIX) as denied:
                path = Path(outside).resolve() / 'socket'
                denied.bind(str(path))
                denied.listen(4)
                check('other_unix', False, 'unix', path)
            bind = Path(outside).resolve() / 'bind'
            check('other_unix_bind', False, 'unix-bind', bind, reset=bind.unlink)
            with socket.socket(socket.AF_INET6) as denied:
                denied.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
                denied.bind(('::1', self._broker.address[1]))
                denied.listen(4)
                check('ipv6', False, 'tcp', '::1', self._broker.address[1])
        interfaces = subprocess.run(['/sbin/ifconfig', '-a'], check=True, stdout=subprocess.PIPE,
                                    stderr=subprocess.DEVNULL, timeout=5)
        if len(interfaces.stdout) > 65536:
            raise SandboxError('native_interface_probe_failed')
        addresses = re.findall(rb'\binet (\d+\.\d+\.\d+\.\d+)\b', interfaces.stdout)
        addresses = [v.decode('ascii') for v in addresses if not ipaddress.ip_address(v.decode('ascii')).is_loopback]
        if not addresses:
            raise SandboxError('native_interface_probe_unavailable')
        with socket.socket() as denied:
            denied.bind((addresses[0], 0))
            denied.listen(4)
            check('interface', False, 'tcp', addresses[0], denied.getsockname()[1])
        target = next(iter(self._broker.allowed))
        check('dns', False, 'dns', target[0])
        # Destructive controls use fresh owned sentinels, restored before sandboxing.
        record_probe = self._control / '.marmish-probe-record'
        config = self._directory / 'restricted.json'
        binary = self._directory / 'immutable-program'
        try:
            for label, destination in (('record', record_probe), ('config', config), ('binary', binary)):
                content = probe.read_bytes() if label == 'binary' else b'owned immutable sentinel'
                def restore(p=destination, data=content):
                    p.write_bytes(data)
                    p.chmod(0o700 if label == 'binary' else 0o600)
                restore()
                for mode in ('write', 'truncate', 'unlink'):
                    check('control_' + label + '_' + mode, False, mode, destination,
                          reset=restore)
                replacement = self.home / 'replacement'
                replacement.write_bytes(b'owned replacement')
                def reset_rename(p=destination):
                    restore()
                    replacement.write_bytes(b'owned replacement')
                check('control_' + label + '_rename', False, 'rename', replacement, destination,
                      reset=reset_rename)
            sibling = self._directory / 'restricted.ack.other'
            check('control_create', False, 'create', sibling, reset=sibling.unlink)
            ack, temporary = self._directory / 'restricted.ack', self._directory / 'restricted.ack.tmp'
            check('ack_create', True, 'create', temporary, reset=temporary.unlink)
            check('ack_publish', True, 'rename', temporary, ack,
                  reset=lambda: (ack.unlink(), temporary.write_bytes(b'ephemeral')))
        finally:
            for path in (record_probe, config, binary, self.home / 'replacement',
                         self._directory / 'restricted.ack', self._directory / 'restricted.ack.tmp',
                         self._directory / 'restricted.ack.other', private):
                with contextlib.suppress(FileNotFoundError):
                    path.unlink()

    def start(self):
        if self.process:
            if self.process.poll() is None:
                return self
            raise SandboxError('native_daemon_exited')
        try:
            self._prepare()
            reader, self._writer = os.pipe2(os.O_CLOEXEC) if hasattr(os, 'pipe2') else os.pipe()
            os.set_inheritable(self._writer, False)
            environment = {'PATH': '/usr/bin:/bin', 'LANG': 'C.UTF-8', 'HOME': str(self.home),
                           'MARMISH_RESTRICTED_CONFIG': str(self._config), **_execution_marker()}
            command = self._daemon_command()
            from marmish_bootstrap import open_pipe
            bootstrap_reader = subprocess.DEVNULL
            try:
                bootstrap_reader = open_pipe(self._bootstrap)
                self.process = subprocess.Popen([str(self._directory / 'marmish-lifetime'), str(reader), str(os.getpid()), *command],
                                                pass_fds=(reader,), start_new_session=True,
                                                stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                                stdin=bootstrap_reader, close_fds=True, env=environment)
            finally:
                os.close(reader)
                if bootstrap_reader >= 0:
                    os.close(bootstrap_reader)
            os.set_blocking(self.process.stdout.fileno(), False)
            deadline = continuous_time() + 15
            response = bytearray()
            while continuous_time() < deadline:
                if self.process.poll() is not None:
                    raise SandboxError('native_daemon_start_failed')
                if self.peer_identity is None:
                    with contextlib.suppress(BlockingIOError):
                        response.extend(os.read(self.process.stdout.fileno(), 32))
                    if len(response) > 32:
                        raise SandboxError('invalid_native_ready')
                    if b'\n' in response:
                        pid = int(response.strip())
                        self.peer_identity = process_identity(pid)
                        _atomic_record(self._control_fd, {'daemon': self.peer_identity,
                            'shim': process_identity(self.process.pid), 'daemon_sha256': self.wnd_sha256,
                            'shim_sha256': digest(self._directory / 'marmish-lifetime'),
                            'nonce': self._nonce, 'config_sha256': digest(self._config),
                            'runtime': self._directory.name})
                        self._own_record = True
                        # The shim blocks child exec until this durable record exists.
                        os.write(self._writer, b'G')
                ack = self._config.with_suffix('.ack')
                if self.peer_identity and ack.exists() and self.socket.exists():
                    if _private_json(ack) != {'version': 1, 'nonce': self._nonce, 'config_sha256': digest(self._config)}:
                        raise SandboxError('restricted_ack_mismatch')
                    if process_identity(self.peer_identity['pid']) != self.peer_identity:
                        raise SandboxError('daemon_identity_changed')
                    meta = self.socket.lstat()
                    if not stat.S_ISSOCK(meta.st_mode) or meta.st_uid != os.geteuid():
                        raise SandboxError('unsafe_daemon_socket')
                    self._socket_inode = meta.st_ino
                    from marmish_bootstrap import wipe
                    wipe(self._bootstrap)
                    self._bootstrap = None
                    return self
                time.sleep(.02)
            raise SandboxError('native_daemon_start_timeout')
        except BaseException:
            self.close()
            raise

    def close(self):
        from marmish_bootstrap import wipe
        wipe(self._bootstrap)
        self._bootstrap = None
        failure = None
        if self._writer is not None:
            os.close(self._writer)
            self._writer = None
        if self.process is not None:
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                # The shim should reap the daemon first. Leave manifest/state
                # intact if its identity-verified recovery is still required.
                failure = SandboxError('native_cleanup_timeout')
            finally:
                if self.process.stdout:
                    self.process.stdout.close()
                self.process = None
        if self._broker:
            self._broker.close()
            self._broker = None
        if self._own_record and failure is None:
            try:
                self._recover()
                self._own_record = False
            except BaseException:
                failure = SandboxError('native_cleanup_identity_failed')
        if self._socket_inode is not None:
            with contextlib.suppress(FileNotFoundError):
                if self.socket.lstat().st_ino == self._socket_inode:
                    self.socket.unlink()
            self._socket_inode = None
        with contextlib.suppress(FileNotFoundError):
            if self._own_record and failure is None:
                self._record.unlink()
                self._own_record = False
        if self._directory and failure is None:
            shutil.rmtree(self._directory)
            self._directory = None
        if self._lock is not None:
            os.close(self._lock)
            self._lock = None
        if self._control_fd is not None:
            os.close(self._control_fd)
            self._control_fd = None
        if failure:
            raise failure
