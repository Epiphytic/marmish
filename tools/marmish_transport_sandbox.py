"""Physical-origin confinement for the pinned prototype MDK daemon.

ConfinedDaemon(home, wnd, relays, wnd_sha256) requires a private persistent
home and explicit root-path wss DNS origins. start() returns self after a
capability/port-443 self-test and socket readiness, within 15 seconds. close()
is idempotent and bounded; it preserves identity and MLS state. socket is
home/'s'. Context management starts/closes. SandboxError contains only fixed
operation codes. This confines physical origins, not paths inside TLS.
"""
from __future__ import annotations

import contextlib
import fcntl
import hashlib
import ipaddress
import json
import os
from pathlib import Path
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

STARTUP_SECONDS = 15
MAX_ORIGINS = 16
MAX_CONNECTIONS = 32
CONNECT_SECONDS = 5
IDLE_SECONDS = 660
BUFFER = 16384


def _clock():
    # The namespace bootstrap mounts only this file; Linux needs no helper import.
    if sys.platform.startswith('linux'):
        return time.clock_gettime(time.CLOCK_BOOTTIME)
    from marmish_platform import continuous_time
    return continuous_time()


class SandboxError(RuntimeError):
    """A fixed operation category; never includes remote or daemon output."""


def canonical_relays(values):
    # Reuse the existing strict prototype URL parser only in the host process.
    from mdk_smoke import relay_url, SmokeError
    if not isinstance(values, list) or not 1 <= len(values) <= MAX_ORIGINS:
        raise SandboxError("invalid_relays")
    try:
        result = list(dict.fromkeys(relay_url(value) for value in values))
        for value in result:
            try:
                ipaddress.ip_address(urlsplit(value).hostname)
            except ValueError:
                continue
            raise SandboxError("dns_origin_required")
        return result
    except SmokeError:
        raise SandboxError("invalid_relays") from None


def _private(path, directory):
    meta = path.lstat()
    if (meta.st_uid != os.geteuid() or stat.S_IMODE(meta.st_mode) != (0o700 if directory else 0o600)
            or (not stat.S_ISDIR(meta.st_mode) if directory else not stat.S_ISREG(meta.st_mode))
            or (not directory and meta.st_nlink != 1)):
        raise SandboxError("unsafe_private_path")


class _DuplexPump:
    """Per-connection forwarding state, bounded to BUFFER bytes each way."""

    def __init__(self, left, right, stop):
        self.streams = (left, right)
        self.stop = stop
        self.queues = {left: bytearray(), right: bytearray()}
        self.readable = {left, right}
        self.peers = {left: right, right: left}
        self.pending_shutdown = set()
        self.deadline = _clock() + IDLE_SECONDS

    def run(self):
        for stream in self.streams:
            stream.setblocking(False)
        with selectors.DefaultSelector() as poll:
            while not self.stop.is_set() and (self.readable or any(self.queues.values())):
                self._register(poll)
                remaining = self.deadline - _clock()
                if remaining <= 0:
                    return
                for key, events in poll.select(min(remaining, .2)):
                    self._transfer(key.fileobj, events)

    def _register(self, poll):
        for stream in self.streams:
            events = 0
            if stream in self.readable and len(self.queues[self.peers[stream]]) < BUFFER:
                events |= selectors.EVENT_READ
            if self.queues[stream]:
                events |= selectors.EVENT_WRITE
            with contextlib.suppress(KeyError):
                poll.unregister(stream)
            if events:
                poll.register(stream, events)

    def _receive(self, stream):
        peer = self.peers[stream]
        try:
            data = stream.recv(BUFFER - len(self.queues[peer]))
        except BlockingIOError:
            return False
        if data:
            self.queues[peer].extend(data)
            return True
        self.readable.discard(stream)
        self.pending_shutdown.add(peer)
        return False

    def _send(self, stream):
        try:
            sent = stream.send(self.queues[stream])
        except BlockingIOError:
            return False
        del self.queues[stream][:sent]
        return bool(sent)

    def _transfer(self, stream, events):
        received = bool(events & selectors.EVENT_READ) and self._receive(stream)
        sent = bool(events & selectors.EVENT_WRITE) and self._send(stream)
        if received or sent:
            self.deadline = _clock() + IDLE_SECONDS
        self._flush_half_closes()

    def _flush_half_closes(self):
        for stream in tuple(self.pending_shutdown):
            if not self.queues[stream]:
                with contextlib.suppress(OSError):
                    stream.shutdown(socket.SHUT_WR)
                self.pending_shutdown.remove(stream)


