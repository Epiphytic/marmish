"""Select the native confinement boundary; unsupported hosts fail closed."""
import sys

from mdk_smoke import require
from marmish_transport_sandbox import ConfinedDaemon as LinuxConfinedDaemon, SandboxError


def ConfinedDaemon(home, wnd, relays, wnd_sha256, *, manifest=None):
    if sys.platform == 'darwin':
        from marmish_mdk_build import verify_manifest
        verify_manifest(wnd, wnd_sha256, manifest, expected_platform='darwin')
        from marmish_transport_macos import DarwinConfinedDaemon
        return DarwinConfinedDaemon(home, wnd, relays, wnd_sha256, manifest=manifest)
    require(sys.platform.startswith('linux'), 'unsupported_platform')
    if manifest is not None:
        from marmish_mdk_build import verify_manifest
        verify_manifest(wnd, wnd_sha256, manifest, expected_platform='linux')
    daemon = LinuxConfinedDaemon(home, wnd, relays, wnd_sha256)
    daemon.rpc_home, daemon.rpc_socket, daemon.peer_identity = '/state', '/state/s', None
    return daemon
