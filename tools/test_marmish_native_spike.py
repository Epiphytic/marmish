"""Hermetic diagnostics: no Darwin, network or keychain calls."""
import contextlib
import ctypes
import errno
import io
import json
import os
from pathlib import Path
import plistlib
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch
import marmish_native_spike as spike
import marmish_transport_macos as native
from marmish_transport_sandbox import SandboxError


def records(diagnostic):
    output = io.StringIO()
    with contextlib.redirect_stderr(output):
        diagnostic.emit()
    return [json.loads(line) for line in output.getvalue().splitlines()]


class DiagnosticTests(unittest.TestCase):
    def test_each_stage_redacts_and_freezes_failure(self):
        secret = 'private-secret-command-path-output-nsec'
        errors = (OSError(13, secret, secret), subprocess.CalledProcessError(19, [secret], secret, secret),
                  subprocess.TimeoutExpired([secret], 1, secret, secret), ValueError(secret), AttributeError(secret))
        for stage in spike.STAGES:
            for error in errors:
                diagnostic = spike.Diagnostics()
                with self.assertRaises(type(error)):
                    diagnostic.run(stage, lambda: (_ for _ in ()).throw(error))
                diagnostic.cleanup('workspace_cleanup', lambda: None)
                result = records(diagnostic)
                self.assertEqual(len(result), 1)
                self.assertEqual(result[0]['stage'], stage)
                self.assertEqual(set(result[0]), {'outcome', 'stage', 'category', 'errno', 'exit_code'})
                self.assertNotIn(secret, json.dumps(result))
        for value in (True, False, -256, 256, '7'):
            self.assertIsNone(spike._number(value, -255, 255))

    def test_duplicate_and_all_cleanup_failures_keep_primary_status(self):
        diagnostic = spike.Diagnostics()
        diagnostic('failed', 'preflight_broker_confined', SandboxError('native_probe_broker'), 65)
        diagnostic('failed', 'preflight_broker_confined', SandboxError('native_probe_broker'))
        for stage in ('broker_cleanup', 'workspace_cleanup', 'preflight_files_cleanup', 'preflight_outside_cleanup'):
            diagnostic.cleanup(stage, lambda: (_ for _ in ()).throw(OSError(13, 'private')))
        result = records(diagnostic)
        self.assertEqual(result[0]['stage'], 'preflight_broker_confined')
        self.assertEqual(result[0]['exit_code'], 65)
        self.assertEqual(len(result), 5)
        self.assertTrue(all(value['errno'] == 13 for value in result[1:]))
        with self.assertRaises(TypeError):
            diagnostic.primary['stage'] = 'unexpected'
        diagnostic = spike.Diagnostics()
        with self.assertRaises(OSError):
            diagnostic.cleanup('workspace_cleanup', lambda: (_ for _ in ()).throw(OSError(5, 'private')))
        self.assertEqual(records(diagnostic)[0]['stage'], 'workspace_cleanup')

    def test_preflight_observer_default_and_exact_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            daemon = native.DarwinConfinedDaemon(home, home / 'wnd', ['wss://selected.example/'], '0' * 64)
            daemon._directory = home
            daemon._broker = SimpleNamespace(address=('127.0.0.1', 1))
            diagnostic = spike.Diagnostics()
            with patch.object(native.subprocess, 'run', return_value=SimpleNamespace(returncode=65)):
                for observer in (diagnostic, lambda *args: True, None):
                    with self.assertRaisesRegex(SandboxError, 'native_control_broker'):
                        daemon._preflight(observer)
            self.assertEqual(records(diagnostic)[0]['exit_code'], 65)
            self.assertEqual(records(diagnostic)[0]['stage'], 'preflight_broker_control')
            with patch.object(native.subprocess, 'run') as run:
                with self.assertRaisesRegex(ValueError, 'observer'):
                    daemon._preflight(lambda *args: (_ for _ in ()).throw(ValueError('observer')))
                run.assert_not_called()

    def test_main_success_and_unexpected_failure(self):
        for failure in (None, RuntimeError('private-secret')):
            output, errors = io.StringIO(), io.StringIO()
            with patch.object(spike.sys, 'platform', 'darwin'), patch.object(spike.os, 'geteuid', return_value=501), \
                    patch.object(spike, 'run_spike', side_effect=failure), \
                    contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                status = spike.main(['--relay', 'wss://selected.example/'])
            self.assertEqual(status, int(failure is not None))
            self.assertNotIn('private-secret', output.getvalue() + errors.getvalue())
            if failure:
                self.assertEqual(json.loads(errors.getvalue())['stage'], 'unexpected')
                self.assertEqual(output.getvalue(), '')
            else:
                self.assertEqual(json.loads(output.getvalue()), {'platform': 'darwin', 'identity': 'passed',
                    'storage': 'passed', 'confinement': 'passed', 'relay_count': 1})

    def test_actual_spike_cleanup_order(self):
        for primary_failure, cleanup_failure in ((True, False), (True, True), (False, True)):
            with tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                control = root / 'control'
                control.mkdir()
                def fail(diagnostic):
                    if primary_failure:
                        diagnostic.run('preflight_dns_confined', lambda: (_ for _ in ()).throw(OSError(13, 'private')))
                daemon = SimpleNamespace(_preflight=fail)
                broker = SimpleNamespace(close=MagicMock(side_effect=OSError(5, 'private') if cleanup_failure else None))
                workspace = SimpleNamespace(name=str(root), cleanup=MagicMock(side_effect=OSError(5, 'private') if cleanup_failure else None))
                diagnostic = spike.Diagnostics()
                with patch.object(spike.tempfile, 'TemporaryDirectory', return_value=workspace), \
                        patch.object(spike, '_open_control', side_effect=lambda h: os.open(h, os.O_RDONLY)), \
                        patch.object(spike, 'control_dir', return_value=control), \
                        patch.object(spike.shutil, 'which', return_value='/compiler'), \
                        patch.object(spike.subprocess, 'run') as commands, patch.object(spike, 'storage_probe'), \
                        patch.object(spike, 'continuous_time', return_value=1), \
                        patch.object(spike, 'process_identity', return_value={'pid': os.getpid()}), \
                        patch.object(spike, 'DarwinConfinedDaemon', return_value=daemon), \
                        patch.object(spike, 'SocksBroker', return_value=SimpleNamespace(start=lambda: broker)):
                    with self.assertRaises(OSError):
                        spike.run_spike(['wss://selected.example/'], diagnostic)
                result = records(diagnostic)
                self.assertEqual(result[0]['stage'], 'preflight_dns_confined' if primary_failure else 'broker_cleanup')
                self.assertEqual(len(result), (3 if primary_failure else 2) if cleanup_failure else 1)
                self.assertEqual(commands.call_count, 4)
                for call in commands.call_args_list:
                    self.assertEqual(call.kwargs['stdout'], subprocess.DEVNULL)
                    self.assertEqual(call.kwargs['stderr'], subprocess.DEVNULL)
                broker.close.assert_called_once()
                workspace.cleanup.assert_called_once()

    def test_preflight_primary_survives_real_cleanup_paths(self):
        original_temporary, original_unlink = tempfile.TemporaryDirectory, Path.unlink
        for failure_kind in ('files', 'outside'):
            with original_temporary() as directory:
                home = Path(directory)
                runtime, control = home / 'runtime', home / 'control'
                runtime.mkdir()
                control.mkdir()
                probe = runtime / 'marmish-probe'
                probe.write_bytes(b'owned probe')
                daemon = native.DarwinConfinedDaemon(home, home / 'wnd', ['wss://selected.example/'], '0' * 64)
                daemon._directory, daemon._control = runtime, control
                daemon._broker = SimpleNamespace(address=('127.0.0.1', 123), allowed={('selected.example', 443)})
                outside_directories = []
                def temporary(*args, **kwargs):
                    value = original_temporary(dir=home)
                    outside_directories.append(value)
                    if failure_kind == 'outside':
                        return SimpleNamespace(name=value.name, cleanup=lambda: (_ for _ in ()).throw(OSError(5, 'private')))
                    return value
                def invoke(command, **kwargs):
                    if command[0] == '/sbin/ifconfig':
                        return SimpleNamespace(stdout=b' inet 10.0.0.1 ', returncode=0)
                    confined = command[0] == '/usr/bin/sandbox-exec'
                    args = command[command.index(str(probe)) + 1:]
                    mode = args[0]
                    destination = Path(args[1]) if len(args) > 1 else home
                    if not confined:
                        if ((failure_kind == 'files' and mode == 'write' and destination.name == '.marmish-probe-record')
                                or (failure_kind == 'outside' and mode == 'read' and destination.name == 'sentinel')):
                            return SimpleNamespace(returncode=65)
                        if mode in ('unix-bind', 'create'):
                            destination.write_bytes(b'owned')
                        if mode == 'rename':
                            os.rename(args[1], args[2])
                        return SimpleNamespace(returncode=0)
                    allowed = ((mode == 'tcp' and args[1] == '127.0.0.1' and str(args[2]) == '123')
                               or (mode in ('read', 'write') and destination.name == 'private-probe')
                               or mode == 'unix-staged'
                               or (mode == 'create' and destination.name == 'restricted.ack.tmp')
                               or (mode == 'rename' and Path(args[2]).name == 'restricted.ack'))
                    return SimpleNamespace(returncode=0 if allowed else 2)
                def unlink(path, *args, **kwargs):
                    if failure_kind == 'files' and path.name == '.marmish-probe-record':
                        raise OSError(5, 'private')
                    return original_unlink(path, *args, **kwargs)
                sock = MagicMock()
                sock.__enter__.return_value = sock
                sock.getsockname.return_value = ('127.0.0.1', 456)
                diagnostic = spike.Diagnostics()
                try:
                    with patch.object(native.tempfile, 'TemporaryDirectory', side_effect=temporary), \
                            patch.object(native.subprocess, 'run', side_effect=invoke), \
                            patch.object(native.socket, 'socket', return_value=sock), \
                            patch.object(Path, 'unlink', unlink):
                        with self.assertRaises(SandboxError):
                            daemon._preflight(diagnostic)
                    result = records(diagnostic)
                    primary = 'preflight_control_record_write_control' if failure_kind == 'files' else 'preflight_outside_read_control'
                    cleanup = 'preflight_files_cleanup' if failure_kind == 'files' else 'preflight_outside_cleanup'
                    self.assertEqual(result[0]['stage'], primary)
                    self.assertEqual(result[0]['exit_code'], 65)
                    self.assertEqual(result[1]['stage'], cleanup)
                finally:
                    for value in outside_directories:
                        value.cleanup()