def _pump(left, right, stop):
    """Bounded full-duplex copying, preserving half-close and idle deadline."""
    _DuplexPump(left, right, stop).run()


class _Broker:
    def __init__(self, routes):
        self.routes = routes
        self.stop = threading.Event()
        self.limit = threading.BoundedSemaphore(MAX_CONNECTIONS)
        self.lock = threading.Lock()
        self.sockets = set()
        self.resolvers = set()
        self.listeners = []
        self.threads = []

    def start(self):
        for path, target in self.routes:
            listener = socket.socket(socket.AF_UNIX)
            listener.bind(str(path))
            os.chmod(path, 0o600)
            listener.listen(8)
            listener.settimeout(.2)
            self.listeners.append(listener)
            thread = threading.Thread(target=self._accept, args=(listener, target), daemon=True)
            thread.start()
            self.threads.append(thread)

    def _accept(self, listener, target):
        local_limit = threading.BoundedSemaphore(8)
        while not self.stop.is_set():
            try:
                stream, _ = listener.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            if not self.limit.acquire(blocking=False):
                stream.close()
                continue
            if not local_limit.acquire(blocking=False):
                self.limit.release()
                stream.close()
                continue
            with self.lock:
                self.sockets.add(stream)
            thread = threading.Thread(target=self._serve, args=(stream, target, local_limit), daemon=True)
            thread.start()
            # Finished thread objects are discarded to keep bookkeeping bounded.
            with self.lock:
                self.threads = [t for t in self.threads if t.is_alive()]
                self.threads.append(thread)

    def _serve(self, stream, target, local_limit):
        upstream = None
        try:
            # target is constructor-owned; no address is accepted from the peer.
            upstream = self._connect(target)
            if not self.stop.is_set():
                _pump(stream, upstream, self.stop)
        except (OSError, ValueError):
            pass
        finally:
            with self.lock:
                self.sockets.discard(stream)
                self.sockets.discard(upstream)
            stream.close()
            if upstream is not None:
                upstream.close()
            local_limit.release()
            self.limit.release()

    def _resolve(self, target, deadline):
        # libc DNS does not obey socket timeouts. Isolate it in a killable,
        # bounded-output child; only public configured origins enter argv.
        code = ('import socket,json,sys; '
                'rows=socket.getaddrinfo(sys.argv[1],int(sys.argv[2]),type=socket.SOCK_STREAM); '
                'print(json.dumps([(r[0],r[4][0]) for r in rows[:32]]))')
        process = subprocess.Popen(['/usr/bin/python3', '-I', '-c', code, target[0], str(target[1])],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, close_fds=True,
                                   env={'PATH': '/usr/bin:/bin'})
        with self.lock:
            self.resolvers.add(process)
            if self.stop.is_set():
                with contextlib.suppress(ProcessLookupError):
                    process.kill()
        try:
            try:
                output, _ = process.communicate(timeout=max(.001, deadline - _clock()))
            except subprocess.TimeoutExpired:
                with contextlib.suppress(ProcessLookupError):
                    process.kill()
                process.communicate()
                raise OSError('resolver_timeout') from None
            if process.returncode != 0 or len(output) > 4096:
                raise OSError('resolver_failed')
            rows = json.loads(output)
            if not isinstance(rows, list) or not 1 <= len(rows) <= 32:
                raise OSError('resolver_failed')
            return [(family, str(ipaddress.ip_address(address))) for family, address in rows]
        finally:
            with self.lock:
                self.resolvers.discard(process)

    def _connect(self, target):
        deadline = _clock() + CONNECT_SECONDS
        for family, address in self._resolve(target, deadline):
            if self.stop.is_set() or _clock() >= deadline:
                break
            if family not in (socket.AF_INET, socket.AF_INET6):
                continue
            stream = socket.socket(family)
            with self.lock:
                if self.stop.is_set():
                    stream.close()
                    break
                self.sockets.add(stream)
            try:
                stream.settimeout(max(.001, deadline - _clock()))
                stream.connect((address, target[1]))
                return stream
            except OSError:
                with self.lock:
                    self.sockets.discard(stream)
                stream.close()
        raise OSError('connect_failed')

    def close(self):
        self.stop.set()
        with self.lock:
            streams = list(self.sockets) + self.listeners
            resolvers = tuple(self.resolvers)
        for process in resolvers:
            with contextlib.suppress(ProcessLookupError):
                process.kill()
        for stream in streams:
            with contextlib.suppress(OSError):
                stream.shutdown(socket.SHUT_RDWR)
            stream.close()
        deadline = _clock() + 1
        for thread in tuple(self.threads):
            thread.join(max(0, deadline - _clock()))


