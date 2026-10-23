#!/usr/bin/env python3
"""Empirical Darwin gate before cold MDK builds; contains no identities."""
from __future__ import annotations

import argparse
import ctypes
import fcntl
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile

from marmish_platform import continuous_time, process_identity
from marmish_transport_macos import DarwinConfinedDaemon, SocksBroker, PROFILE, control_dir, _open_control, socket_path_fits
from marmish_transport_sandbox import SandboxError, canonical_relays


def storage_probe(home):
    result = subprocess.run(['/usr/sbin/diskutil', 'info', '-plist', str(home)], check=True,
                            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5)
    if len(result.stdout) > 65536 or plistlib.loads(result.stdout).get('FilesystemType') != 'apfs':
        raise SandboxError('unsupported_native_filesystem')
    library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
    library.renameatx_np.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    library.renameatx_np.restype = ctypes.c_int
    source, target = home / 'publish.pending', home / 'publish.key'
    with source.open('xb') as stream:
        stream.write(os.urandom(32))
        stream.flush()
        os.fsync(stream.fileno())
        fcntl.fcntl(stream.fileno(), 51)  # Darwin F_FULLFSYNC
    if library.renameatx_np(-2, os.fsencode(source), -2, os.fsencode(target), 4):
        raise SandboxError('native_exclusive_publish_failed')
    with source.open('xb') as stream:
        stream.write(b'do not overwrite')
    if library.renameatx_np(-2, os.fsencode(source), -2, os.fsencode(target), 4) != -1 or ctypes.get_errno() != 17:
        raise SandboxError('native_exclusive_publish_unsafe')
    directory = os.open(home, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--relay', action='append', required=True)
    args = parser.parse_args()
    if sys.platform != 'darwin' or os.geteuid() == 0:
        parser.error('native non-root Darwin runner required')
    relays = canonical_relays(args.relay)
    if shutil.which('clang') is None:
        raise SandboxError('missing_native_compiler')
    with tempfile.TemporaryDirectory(prefix='mn-', dir='/private/tmp') as temporary:
        root = Path(temporary).resolve()
        root.chmod(0o700)
        home = root / 'data'
        home.mkdir(mode=0o700)
        control_fd = _open_control(home)
        os.close(control_fd)
        runtime = control_dir(home) / '.marmish-native-spike'
        runtime.mkdir(mode=0o700)
        for name in ('marmish-probe', 'marmish-lifetime'):
            subprocess.run(['clang', '-Wall', '-Wextra', '-Werror', '-O2', '-mmacosx-version-min=15.0',
                            str(Path(__file__).resolve().parent / 'native' / (name + '.c')),
                            '-o', str(runtime / name)], check=True, timeout=60)
            subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', str(runtime / name)],
                           check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
        (runtime / 'profile.sb').write_text(PROFILE)
        identity = process_identity(os.getpid())
        if identity['pid'] != os.getpid() or continuous_time() <= 0:
            raise SandboxError('native_identity_probe_failed')
        storage_probe(home)
        if not socket_path_fits(home):
            raise SandboxError('native_socket_path_too_long')
        daemon = DarwinConfinedDaemon(home, runtime / 'unused-wnd', relays, '0' * 64)
        daemon._directory = runtime
        daemon._broker = SocksBroker(relays).start()
        try:
            daemon._preflight()
        finally:
            daemon._broker.close()
        print(json.dumps({'platform': 'darwin', 'identity': 'passed', 'storage': 'passed',
                          'confinement': 'passed', 'relay_count': len(relays)}))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        print(str(error) if isinstance(error, SandboxError) else 'native_spike_failed', file=sys.stderr)
        raise SystemExit(1)
