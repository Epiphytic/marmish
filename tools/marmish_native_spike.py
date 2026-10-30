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
from types import MappingProxyType

from marmish_platform import continuous_time, process_identity
from marmish_transport_macos import DarwinConfinedDaemon, SocksBroker, PROFILE, control_dir, _open_control, socket_path_fits
from marmish_transport_sandbox import SandboxError, canonical_relays

PROBES = ('broker', 'home_read', 'home_write', 'rpc_staging', 'outside_read',
          'outside_write', 'other_tcp', 'udp', 'other_unix', 'other_unix_bind',
          'ipv6', 'interface', 'dns', 'control_create', 'ack_create', 'ack_publish',
          *(f'control_{item}_{mode}' for item in ('record', 'config', 'binary')
            for mode in ('write', 'truncate', 'unlink', 'rename')))
PREFLIGHT_STAGES = {f'preflight_{probe}_{part}' for probe in PROBES
                    for part in ('control', 'reset', 'confined')} | {
    'preflight_' + item for item in ('home_fixture', 'outside_create', 'outside_fixture',
    'tcp_fixture', 'unix_fixture', 'ipv6_fixture', 'outside_cleanup', 'interfaces',
    'interface_fixture', 'dns_fixture', 'control_fixture', 'files_cleanup')}
STAGES = PREFLIGHT_STAGES | {
    'arguments', 'compiler', 'workspace', 'control', 'profile', 'identity', 'clock',
    'socket_budget', 'daemon_fixture', 'broker', 'confinement', 'broker_cleanup',
    'workspace_cleanup', 'unexpected', 'storage_diskutil', 'storage_plist',
    'storage_abi', 'storage_create', 'storage_write', 'storage_fsync',
    'storage_fullfsync', 'storage_close', 'storage_publish', 'storage_collision',
    'storage_directory_open', 'storage_directory_fsync', 'storage_directory_close',
    *(f'{action}_{helper}' for action in ('compile', 'sign')
      for helper in ('marmish-probe', 'marmish-lifetime'))}
REFUSALS = {'native_control_' + name for name in PROBES} | {
    'native_probe_' + name for name in PROBES} | {
    'unsupported_native_filesystem', 'native_exclusive_publish_failed',
    'native_exclusive_publish_unsafe', 'native_identity_probe_failed',
    'native_socket_path_too_long', 'missing_native_compiler',
    'native_interface_probe_failed', 'native_interface_probe_unavailable'}


def _number(value, low, high):
    return value if type(value) is int and low <= value <= high else None


class Diagnostics:
    """First failure is frozen before cleanup; never render exception contents."""
    def __init__(self):
        self.primary = None
        self.secondary = []

    def __call__(self, event, stage, error=None, exit_code=None):
        if stage not in STAGES or event not in ('begin', 'failed', 'cleanup_failed'):
            raise ValueError('invalid_diagnostic_event')
        if event == 'begin':
            return
        category, number = 'native_unexpected_failed', None
        if isinstance(error, SandboxError) and str(error) in REFUSALS:
            category = str(error)
        elif isinstance(error, subprocess.TimeoutExpired):
            category = 'native_timeout'
        elif isinstance(error, subprocess.CalledProcessError):
            category = 'native_subprocess_failed'
            exit_code = error.returncode
        elif isinstance(error, OSError):
            category, number = 'native_io_failed', error.errno
        elif isinstance(error, (ValueError, TypeError)):
            category = 'native_result_invalid'
        record = MappingProxyType({'outcome': 'failed', 'category': category, 'stage': stage,
                                   'errno': _number(number, 1, 4095),
                                   'exit_code': _number(exit_code, -255, 255)})
        if self.primary is None:
            self.primary = record
        elif event == 'cleanup_failed':
            self.secondary.append(record)

    def run(self, stage, action):
        self('begin', stage)
        try:
            return action()
        except Exception as error:
            self('failed', stage, error)
            raise

    def cleanup(self, stage, action):
        self('begin', stage)
        try:
            action()
        except Exception as error:
            prior = self.primary is not None
            self('cleanup_failed', stage, error)
            if not prior:
                raise

    def emit(self):
        for record in ([self.primary] if self.primary else []) + self.secondary:
            print(json.dumps(dict(record), sort_keys=True), file=sys.stderr)


