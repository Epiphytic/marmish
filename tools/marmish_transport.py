"""Select the native confinement boundary; unsupported hosts fail closed."""
import sys

from mdk_smoke import require
from marmish_transport_sandbox import ConfinedDaemon as LinuxConfinedDaemon, SandboxError


def _create_daemon(home, wnd, relays, wnd_sha256, *, manifest=None, bootstrap=None):
    if sys.platform == 'darwin':
        from marmish_mdk_build import verify_manifest
        verify_manifest(wnd, wnd_sha256, manifest, expected_platform='darwin')
        from marmish_transport_macos import DarwinConfinedDaemon
        return DarwinConfinedDaemon(home, wnd, relays, wnd_sha256, manifest=manifest, bootstrap=bootstrap)
    require(sys.platform.startswith('linux'), 'unsupported_platform')
    if bootstrap is not None and manifest is None:
        manifest = str(wnd) + '.manifest.json'
    if manifest is not None:
        from marmish_mdk_build import verify_manifest
        verify_manifest(wnd, wnd_sha256, manifest, expected_platform='linux')
    daemon = LinuxConfinedDaemon(home, wnd, relays, wnd_sha256, bootstrap=bootstrap, manifest=manifest)
    daemon.rpc_home, daemon.rpc_socket, daemon.peer_identity = '/state', '/state/s', None
    return daemon


def ConfinedDaemon(home, wnd, relays, wnd_sha256, *, manifest=None, bootstrap=None):
    try:
        return _create_daemon(home, wnd, relays, wnd_sha256, manifest=manifest, bootstrap=bootstrap)
    except BaseException:
        from marmish_bootstrap import wipe
        wipe(bootstrap)
        raise


from contextlib import contextmanager


@contextmanager
def acquire_profile_home_lock(home):
    """Exclude normal transport startup while an explicit migration owns state."""
    import os
    import fcntl
    from pathlib import Path
    lock = directory = None
    try:
        if sys.platform == 'darwin':
            from marmish_transport_macos import _open_control, _lock_control
            directory = _open_control(Path(home))
            lock = _lock_control(directory)
        else:
            require(sys.platform.startswith('linux'), 'unsupported_platform')
            from marmish_transport_sandbox import _private
            home = Path(home)
            _private(home, True)
            path = home / '.marmish-transport.lock'
            lock = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
            _private(path, False)
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise SandboxError('daemon_home_in_use') from None
        yield
    finally:
        if lock is not None:
            os.close(lock)
        if directory is not None:
            os.close(directory)