class ConfinedDaemon:
    def __init__(self, home: Path, wnd: Path, relays: list[str], wnd_sha256: str):
        self.home = Path(home).absolute()
        self.wnd = Path(wnd).absolute()
        self.relays = canonical_relays(relays)
        self.wnd_sha256 = wnd_sha256
        self.socket = self.home / 's'
        self.process = None
        self._lock = None
        self._temporary = None
        self._execution_marker = {}
        self._broker = None
        self._socket_inode = None

    def __enter__(self):
        return self.start()

    def __exit__(self, *_):
        self.close()

    def start(self):
        if self.process is not None:
            if self.process.poll() is None:
                return self
            self.close()
            raise SandboxError("daemon_exited")
        try:
            from marmish_platform import execution_marker
            self._execution_marker = execution_marker()
            self._prepare()
            deadline = _clock() + STARTUP_SECONDS
            directory = Path(self._temporary.name)
            config_path = directory / 'config.json'
            self.process = subprocess.Popen(
                ['unshare', '--user', '--map-root-user', '--net', '/usr/bin/python3',
                 str(Path(__file__).resolve()), '--outer', str(config_path)],
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                env={'PATH': '/usr/bin:/usr/sbin:/bin', 'LANG': 'C.UTF-8',
                     **self._execution_marker},
                start_new_session=True, close_fds=True,
            )
            os.set_blocking(self.process.stdout.fileno(), False)
            ready = bytearray()
            while _clock() < deadline:
                if self.process.poll() is not None:
                    raise SandboxError("namespace_start_failed")
                try:
                    ready.extend(os.read(self.process.stdout.fileno(), 64))
                except BlockingIOError:
                    pass
                if len(ready) > 64 or (b'\n' in ready and ready != b'READY\n'):
                    raise SandboxError("invalid_namespace_ready")
                if ready == b'READY\n' and self.socket.exists():
                    meta = self.socket.lstat()
                    if not stat.S_ISSOCK(meta.st_mode) or meta.st_uid != os.geteuid():
                        raise SandboxError("unsafe_daemon_socket")
                    self._socket_inode = meta.st_ino
                    return self
                time.sleep(.02)
            raise SandboxError("namespace_start_timeout")
        except (OSError, ValueError):
            self.close()
            raise SandboxError("namespace_start_failed") from None
        except BaseException:
            self.close()
            raise

    def _prepare(self):
        if any(shutil.which(tool) is None for tool in ('bwrap', 'unshare', 'ip', 'sysctl')):
            raise SandboxError("missing_confinement_tool")
        if len(os.fsencode(self.socket)) > 100:
            raise SandboxError("daemon_socket_path_too_long")
        if not self.home.exists():
            self.home.mkdir(mode=0o700)
        _private(self.home, True)
        if not isinstance(self.wnd_sha256, str) or len(self.wnd_sha256) != 64:
            raise SandboxError("invalid_daemon_hash")
        with self.wnd.open('rb') as source:
            actual = hashlib.file_digest(source, 'sha256').hexdigest()
        if actual != self.wnd_sha256:
            raise SandboxError("daemon_hash_mismatch")
        lock_path = self.home / '.marmish-transport.lock'
        self._lock = os.open(lock_path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
        _private(lock_path, False)
        try:
            fcntl.flock(self._lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise SandboxError("daemon_home_in_use") from None
        if self.socket.is_symlink():
            raise SandboxError("unsafe_daemon_socket")
        if self.socket.exists():
            meta = self.socket.lstat()
            if not stat.S_ISSOCK(meta.st_mode) or meta.st_uid != os.geteuid():
                raise SandboxError("unsafe_daemon_socket")
            with socket.socket(socket.AF_UNIX) as probe:
                probe.settimeout(.2)
                try:
                    probe.connect(str(self.socket))
                except ConnectionRefusedError:
                    self.socket.unlink()
                else:
                    raise SandboxError("daemon_home_in_use")
        self._temporary = tempfile.TemporaryDirectory(prefix='marmish-confinement-')
        directory = Path(self._temporary.name)
        bridges = directory / 'bridges'
        bridges.mkdir(mode=0o700)
        origins = []
        routes = []
        hosts = ['127.0.0.1 localhost']
        # A hostname always gets one address, including when multiple ports occur.
        addresses = {}
        for index, relay in enumerate(self.relays):
            parsed = urlsplit(relay)
            host, port = parsed.hostname, parsed.port or 443
            address = addresses.setdefault(host, f'127.0.0.{len(addresses) + 2}')
            hosts.append(f'{address} {host}')
            origins.append([address, port, f'{index}.sock'])
            routes.append((bridges / f'{index}.sock', (host, port)))
        (directory / 'hosts').write_text('\n'.join(dict.fromkeys(hosts)) + '\n')
        (directory / 'resolv').write_text('')
        config = {'home': str(self.home), 'wnd': str(self.wnd), 'origins': origins,
                  'relays': self.relays, 'directory': str(directory),
                  'execution_marker': self._execution_marker}
        (directory / 'config.json').write_text(json.dumps(config))
        self._broker = _Broker(routes)
        self._broker.start()

    def close(self):
        if self.process is not None:
            process, self.process = self.process, None
            if process.poll() is None:
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=2)
            if process.stdout is not None:
                process.stdout.close()
        if self._broker is not None:
            self._broker.close()
            self._broker = None
        if self._socket_inode is not None:
            with contextlib.suppress(FileNotFoundError):
                if self.socket.lstat().st_ino == self._socket_inode:
                    self.socket.unlink()
            self._socket_inode = None
        if self._temporary is not None:
            self._temporary.cleanup()
            self._temporary = None
        if self._lock is not None:
            os.close(self._lock)
            self._lock = None


def _outer(config_path):
    config = json.loads(Path(config_path).read_text())
    subprocess.run(['/usr/sbin/ip', 'link', 'set', 'lo', 'up'], check=True)
    subprocess.run(['/usr/sbin/sysctl', '-q', '-w', 'net.ipv4.ip_unprivileged_port_start=0'], check=True)
    directory = Path(config['directory'])
    args = ['bwrap', '--unshare-user', '--unshare-pid', '--uid', '0', '--gid', '0',
            '--cap-drop', 'ALL', '--die-with-parent', '--new-session', '--clearenv',
            '--setenv', 'PATH', '/usr/bin:/usr/sbin:/bin', '--setenv', 'HOME', '/state',
            '--setenv', 'LANG', 'C.UTF-8', '--ro-bind', '/usr', '/usr',
            '--symlink', 'usr/bin', '/bin', '--symlink', 'usr/lib', '/lib',
            '--symlink', 'usr/lib64', '/lib64', '--proc', '/proc', '--dev', '/dev',
            '--tmpfs', '/tmp', '--dir', '/etc', '--dir', '/etc/ssl',
            '--ro-bind', '/etc/ssl/certs', '/etc/ssl/certs',
            '--ro-bind', '/etc/nsswitch.conf', '/etc/nsswitch.conf',
            '--ro-bind', str(directory / 'hosts'), '/etc/hosts',
            '--ro-bind', str(directory / 'resolv'), '/etc/resolv.conf',
            '--ro-bind', str(directory / 'bridges'), '/bridges',
            '--bind', config['home'], '/state', '--ro-bind', config['wnd'], '/wnd',
            '--ro-bind', str(Path(__file__).resolve()), '/sandbox.py',
            '--ro-bind', config_path, '/config.json', '--',
            '/usr/bin/python3', '/sandbox.py', '--inner', '/config.json']
    marker = config.get('execution_marker', {})
    if marker:
        args[args.index('--'):args.index('--')] = [
            '--setenv', 'EPIPHYTIC_EXECUTION_ID', marker['EPIPHYTIC_EXECUTION_ID']]
    os.execvp(args[0], args)


def _self_test():
    caps = [line.split(':', 1)[1].strip() for line in Path('/proc/self/status').read_text().splitlines()
            if line.startswith(('CapEff:', 'CapPrm:', 'CapInh:', 'CapAmb:', 'CapBnd:'))]
    if len(caps) != 5 or any(int(value, 16) for value in caps):
        raise SandboxError('namespace_capabilities_remain')
    with socket.socket() as probe:
        probe.bind(('127.0.0.250', 443))
    changed = subprocess.run(['/usr/sbin/ip', 'link', 'set', 'lo', 'down'],
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if changed.returncode == 0:
        raise SandboxError('namespace_network_mutable')
    for family in ([], ['-6']):
        routes = json.loads(subprocess.check_output(['/usr/sbin/ip', '-j', *family, 'route', 'show', 'default']))
        if routes:
            raise SandboxError('namespace_has_external_route')
    if Path('/etc/resolv.conf').read_bytes():
        raise SandboxError('namespace_has_dns')


def _inner(config_path):
    config = json.loads(Path(config_path).read_text())
    _self_test()
    stop = threading.Event()
    slots = threading.BoundedSemaphore(MAX_CONNECTIONS)
    def accept(listener, bridge):
        while not stop.is_set():
            stream, _ = listener.accept()
            if not slots.acquire(blocking=False):
                stream.close()
                continue
            def serve(stream=stream, bridge=bridge):
                try:
                    with stream, socket.socket(socket.AF_UNIX) as upstream:
                        upstream.settimeout(CONNECT_SECONDS)
                        upstream.connect('/bridges/' + bridge)
                        _pump(stream, upstream, stop)
                except OSError:
                    pass
                finally:
                    slots.release()
            threading.Thread(target=serve, daemon=True).start()
    listeners = []
    for address, port, bridge in config['origins']:
        listener = socket.socket()
        listener.bind((address, port))
        listener.listen(8)
        listeners.append(listener)
        threading.Thread(target=accept, args=(listener, bridge), daemon=True).start()
    relays = ','.join(config['relays'])
    child = subprocess.Popen(['/wnd', '--home', '/state', '--socket', '/state/s',
                              '--logs-dir', '/state/logs', '--secret-store', 'file',
                              '--discovery-relays', relays, '--default-account-relays', relays],
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, close_fds=True)
    print('READY', flush=True)
    raise SystemExit(child.wait())


if __name__ == '__main__':
    try:
        if len(sys.argv) != 3:
            raise SandboxError('invalid_bootstrap')
        if sys.argv[1] == '--outer':
            _outer(sys.argv[2])
        elif sys.argv[1] == '--inner':
            _inner(sys.argv[2])
        else:
            raise SandboxError('invalid_bootstrap')
    except (OSError, ValueError, subprocess.SubprocessError, SandboxError):
        raise SystemExit(1)