class StorageTests(unittest.TestCase):
    def test_errno_clear_and_collision_snapshot(self):
        for collision_errno in (errno.EEXIST, errno.EACCES, 0):
            with tempfile.TemporaryDirectory() as temporary:
                home, observed = Path(temporary), []
                def rename(_a, source, _b, target, _flags):
                    observed.append(ctypes.get_errno())
                    if len(observed) == 1:
                        os.rename(source, target)
                        return 0
                    ctypes.set_errno(collision_errno)
                    return -1
                result = SimpleNamespace(stdout=plistlib.dumps({'FilesystemType': 'apfs'}))
                diagnostic = spike.Diagnostics()
                ctypes.set_errno(errno.ESRCH)
                with patch.object(spike.ctypes, 'CDLL', return_value=SimpleNamespace(renameatx_np=MagicMock(side_effect=rename))), \
                        patch.object(spike.subprocess, 'run', return_value=result), \
                        patch.object(spike.os, 'fsync'), patch.object(spike.fcntl, 'fcntl'):
                    if collision_errno == errno.EEXIST:
                        spike.storage_probe(home, diagnostic)
                    else:
                        with self.assertRaises(OSError if collision_errno else SandboxError):
                            spike.storage_probe(home, diagnostic)
                self.assertEqual(observed, [0, 0])
                if collision_errno == errno.EEXIST:
                    self.assertIsNone(diagnostic.primary)
                else:
                    self.assertEqual(diagnostic.primary['stage'], 'storage_collision')
                    self.assertEqual(diagnostic.primary['errno'], collision_errno or None)

    def test_failed_native_call_reports_only_its_snapshot_errno(self):
        for number in (0, errno.EACCES):
            with tempfile.TemporaryDirectory() as directory:
                def rename(*args):
                    self.assertEqual(ctypes.get_errno(), 0)
                    ctypes.set_errno(number)
                    return -1
                diagnostic = spike.Diagnostics()
                ctypes.set_errno(errno.ESRCH)
                with patch.object(spike.ctypes, 'CDLL', return_value=SimpleNamespace(renameatx_np=MagicMock(side_effect=rename))), \
                        patch.object(spike.subprocess, 'run', return_value=SimpleNamespace(stdout=plistlib.dumps({'FilesystemType': 'apfs'}))), \
                        patch.object(spike.os, 'fsync'), patch.object(spike.fcntl, 'fcntl'):
                    with self.assertRaises(OSError if number else SandboxError):
                        spike.storage_probe(Path(directory), diagnostic)
                self.assertEqual(diagnostic.primary['stage'], 'storage_publish')
                self.assertEqual(diagnostic.primary['errno'], number or None)
                self.assertEqual(diagnostic.primary['category'], 'native_io_failed' if number else 'native_exclusive_publish_failed')

    def test_actual_storage_operation_failures_have_exact_stage(self):
        original_open, original_path_open = os.open, Path.open
        targets = ('storage_create', 'storage_write', 'storage_fsync', 'storage_fullfsync',
                   'storage_directory_open', 'storage_directory_fsync', 'storage_close')
        for target in targets:
            with tempfile.TemporaryDirectory() as directory:
                home = Path(directory)
                diagnostic = spike.Diagnostics()
                fsyncs = []
                def fail():
                    raise OSError(errno.EACCES, 'private-storage-path')
                def path_open(path, *args, **kwargs):
                    if target == 'storage_create':
                        fail()
                    stream = original_path_open(path, *args, **kwargs)
                    if target != 'storage_close':
                        return stream
                    def close():
                        stream.close()
                        fail()
                    return SimpleNamespace(write=stream.write, flush=stream.flush,
                                           fileno=stream.fileno, close=close)
                def fsync(fd):
                    fsyncs.append(fd)
                    if ((target == 'storage_fsync' and len(fsyncs) == 1)
                            or (target == 'storage_directory_fsync' and len(fsyncs) == 2)):
                        fail()
                def open_directory(path, *args, **kwargs):
                    if target == 'storage_directory_open' and path == home:
                        fail()
                    return original_open(path, *args, **kwargs)
                def rename(_a, source, _b, destination, _flags):
                    if Path(destination.decode()).exists():
                        ctypes.set_errno(errno.EEXIST)
                        return -1
                    os.rename(source, destination)
                    return 0
                function = MagicMock(side_effect=rename)
                with patch.object(spike.subprocess, 'run', return_value=SimpleNamespace(stdout=plistlib.dumps({'FilesystemType': 'apfs'}))), \
                        patch.object(spike.ctypes, 'CDLL', return_value=SimpleNamespace(renameatx_np=function)), \
                        patch.object(Path, 'open', path_open), patch.object(spike.os, 'open', side_effect=open_directory), \
                        patch.object(spike.os, 'fsync', side_effect=fsync), \
                        patch.object(spike.os, 'urandom', side_effect=lambda size: fail() if target == 'storage_write' else bytes(size)), \
                        patch.object(spike.fcntl, 'fcntl', side_effect=lambda *args: fail() if target == 'storage_fullfsync' else None):
                    with self.assertRaises(OSError):
                        spike.storage_probe(home, diagnostic)
                self.assertEqual(diagnostic.primary['stage'], target)
                self.assertEqual(diagnostic.primary['errno'], errno.EACCES)
                if target == 'storage_close':
                    function.assert_not_called()

    def test_bad_plist_and_subprocess_errors(self):
        for payload in (b'<plist><private', plistlib.dumps(['private'])):
            diagnostic = spike.Diagnostics()
            with patch.object(spike.subprocess, 'run', return_value=SimpleNamespace(stdout=payload)):
                with self.assertRaises(Exception):
                    spike.storage_probe(Path('/never-used'), diagnostic)
            self.assertEqual(diagnostic.primary['stage'], 'storage_plist')
            self.assertNotIn('private', json.dumps(dict(diagnostic.primary)))
        diagnostic = spike.Diagnostics()
        with patch.object(spike.subprocess, 'run', side_effect=subprocess.CalledProcessError(3, ['private'])):
            with self.assertRaises(subprocess.CalledProcessError):
                spike.storage_probe(Path('/never-used'), diagnostic)
        self.assertEqual(diagnostic.primary['exit_code'], 3)
        self.assertEqual(diagnostic.primary['stage'], 'storage_diskutil')


