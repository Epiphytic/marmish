"""Authenticated package selection and private CI executable boundaries."""
import hashlib
import io
import json
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch, Mock

import marmish_ci_bwrap as bwrap


class BwrapTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def record(self, **changes):
        row = dict(Package='bubblewrap', Version=bwrap.FLOORS['noble'], Architecture='amd64',
                   Filename='pool/main/b/bubblewrap/bubblewrap_x_amd64.deb', Size='512', SHA256='a' * 64)
        row.update(changes)
        return '\n'.join(key + ': ' + value for key, value in row.items()) + '\n\n'

    def test_identical_duplicate_records_allowed_conflicting_records_rejected(self):
        record = self.record()
        self.assertEqual(bwrap.metadata((record * 2).encode(), bwrap.FLOORS['noble'], 'amd64'), (512, 'a' * 64))
        for changes in [dict(SHA256='b' * 64), dict(Size='1024'), dict(Architecture='arm64'),
                        dict(Filename='../../outside')]:
            with self.subTest(changes=changes), self.assertRaises(RuntimeError):
                bwrap.metadata((record + self.record(**changes)).encode(), bwrap.FLOORS['noble'], 'amd64')
        for changes in [dict(Size='9999999'), dict(SHA256='bad'), dict(Package='other')]:
            with self.assertRaises(RuntimeError):
                bwrap.metadata(self.record(**changes).encode(), bwrap.FLOORS['noble'], 'amd64')

    def test_version_floor_independent_of_valid_until(self):
        # Package metadata has no Valid-Until; dpkg ordering enforces the floor.
        for suite, floor in bwrap.FLOORS.items():
            if Path('/usr/bin/dpkg').exists():
                self.assertEqual(subprocess.run(['/usr/bin/dpkg', '--compare-versions', floor, 'ge', floor]).returncode, 0)
                self.assertNotEqual(subprocess.run(['/usr/bin/dpkg', '--compare-versions', '0.1', 'ge', floor]).returncode, 0)
            self.assertIn(suite, ('noble', 'resolute'))

    def test_apt_isolation_and_exact_signed_sources(self):
        env = bwrap.apt_config(self.root, 'resolute', 'arm64')
        text = Path(env['APT_CONFIG']).read_text()
        for key in ('Dir::Etc::main', 'Dir::Etc::parts', 'Dir::Etc::trustedparts',
                    'Dir::Etc::preferences', 'Dir::Etc::preferencesparts',
                    'Dir::Etc::netrc', 'Dir::Etc::netrcparts',
                    'Dir::State::status', 'Acquire::Check-Date', 'APT::Update::Error-Mode'):
            self.assertIn(key, text)
        for name in ('preferences', 'auth.conf'):
            self.assertEqual((self.root / name).read_bytes(), b'')
        self.assertNotIn('APT::Sandbox::User', text)
        self.assertNotIn('Valid-Until "false"', text)
        sources = (self.root / 'sources.list').read_text()
        self.assertEqual(sources.count('https://ports.ubuntu.com/ubuntu-ports'), 3)
        self.assertEqual(sources.count('signed-by=' + str(bwrap.KEYRING)), 3)
        self.assertNotIn('trusted=yes', sources)
        self.assertEqual((self.root / 'status').read_bytes(), b'')

    def tar(self, *, name='./usr/bin/bwrap', mode=0o755, kind=tarfile.REGTYPE, machine=62, duplicate=False):
        data = bytearray(64)
        data[:6] = b'\x7fELF\x02\x01'
        struct.pack_into('<H', data, 18, machine)
        body = io.BytesIO()
        with tarfile.open(fileobj=body, mode='w') as archive:
            for _ in range(2 if duplicate else 1):
                entry = tarfile.TarInfo(name)
                entry.type, entry.mode, entry.size, entry.linkname = kind, mode, len(data), 'outside'
                archive.addfile(entry, io.BytesIO(data))
        return body.getvalue()

    def test_only_exact_unprivileged_native_payload_is_extracted(self):
        target = self.root / 'bwrap'
        bwrap.extract(self.tar(), target, 'amd64')
        self.assertEqual(target.stat().st_mode & 0o777, 0o755)
        for options in [dict(name='../usr/bin/bwrap'), dict(mode=0o4755), dict(kind=tarfile.SYMTYPE),
                        dict(kind=tarfile.LNKTYPE), dict(machine=183), dict(duplicate=True)]:
            with self.subTest(options=options), self.assertRaises(RuntimeError):
                bwrap.extract(self.tar(**options), self.root / 'bad', 'amd64')
            self.assertFalse((self.root / 'bad').exists())
        with self.assertRaises(FileExistsError):
            bwrap.extract(self.tar(), target, 'amd64')

    def test_timeout_kills_descendant_group(self):
        marker = self.root / 'late'
        script = 'import os,time; pid=os.fork(); time.sleep(1); open(' + repr(str(marker)) + ',"w").write("bad")'
        with self.assertRaisesRegex(RuntimeError, 'bwrap_tool_timeout'):
            bwrap.run([sys.executable, '-c', script], root=self.root, timeout=.1)
        import time
        time.sleep(1.1)
        self.assertFalse(marker.exists())

    def test_global_budget_and_runtime_failures_keep_truthful_categories(self):
        with patch.object(bwrap, 'DEADLINE', bwrap.continuous_time() - 1):
            with self.assertRaisesRegex(RuntimeError, 'bwrap_tool_timeout'):
                bwrap.run([sys.executable, '-c', 'import time; time.sleep(10)'], root=self.root)
        for error, category in [(PermissionError(), 'bwrap_execution_denied'),
                (RuntimeError('bwrap_tool_failed'), 'bwrap_runtime_check_failed'),
                (RuntimeError('bwrap_tool_timeout'), 'bwrap_tool_timeout'),
                (RuntimeError('bwrap_tool_output_oversized'), 'bwrap_tool_output_oversized'),
                (RuntimeError('bwrap_metadata_oversized'), 'bwrap_metadata_oversized')]:
            with self.subTest(category=category), patch.object(bwrap, 'run', side_effect=error):
                with self.assertRaisesRegex(RuntimeError, '^' + category + '$'):
                    bwrap.runtime_version(self.root / 'bwrap', self.root)

    def test_caps_and_probe_actual_absolute_chain(self):
        with self.assertRaisesRegex(RuntimeError, 'bwrap_tool_output_oversized'):
            bwrap.run([sys.executable, '-c', 'print("x"*2000)'], root=self.root, limit=100)
        with patch.object(bwrap.subprocess, 'Popen') as spawn, \
             patch.object(bwrap, 'check_storage', side_effect=RuntimeError('bwrap_metadata_oversized')):
            spawn.return_value.poll.return_value = 0
            spawn.return_value.pid = 999999999
            with self.assertRaisesRegex(RuntimeError, 'bwrap_metadata_oversized'):
                bwrap.run(['unused'], root=self.root)
        with patch('marmish_transport_sandbox.validated_bwrap', return_value=Path('/private/bwrap')), \
             patch.object(bwrap, 'run', return_value=b'') as run:
            bwrap.probe()
            argv = run.call_args.args[0]
            self.assertEqual(argv[:4], ['/usr/bin/unshare', '--user', '--map-root-user', '--net'])
            self.assertEqual(argv.count('/private/bwrap'), 2)
            self.assertEqual(argv[-1], '/usr/bin/true')
        with patch('marmish_transport_sandbox.validated_bwrap', return_value=Path('/private/bwrap')), \
             patch.object(bwrap, 'run', side_effect=RuntimeError('bwrap_tool_failed')):
            with self.assertRaisesRegex(RuntimeError, 'userns_denied_private_bwrap'):
                bwrap.probe()
        for category in ('bwrap_tool_timeout', 'bwrap_tool_output_oversized', 'bwrap_metadata_oversized'):
            with patch('marmish_transport_sandbox.validated_bwrap', return_value=Path('/private/bwrap')), \
                 patch.object(bwrap, 'run', side_effect=RuntimeError(category)):
                with self.assertRaisesRegex(RuntimeError, '^' + category + '$'):
                    bwrap.probe()

    def test_environment_pair_forwarded_only_after_validation(self):
        with patch.dict(os.environ, {'MARMISH_BWRAP': '/private/bwrap', 'MARMISH_BWRAP_SHA256': 'a' * 64}), \
             patch('marmish_transport_sandbox.validated_bwrap') as validate:
            self.assertEqual(bwrap.environment(), {'MARMISH_BWRAP': '/private/bwrap', 'MARMISH_BWRAP_SHA256': 'a' * 64})
            validate.assert_called_once()
        with patch.dict(os.environ, {'GITHUB_TOKEN': 'secret'}):
            with self.assertRaisesRegex(RuntimeError, 'unexpected_api_token'):
                bwrap.bootstrap(self.root / 'state')

    def test_cleanup_refuses_unowned_prefix_and_preserves_unrelated_data(self):
        pointer, foreign = self.root / 'state', self.root / 'user-data'
        foreign.mkdir(mode=0o700)
        (foreign / 'keep').write_text('keep')
        pointer.write_text(str(foreign))
        pointer.chmod(0o600)
        with patch.dict(os.environ, {'RUNNER_TEMP': str(self.root)}):
            with self.assertRaisesRegex(RuntimeError, 'bwrap_cleanup_ownership'):
                bwrap.cleanup(pointer)
            self.assertEqual((foreign / 'keep').read_text(), 'keep')
            owned = self.root / 'marmish-bwrap-owned'
            owned.mkdir(mode=0o700)
            (owned / 'owned-state').write_text(str(pointer.absolute()))
            pointer.write_text(str(owned))
            bwrap.cleanup(pointer)
            self.assertFalse(owned.exists())
            self.assertTrue(foreign.exists())

    def test_orchestration_auth_floor_and_digest_failures_remove_only_owned_state(self):
        original_exists, original_read, original_file = Path.exists, Path.read_text, Path.is_file
        keyring = Mock()
        keyring.lstat.return_value = Mock(st_uid=0, st_mode=0o644)
        keyring.is_file.return_value, keyring.is_symlink.return_value = True, False
        for failure in ('auth', 'floor', 'floor_timeout', 'hash'):
            with self.subTest(failure=failure):
                pointer = self.root / ('state-' + failure)
                calls = []
                def fake_run(argv, **kwargs):
                    calls.append(argv)
                    if argv[1] == 'update':
                        if failure == 'auth':
                            raise RuntimeError('bwrap_tool_failed')
                        return b''
                    if argv[1] == 'policy':
                        return b'  Candidate: 0.9.0-1ubuntu0.3\n'
                    if argv[1] == '--compare-versions':
                        if failure == 'floor':
                            raise RuntimeError('bwrap_tool_failed')
                        if failure == 'floor_timeout':
                            raise RuntimeError('bwrap_tool_timeout')
                        return b''
                    if argv[1] == 'show':
                        return self.record().encode()
                    if argv[1] == 'download':
                        (kwargs['cwd'] / 'package.deb').write_bytes(b'x' * 512)
                        return b''
                    self.fail('unexpected operation')
                def read(path, *args, **kwargs):
                    return 'ID=ubuntu\nVERSION_CODENAME=noble\n' if str(path) == '/etc/os-release' else original_read(path, *args, **kwargs)
                with patch.dict(os.environ, {'RUNNER_TEMP': str(self.root)}, clear=True), \
                     patch.object(bwrap, 'KEYRING', keyring), patch.object(bwrap.os, 'geteuid', return_value=1000), \
                     patch.object(bwrap.platform, 'machine', return_value='x86_64'), \
                     patch.object(Path, 'read_text', read), \
                     patch.object(Path, 'exists', lambda p: str(p).startswith(('/usr/bin/', '/usr/sbin/')) or original_exists(p)), \
                     patch.object(Path, 'is_file', lambda p: str(p).startswith(('/usr/bin/', '/usr/sbin/')) or original_file(p)), \
                     patch.object(bwrap.os, 'access', return_value=True), \
                     patch.object(bwrap, 'run', side_effect=fake_run):
                    category = {'auth': 'bwrap_tool_failed', 'floor': 'bwrap_candidate_below_floor',
                                'floor_timeout': 'bwrap_tool_timeout', 'hash': 'bwrap_package_digest_mismatch'}[failure]
                    with self.assertRaisesRegex(RuntimeError, '^' + category + '$'):
                        bwrap.bootstrap(pointer)
                self.assertFalse(pointer.exists())
                self.assertEqual(list(self.root.glob('marmish-bwrap-*')), [])
                self.assertFalse(any('--fsys-tarfile' in command for command in calls))


if __name__ == '__main__':
    unittest.main()