def storage_probe(home, diagnostic):
    run = diagnostic.run
    result = run('storage_diskutil', lambda: subprocess.run(
        ['/usr/sbin/diskutil', 'info', '-plist', str(home)], check=True,
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5))
    def plist():
        if len(result.stdout) > 65536 or plistlib.loads(result.stdout).get('FilesystemType') != 'apfs':
            raise SandboxError('unsupported_native_filesystem')
    run('storage_plist', plist)
    def abi():
        library = ctypes.CDLL('/usr/lib/libSystem.B.dylib', use_errno=True)
        library.renameatx_np.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
        library.renameatx_np.restype = ctypes.c_int
        return library
    library = run('storage_abi', abi)
    source, target = home / 'publish.pending', home / 'publish.key'
    stream = run('storage_create', lambda: source.open('xb'))
    try:
        def write():
            stream.write(os.urandom(32))
            stream.flush()
        run('storage_write', write)
        run('storage_fsync', lambda: os.fsync(stream.fileno()))
        run('storage_fullfsync', lambda: fcntl.fcntl(stream.fileno(), 51))
    finally:
        diagnostic.cleanup('storage_close', stream.close)
    def rename(collision):
        ctypes.set_errno(0)
        result = library.renameatx_np(-2, os.fsencode(source), -2, os.fsencode(target), 4)
        number = ctypes.get_errno()
        if collision:
            if result == -1 and number not in (0, 17):
                raise OSError(number, 'native_exclusive_publish_unsafe')
            if result != -1 or number != 17:
                raise SandboxError('native_exclusive_publish_unsafe')
        elif result:
            if result == -1 and number:
                raise OSError(number, 'native_exclusive_publish_failed')
            raise SandboxError('native_exclusive_publish_failed')
    run('storage_publish', lambda: rename(False))
    def collision():
        with source.open('xb') as output:
            output.write(b'do not overwrite')
        rename(True)
    run('storage_collision', collision)
    directory = run('storage_directory_open', lambda: os.open(home, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC))
    try:
        run('storage_directory_fsync', lambda: os.fsync(directory))
    finally:
        diagnostic.cleanup('storage_directory_close', lambda: os.close(directory))


def run_spike(relays, diagnostic):
    run = diagnostic.run
    temporary = broker = None
    try:
        def compiler():
            if shutil.which('clang') is None:
                raise SandboxError('missing_native_compiler')
        run('compiler', compiler)
        temporary = run('workspace', lambda: tempfile.TemporaryDirectory(prefix='mn-', dir='/private/tmp'))
        root = run('workspace', lambda: Path(temporary.name).resolve())
        home = root / 'data'
        def control():
            root.chmod(0o700)
            home.mkdir(mode=0o700)
            descriptor = _open_control(home)
            os.close(descriptor)
            runtime = control_dir(home) / '.marmish-native-spike'
            runtime.mkdir(mode=0o700)
            return runtime
        runtime = run('control', control)
        for name in ('marmish-probe', 'marmish-lifetime'):
            run('compile_' + name, lambda n=name: subprocess.run(
                ['clang', '-Wall', '-Wextra', '-Werror', '-O2', '-mmacosx-version-min=15.0',
                 str(Path(__file__).resolve().parent / 'native' / (n + '.c')), '-o', str(runtime / n)],
                check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60))
            run('sign_' + name, lambda n=name: subprocess.run(
                ['/usr/bin/codesign', '--force', '--sign', '-', str(runtime / n)],
                check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10))
        run('profile', lambda: (runtime / 'profile.sb').write_text(PROFILE))
        def identity():
            if process_identity(os.getpid())['pid'] != os.getpid():
                raise SandboxError('native_identity_probe_failed')
        run('identity', identity)
        def clock():
            if continuous_time() <= 0:
                raise SandboxError('native_identity_probe_failed')
        run('clock', clock)
        storage_probe(home, diagnostic)
        def budget():
            if not socket_path_fits(home):
                raise SandboxError('native_socket_path_too_long')
        run('socket_budget', budget)
        daemon = run('daemon_fixture', lambda: DarwinConfinedDaemon(home, runtime / 'unused-wnd', relays, '0' * 64))
        daemon._directory = runtime
        broker = run('broker', lambda: SocksBroker(relays).start())
        daemon._broker = broker
        run('confinement', lambda: daemon._preflight(diagnostic=diagnostic))
    finally:
        try:
            if broker is not None:
                diagnostic.cleanup('broker_cleanup', broker.close)
        finally:
            if temporary is not None:
                diagnostic.cleanup('workspace_cleanup', temporary.cleanup)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--relay', action='append', required=True)
    args = parser.parse_args(argv)
    if sys.platform != 'darwin' or os.geteuid() == 0:
        parser.error('native non-root Darwin runner required')
    diagnostic = Diagnostics()
    try:
        relays = diagnostic.run('arguments', lambda: canonical_relays(args.relay))
        run_spike(relays, diagnostic)
    except Exception as error:
        if diagnostic.primary is None:
            diagnostic('failed', 'unexpected', error)
    if diagnostic.primary:
        diagnostic.emit()
        return 1
    print(json.dumps({'platform': 'darwin', 'identity': 'passed', 'storage': 'passed',
                      'confinement': 'passed', 'relay_count': len(relays)}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
