"""Native boundaries: exact layouts, fail-closed peer checks and private paths."""
import ctypes
import errno
import importlib.util
import os
from pathlib import Path
import socket
import signal
import tempfile
import unittest
from unittest.mock import Mock, patch
from types import SimpleNamespace

import marmish_platform as native
from mdk_smoke import SmokeError


class LinuxPidfdTests(unittest.TestCase):
    """No real pidfd operations: independently exercise both API boundaries."""
    def setUp(self):
        native._linux_pidfd_libc.cache_clear()
        self.addCleanup(native._linux_pidfd_libc.cache_clear)
        previous_errno = ctypes.get_errno()
        self.addCleanup(ctypes.set_errno, previous_errno)
        self.enterContext(patch.object(native.sys, 'platform', 'linux'))
        self.enterContext(patch.object(native.os, 'pidfd_open', None, create=True))
        self.enterContext(patch.object(native.signal, 'pidfd_send_signal', None, create=True))
        self.open = Mock(return_value=7)
        self.send = Mock(return_value=0)
        self.library = SimpleNamespace(pidfd_open=self.open, pidfd_send_signal=self.send)
        self.loader = self.enterContext(patch.object(native.ctypes, 'CDLL', return_value=self.library))

    def test_native_apis_accept_signal_enums_without_loading_libc(self):
        opening, sending = Mock(return_value=7), Mock(return_value=None)
        with patch.object(native.os, 'pidfd_open', opening), \
                patch.object(native.signal, 'pidfd_send_signal', sending):
            self.assertEqual(native.linux_pidfd_open(123), 7)
            opening.assert_called_once_with(123, 0)
            for number in (signal.SIGINT, signal.SIGTERM, signal.SIGKILL):
                self.assertIsNone(native.linux_pidfd_send_signal(7, number))
                sending.assert_called_with(7, int(number), None, 0)
                self.assertIs(type(sending.call_args.args[1]), int)
        self.loader.assert_not_called()

    def test_libc_signatures_flags_enums_and_descriptor_zero(self):
        self.open.return_value = 0
        self.assertEqual(native.linux_pidfd_open(123), 0)
        self.open.assert_called_once_with(123, 0)
        self.assertEqual(self.open.argtypes, [ctypes.c_int, ctypes.c_uint])
        self.assertIs(self.open.restype, ctypes.c_int)
        with patch.object(native.os, 'close') as close:
            for number in (signal.SIGINT, signal.SIGTERM, signal.SIGKILL):
                self.assertIsNone(native.linux_pidfd_send_signal(0, number))
                self.send.assert_called_with(0, int(number), None, 0)
                self.assertIs(type(self.send.call_args.args[1]), int)
            close.assert_not_called()
        self.assertEqual(self.send.argtypes,
                         [ctypes.c_int, ctypes.c_int, ctypes.c_void_p, ctypes.c_uint])
        self.assertIs(self.send.restype, ctypes.c_int)
        self.loader.assert_called_once_with(None, use_errno=True)

    def test_backend_selection_is_independent_per_operation(self):
        opening, sending = Mock(return_value=9), Mock(return_value=None)
        with patch.object(native.os, 'pidfd_open', opening):
            self.assertEqual(native.linux_pidfd_open(123), 9)
            native.linux_pidfd_send_signal(9, signal.SIGTERM)
        self.open.assert_not_called()
        self.send.assert_called_once_with(9, int(signal.SIGTERM), None, 0)
        self.send.reset_mock()
        with patch.object(native.signal, 'pidfd_send_signal', sending):
            self.assertEqual(native.linux_pidfd_open(123), 7)
            native.linux_pidfd_send_signal(7, signal.SIGKILL)
        self.open.assert_called_once_with(123, 0)
        sending.assert_called_once_with(7, int(signal.SIGKILL), None, 0)
        self.send.assert_not_called()

    def test_invalid_arguments_never_reach_either_backend(self):
        opening, sending = Mock(), Mock()
        with patch.object(native.os, 'pidfd_open', opening), \
                patch.object(native.signal, 'pidfd_send_signal', sending):
            for pid in (True, False, 0, -1, 1 << 31, '123', 1.5, signal.SIGINT):
                with self.subTest(pid=pid), self.assertRaises(ValueError):
                    native.linux_pidfd_open(pid)
            for fd in (True, False, -1, 1 << 31, '7', 1.5, signal.SIGINT):
                with self.subTest(fd=fd), self.assertRaises(ValueError):
                    native.linux_pidfd_send_signal(fd, signal.SIGINT)
            for number in (True, False, -1, signal.NSIG, 1 << 100, '2', 2.0):
                with self.subTest(number=number), self.assertRaises(ValueError):
                    native.linux_pidfd_send_signal(7, number)
        opening.assert_not_called()
        sending.assert_not_called()
        self.loader.assert_not_called()

    def test_valid_integer_boundaries_reach_the_abi_without_truncation(self):
        self.assertEqual(native.linux_pidfd_open((1 << 31) - 1), 7)
        self.open.assert_called_once_with((1 << 31) - 1, 0)
        for number in (0, signal.NSIG - 1):
            native.linux_pidfd_send_signal((1 << 31) - 1, number)
            self.send.assert_called_with((1 << 31) - 1, number, None, 0)

    def test_non_linux_refuses_before_native_or_libc_calls(self):
        opening, sending = Mock(), Mock()
        with patch.object(native.sys, 'platform', 'darwin'), \
                patch.object(native.os, 'pidfd_open', opening), \
                patch.object(native.signal, 'pidfd_send_signal', sending):
            for operation, args in ((native.linux_pidfd_open, (123,)),
                                    (native.linux_pidfd_send_signal, (7, signal.SIGINT))):
                with self.assertRaises(OSError) as caught:
                    operation(*args)
                self.assertEqual(caught.exception.errno, errno.ENOSYS)
        opening.assert_not_called()
        sending.assert_not_called()
        self.loader.assert_not_called()

    def test_libc_errors_preserve_errno_subclasses_and_never_retry(self):
        cases = ((native.linux_pidfd_open, (123,), self.open),
                 (native.linux_pidfd_send_signal, (7, signal.SIGINT), self.send))
        for operation, args, function in cases:
            for code in (errno.ESRCH, errno.EPERM, errno.ENOSYS, errno.EINVAL, errno.EINTR, 0):
                def fail(*_args):
                    self.assertEqual(ctypes.get_errno(), 0)
                    ctypes.set_errno(code)
                    return -1
                function.reset_mock()
                function.side_effect = fail
                ctypes.set_errno(errno.EBUSY)
                with self.subTest(operation=operation.__name__, code=code), \
                        patch.object(native.os, 'close') as close, self.assertRaises(OSError) as caught:
                    operation(*args)
                self.assertEqual(caught.exception.errno, code or errno.EIO)
                if code == errno.ESRCH:
                    self.assertIsInstance(caught.exception, ProcessLookupError)
                if code == errno.EPERM:
                    self.assertIsInstance(caught.exception, PermissionError)
                function.assert_called_once()
                close.assert_not_called()
            function.side_effect = None

    def test_native_errors_propagate_without_libc_fallback(self):
        for operation, args, module, name in (
                (native.linux_pidfd_open, (123,), native.os, 'pidfd_open'),
                (native.linux_pidfd_send_signal, (7, signal.SIGINT), native.signal, 'pidfd_send_signal')):
            for code in (errno.ESRCH, errno.EPERM, errno.ENOSYS, errno.EINTR):
                error = OSError(code, 'original')
                function = Mock(side_effect=error)
                with self.subTest(operation=operation.__name__, code=code), \
                        patch.object(module, name, function), self.assertRaises(OSError) as caught:
                    operation(*args)
                self.assertIs(caught.exception, error)
                function.assert_called_once()
        self.loader.assert_not_called()

    def test_missing_libc_symbols_are_independent_and_fail_closed(self):
        del self.library.pidfd_open
        with self.assertRaises(OSError) as caught:
            native.linux_pidfd_open(123)
        self.assertEqual(caught.exception.errno, errno.ENOSYS)
        native.linux_pidfd_send_signal(7, signal.SIGTERM)
        self.library.pidfd_open = self.open
        del self.library.pidfd_send_signal
        self.assertEqual(native.linux_pidfd_open(123), 7)
        with self.assertRaises(OSError) as caught:
            native.linux_pidfd_send_signal(7, signal.SIGTERM)
        self.assertEqual(caught.exception.errno, errno.ENOSYS)

    def test_libc_load_error_preserves_errno_or_uses_enosys(self):
        for error, expected in ((OSError(errno.EPERM, 'load'), errno.EPERM),
                                (OSError('no errno'), errno.ENOSYS)):
            for operation, args in ((native.linux_pidfd_open, (123,)),
                                    (native.linux_pidfd_send_signal, (7, signal.SIGINT))):
                native._linux_pidfd_libc.cache_clear()
                self.loader.side_effect = error
                with self.subTest(operation=operation.__name__, expected=expected), \
                        self.assertRaises(OSError) as caught:
                    operation(*args)
                self.assertEqual(caught.exception.errno, expected)
        self.open.assert_not_called()
        self.send.assert_not_called()

    def test_noncallable_native_api_does_not_trigger_fallback(self):
        with patch.object(native.os, 'pidfd_open', 123), \
                patch.object(native.signal, 'pidfd_send_signal', 456):
            with self.assertRaises(ValueError):
                native.linux_pidfd_open(123)
            with self.assertRaises(ValueError):
                native.linux_pidfd_send_signal(7, signal.SIGINT)
        self.loader.assert_not_called()

    def test_import_never_loads_libc(self):
        spec = importlib.util.spec_from_file_location('_pidfd_import_test', native.__file__)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        self.loader.assert_not_called()
        self.assertEqual(module._linux_pidfd_libc.cache_info().currsize, 0)