class WorkflowTests(unittest.TestCase):
    def source(self):
        return (Path(__file__).resolve().parent.parent / '.github/workflows/native-validation.yml').read_text()

    def block(self, name):
        value = self.source().split('      - name: ' + name + '\n', 1)[1].split('      - ', 1)[0]
        lines = []
        for line in value.splitlines():
            if line.strip() and not line.startswith('        '):
                break
            lines.append(line)
        return '\n'.join(lines) + '\n'

    def execute(self, block, root, checkout='failure', offline='success', e2e='skipped'):
        script = '\n'.join(line[10:] for line in block.split('        run: |\n', 1)[1].splitlines())
        for name, value in (('checkout', checkout), ('offline', offline), ('e2e', e2e)):
            script = script.replace('${{ steps.' + name + '.outcome }}', value)
        return subprocess.run(['/bin/sh', '-eu', '-c', script], cwd=root,
            env={'RUNNER_TEMP': str(root), 'PATH': str(root)}, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def test_missing_checkout_and_retention_never_run_repo_script(self):
        names = ('Remove owned Bubblewrap prefix', 'Clean owned processes and private profiles',
                 'Remove owned Bubblewrap prefix after verified private cleanup')
        with tempfile.TemporaryDirectory() as temporary:
            root, marker = Path(temporary), Path(temporary) / 'called'
            executable = root / 'python3'
            executable.write_text('#!/bin/sh\necho called >> "$RUNNER_TEMP/called"\n')
            executable.chmod(0o700)
            for name in names:
                state = root / ('interop-state' if name.startswith('Clean') else 'bwrap-state')
                for stale in (False, True):
                    if stale:
                        state.symlink_to(root / 'absent')
                    result = self.execute(self.block(name), root)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertEqual(json.loads(result.stdout)['category'],
                        'cleanup_retained_unverified' if stale else 'cleanup_not_initialized')
                    self.assertFalse(marker.exists())
                    if stale:
                        state.unlink()
            for status in ('failure', 'cancelled'):
                result = self.execute(self.block(names[0]), root, checkout='success', offline=status)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(json.loads(result.stdout)['category'],
                    'cleanup_retained_test_failure' if status == 'failure' else 'cleanup_retained_cancellation')
                self.assertFalse(marker.exists())
            result = self.execute(self.block(names[0]), root, checkout='success')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue(marker.exists())
            marker.unlink()
            (root / 'interop-state').write_text('retained')
            result = self.execute(self.block(names[2]), root, checkout='success')
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(marker.exists())

    def test_shim_order_and_mac_gate(self):
        text = self.source()
        self.assertEqual(text.count('        id: checkout\n'), 2)
        self.assertLess(text.index('Native disposable macOS keychain acceptance'), text.index('Native unconfined shim handoff'))
        self.assertLess(text.index('Native unconfined shim handoff'), text.index('Native prerequisite spike'))
        block = self.block('Native unconfined shim handoff')
        self.assertIn("if: runner.os == 'macOS'", block)
        self.assertIn('timeout-minutes: 2', block)
        self.assertIn('PYTHONPATH=tools python3 -m unittest test_marmish_transport_macos.NativeLifetimeTests.test_bootstrap_child_reads_exact_frame_parent_uses_devnull', block)
