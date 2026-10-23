"""Native boundaries: exact layouts, fail-closed peer checks and private paths."""
import ctypes
import os
from pathlib import Path
import socket
import tempfile
import unittest
from unittest.mock import Mock, patch
from types import SimpleNamespace

import marmish_platform as native
from mdk_smoke import SmokeError


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