@unittest.skipUnless(native.sys.platform.startswith('linux'), 'Linux pidfd ABI')
class LinuxPidfdIntegrationTests(unittest.TestCase):
    def test_libc_own_process_descriptor_is_noninheritable(self):
        native._linux_pidfd_libc.cache_clear()
        self.addCleanup(native._linux_pidfd_libc.cache_clear)
        self.addCleanup(ctypes.set_errno, ctypes.get_errno())
        with patch.object(native.os, 'pidfd_open', None, create=True), \
                patch.object(native.signal, 'pidfd_send_signal', None, create=True):
            descriptor = native.linux_pidfd_open(os.getpid())
            try:
                self.assertFalse(os.get_inheritable(descriptor))
                # Signal zero checks the own-process descriptor, delivering no signal.
                self.assertIsNone(native.linux_pidfd_send_signal(descriptor, 0))
            finally:
                os.close(descriptor)


class NativeTests(unittest.TestCase):
    def test_creation_identity_stable_and_clock_advances(self):
        first = native.process_identity(os.getpid())
        self.assertEqual(first, native.process_identity(os.getpid()))
        self.assertEqual(first['pid'], os.getpid())
        self.assertGreater(first['start'], 0)
        self.assertEqual(native._uuid(first['boot_id']), first['boot_id'])
        self.assertLessEqual(native.continuous_time(), native.continuous_time())
        for value in (True, 0, -1, 1 << 31, '1'):
            with self.subTest(value=value), self.assertRaises(SmokeError):
                native.process_identity(value)

    def test_darwin_layout_and_uuid(self):
        self.assertEqual(ctypes.sizeof(native._BsdInfo), 136)
        self.assertEqual(native._BsdInfo.pid.offset, 12)
        self.assertEqual(native._BsdInfo.uid.offset, 20)
        self.assertEqual(native._BsdInfo.ruid.offset, 28)
        self.assertEqual(native._BsdInfo.seconds.offset, 120)
        self.assertEqual(native._BsdInfo.micros.offset, 128)
        self.assertEqual(native._uuid('ABCDEF00-ABCD-ABCD-ABCD-ABCDEF000000'),
                         'abcdef00-abcd-abcd-abcd-abcdef000000')
        for value in ('0' * 36, 'a' * 36, '00000000-0000-0000-0000-000000000000\n'):
            with self.assertRaises(SmokeError):
                native._uuid(value)

    def test_darwin_refuses_missing_credentials_or_changed_process(self):
        connection = Mock()
        connection.getsockopt.return_value = (123).to_bytes(4, byteorder='little', signed=True)
        identity = {'pid': 123, 'boot_id': 'x', 'start': 1}
        library = Mock()
        def credentials(_fd, uid, _gid):
            uid._obj.value = os.geteuid()
            return 0
        library.getpeereid.side_effect = credentials
        with patch.object(native.sys, 'platform', 'darwin'), \
                patch.object(native, '_darwin', return_value=(library, None)):
            with self.assertRaisesRegex(SmokeError, 'missing_peer_identity'):
                native.verify_peer(connection, os.geteuid())
            with patch.object(native, 'process_identity', return_value=identity):
                native.verify_peer(connection, os.geteuid(), identity)
            with patch.object(native, 'process_identity', return_value=dict(identity, start=2)):
                with self.assertRaisesRegex(SmokeError, 'wrong_peer_identity'):
                    native.verify_peer(connection, os.geteuid(), identity)
            library.getpeereid.return_value = -1
            library.getpeereid.side_effect = None
            with self.assertRaisesRegex(SmokeError, 'peer_credentials_failed'):
                native.verify_peer(connection, os.geteuid(), identity)

    def test_darwin_process_inspection_rejects_short_or_mismatched_records(self):
        library = Mock()
        def inspect(pid, _flavor, _arg, buffer, size):
            value = ctypes.cast(buffer, ctypes.POINTER(native._BsdInfo)).contents
            value.pid, value.uid, value.ruid = pid, os.geteuid(), os.geteuid()
            value.seconds, value.micros = 123, 456
            return size
        def boot(_name, buffer, length, _new, _newlen):
            value = b'ABCDEF00-ABCD-ABCD-ABCD-ABCDEF000000\0'
            ctypes.memmove(buffer, value, len(value))
            length._obj.value = len(value)
            return 0
        library.proc_pidinfo.side_effect = inspect
        library.sysctlbyname.side_effect = boot
        with patch.object(native.sys, 'platform', 'darwin'), \
                patch.object(native, '_darwin', return_value=(library, None)):
            self.assertEqual(native.process_identity(123)['start'], 123000456)
            library.proc_pidinfo.side_effect = lambda *_args: 135
            with self.assertRaisesRegex(SmokeError, 'process_identity_failed'):
                native.process_identity(123)
            library.proc_pidinfo.side_effect = inspect
            library.sysctlbyname.side_effect = lambda *_args: -1
            with self.assertRaisesRegex(SmokeError, 'process_identity_failed'):
                native.process_identity(123)

    def test_darwin_absence_requires_fresh_esrch(self):
        library = Mock()
        library.proc_pidinfo.return_value = 0
        with patch.object(native, '_darwin', return_value=(library, None)):
            ctypes.set_errno(3)
            with self.assertRaisesRegex(SmokeError, 'process_identity_failed'):
                native._darwin_inspect(123)
            def absent(*_args):
                ctypes.set_errno(3)
                return 0
            library.proc_pidinfo.side_effect = absent
            self.assertIsNone(native._darwin_inspect(123))

    def test_only_expected_root_owned_darwin_aliases_are_accepted(self):
        with patch.object(native.sys, 'platform', 'darwin'), \
                patch.object(Path, 'lstat', return_value=SimpleNamespace(st_uid=0)), \
                patch.object(Path, 'is_symlink', return_value=True), \
                patch.object(os, 'readlink', side_effect=lambda path: 'private/' + path.name):
            self.assertEqual(native.canonical_state_path('/tmp/job'), Path('/private/tmp/job'))
            self.assertEqual(native.canonical_state_path('/var/job'), Path('/private/var/job'))
            with patch.object(os, 'readlink', return_value='elsewhere'):
                with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                    native.canonical_state_path('/tmp/job')
            with patch.object(Path, 'lstat', return_value=SimpleNamespace(st_uid=123)):
                with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                    native.canonical_state_path('/tmp/job')
            with patch.object(Path, 'is_symlink', return_value=False):
                with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                    native.canonical_state_path('/var/job')
            with patch.object(Path, 'lstat', side_effect=FileNotFoundError):
                with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                    native.canonical_state_path('/tmp/job')
        for value in ('/tmp/../Users/test', '/state/../private', '../relative'):
            with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                native.canonical_state_path(value)

    def test_host_marker_is_bounded(self):
        with patch.dict(os.environ, {'EPIPHYTIC_EXECUTION_ID': 'run:123-abc'}):
            self.assertEqual(native.execution_marker(), {'EPIPHYTIC_EXECUTION_ID': 'run:123-abc'})
        for value in ('', 'secret\n', 'x' * 161, '$HOME', 'a/b'):
            with patch.dict(os.environ, {'EPIPHYTIC_EXECUTION_ID': value}):
                with self.assertRaisesRegex(SmokeError, 'invalid_execution_marker'):
                    native.execution_marker()

    def test_cleanup_failure_preserves_state_and_success_removes_under_guard(self):
        import marmish_protocol as protocol
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / 'state'
            protocol.private_directory(root)
            marker = root / 'private-data'
            marker.write_bytes(b'owned test state')
            bridge = Mock()
            bridge.call.side_effect = SmokeError('cleanup_incomplete')
            with patch.object(protocol, 'EngineBridge', return_value=bridge):
                with self.assertRaisesRegex(SmokeError, 'cleanup_incomplete'):
                    protocol.remove_engine_state('/unused', {'state_root': str(root)})
            self.assertTrue(marker.exists())
            bridge.close.assert_called_once()
            bridge.reset_mock()
            bridge.call.side_effect = None
            bridge.call.return_value = {'ready': True}
            original = protocol.shutil.rmtree
            def guarded_remove(path):
                bridge.close.assert_not_called()
                original(path)
            with patch.object(protocol, 'EngineBridge', return_value=bridge), \
                    patch.object(protocol.shutil, 'rmtree', side_effect=guarded_remove):
                protocol.remove_engine_state('/unused', {'state_root': str(root)})
            self.assertFalse(root.exists())
            bridge.close.assert_called_once()

    def test_private_symlink_is_never_resolved_away(self):
        from marmish_protocol import private_directory
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / 'real').mkdir(mode=0o700)
            (root / 'alias').symlink_to(root / 'real')
            with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                private_directory(root / 'alias' / 'child')


if __name__ == '__main__':
    unittest.main()
