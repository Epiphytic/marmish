"""Authenticated package selection and private CI executable boundaries."""
import hashlib
import contextlib
import io
import json
import os
from pathlib import Path
import signal
import stat
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
        self.root = Path(self.directory.name).resolve()

    @contextlib.contextmanager
    def trusted_fixture(self):
        with patch.object(bwrap, 'TRUSTED_PARENT', self.root), \
                patch.object(bwrap, 'trusted_parent', side_effect=lambda: os.open(self.root, os.O_RDONLY | os.O_DIRECTORY)):
            yield

    def pointer(self, state, root, *, backlink=None):
        info = root.stat()
        state.write_text(json.dumps({'root': str(root), 'device': info.st_dev, 'inode': info.st_ino}))
        state.chmod(0o600)
        (root / 'owned-state').write_text(str(state.absolute()) if backlink is None else backlink)
        (root / 'owned-state').chmod(0o600)

    @contextlib.contextmanager
    def bootstrap_fixture(self, *, reject=False):
        original_read, original_file = Path.read_text, Path.is_file
        keyring = Mock()
        keyring.read_bytes.return_value = b'public test keyring'
        keyring.lstat.return_value = Mock(st_uid=0, st_mode=0o644)
        keyring.is_file.return_value, keyring.is_symlink.return_value = True, False
        def read(path, *args, **kwargs):
            return 'ID=ubuntu\nVERSION_CODENAME=noble\n' if str(path) == '/etc/os-release' else original_read(path, *args, **kwargs)
        def run(argv, **kwargs):
            if argv[1] == 'policy':
                return b' Candidate: 0.9.0-1ubuntu0.3\n'
            if argv[1] == 'show':
                return self.record(SHA256=hashlib.sha256(b'x' * 512).hexdigest()).encode()
            if argv[1] == 'download':
                (kwargs['cwd'] / 'package.deb').write_bytes(b'x' * 512)
            if argv[1] == '--fsys-tarfile':
                return self.tar()
            if argv[1] == '--version':
                return b'bubblewrap 0.9.0'
            return b''
        from marmish_transport_sandbox import SandboxError
        def validate():
            self.assertIn('MARMISH_BWRAP_SHA256', os.environ)
            self.assertEqual(Path(os.environ['MARMISH_BWRAP']).parent.parent.parent, self.root)
            if reject:
                raise SandboxError('fixture_rejection')
            return Path(os.environ['MARMISH_BWRAP'])
        with self.trusted_fixture(), patch.dict(os.environ, {'RUNNER_TEMP': str(self.root / 'untrusted-runner'),
                'TMPDIR': str(self.root / 'untrusted-tmpdir'), 'GITHUB_ENV': str(self.root / 'github.env')}, clear=True), \
                patch.object(bwrap, 'KEYRING', keyring), patch.object(bwrap.platform, 'machine', return_value='x86_64'), \
                patch.object(Path, 'read_text', read), \
                patch.object(Path, 'is_file', lambda p: str(p).startswith(('/usr/bin/', '/usr/sbin/')) or original_file(p)), \
                patch.object(bwrap.os, 'access', return_value=True), patch.object(bwrap, 'run', side_effect=run), \
                patch('marmish_transport_sandbox.validated_bwrap', side_effect=validate):
            yield

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
        self.pointer(pointer, foreign)
        with self.trusted_fixture():
            with self.assertRaisesRegex(RuntimeError, 'bwrap_cleanup_ownership'):
                bwrap.cleanup(pointer)
            self.assertEqual((foreign / 'keep').read_text(), 'keep')
            owned = self.root / ('marmish-bwrap-' + 'a' * 32)
            owned.mkdir(mode=0o700)
            self.pointer(pointer, owned)
            bwrap.cleanup(pointer)
            self.assertFalse(owned.exists())
            self.assertTrue(foreign.exists())

    def test_orchestration_auth_floor_and_digest_failures_remove_only_owned_state(self):
        original_exists, original_read, original_file = Path.exists, Path.read_text, Path.is_file
        keyring = Mock()
        keyring.read_bytes.return_value = b'public test keyring'
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
                with self.trusted_fixture(), patch.dict(os.environ, {'RUNNER_TEMP': str(self.root)}, clear=True), \
                     patch.object(bwrap, 'KEYRING', keyring), patch.object(bwrap.os, 'geteuid', return_value=os.geteuid() or 1000), \
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


    def test_trusted_parent_rejects_owner_mode_and_symlink(self):
        for uid, mode, accepted in ((0, stat.S_IFDIR | 0o1777, True),
                                   (0, stat.S_IFDIR | 0o755, True),
                                   (123, stat.S_IFDIR | 0o1777, False),
                                   (0, stat.S_IFDIR | 0o777, False)):
            with self.subTest(uid=uid, mode=mode), patch.object(bwrap, 'TRUSTED_PARENT', self.root), \
                    patch.object(bwrap.os, 'fstat', return_value=Mock(st_uid=uid, st_mode=mode)):
                if accepted:
                    os.close(bwrap.trusted_parent())
                else:
                    with self.assertRaisesRegex(RuntimeError, 'bwrap_untrusted_parent'):
                        bwrap.trusted_parent()
        alias = self.root / 'alias'
        alias.symlink_to(self.root, target_is_directory=True)
        with patch.object(bwrap, 'TRUSTED_PARENT', alias), self.assertRaises(OSError):
            bwrap.trusted_parent()

    def test_success_exports_only_validated_prefix_and_strips_work(self):
        state = self.root / 'state'
        with self.bootstrap_fixture(), contextlib.redirect_stdout(io.StringIO()):
            bwrap.bootstrap(state)
            value = json.loads(state.read_text())
            prefix = Path(value['root'])
            self.assertEqual(set(p.name for p in prefix.iterdir()), {'bin', 'provenance.json', 'owned-state'})
            self.assertIn('MARMISH_BWRAP=' + str(prefix / 'bin/bwrap'), (self.root / 'github.env').read_text())
            self.assertNotIn('MARMISH_BWRAP', os.environ, 'scoped validation leaked environment')
            bwrap.cleanup(state)
        self.assertFalse(prefix.exists())
        self.assertFalse(state.exists())

    def test_validation_rejection_exports_nothing_and_cleans_owned_prefix(self):
        with self.bootstrap_fixture(reject=True), contextlib.redirect_stdout(io.StringIO()) as output:
            with self.assertRaisesRegex(RuntimeError, 'fixture_rejection'):
                bwrap.bootstrap(self.root / 'state')
        self.assertFalse((self.root / 'github.env').exists())
        self.assertEqual(list(self.root.glob('marmish-bwrap-*')), [])
        self.assertNotIn(str(self.root), output.getvalue())
        self.assertIn('bwrap_validation', output.getvalue())

    def test_setup_failure_cleanup_preserves_original_error(self):
        for cleanup_failure in (False, True):
            with self.subTest(cleanup_failure=cleanup_failure), self.bootstrap_fixture(), \
                    patch.object(bwrap, 'write_private', side_effect=RuntimeError('original_setup_failure')):
                remover = bwrap.remove_prefix
                def remove(*args):
                    if cleanup_failure:
                        raise RuntimeError('cleanup_failure')
                    remover(*args)
                with patch.object(bwrap, 'remove_prefix', side_effect=remove):
                    with self.assertRaisesRegex(RuntimeError, '^original_setup_failure$'):
                        bwrap.bootstrap(self.root / 'state')
                self.assertFalse((self.root / 'github.env').exists())
                if not cleanup_failure:
                    self.assertEqual(list(self.root.glob('marmish-bwrap-*')), [])

    def test_partial_pointer_write_is_removed_without_masking_original_failure(self):
        state = self.root / 'state'
        with self.bootstrap_fixture(), patch.object(bwrap.os, 'fsync', side_effect=OSError('pointer_sync_failed')):
            with self.assertRaisesRegex(OSError, '^pointer_sync_failed$'):
                bwrap.bootstrap(state)
        self.assertFalse(state.exists())
        self.assertFalse((self.root / 'github.env').exists())
        self.assertEqual(list(self.root.glob('marmish-bwrap-*')), [])

    def test_pointer_symlink_parent_and_nonprivate_records_are_rejected(self):
        actual = self.root / 'actual'
        actual.mkdir(mode=0o700)
        alias = self.root / 'alias'
        alias.symlink_to(actual, target_is_directory=True)
        with self.assertRaises(OSError):
            bwrap.private_directory_fd(alias)
        fd = os.open(actual, os.O_RDONLY | os.O_DIRECTORY)
        try:
            for kind in ('mode', 'oversized', 'symlink', 'fifo', 'hardlink'):
                with self.subTest(kind=kind):
                    target = actual / kind
                    if kind == 'symlink':
                        target.symlink_to(actual / 'mode')
                    elif kind == 'fifo':
                        os.mkfifo(target, mode=0o600)
                    else:
                        target.write_bytes(b'x' * (4097 if kind == 'oversized' else 1))
                        target.chmod(0o644 if kind == 'mode' else 0o600)
                        if kind == 'hardlink':
                            os.link(target, actual / 'duplicate')
                    with self.assertRaises((RuntimeError, OSError)):
                        bwrap.read_private(fd, kind)
        finally:
            os.close(fd)

    def test_cleanup_rejects_replaced_prefix_and_wrong_backlink(self):
        for kind in ('backlink', 'symlink', 'mode', 'inode', 'owner', 'backlink_symlink'):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory(dir=self.root) as scope:
                scope = Path(scope)
                with patch.object(bwrap, 'TRUSTED_PARENT', scope), \
                        patch.object(bwrap, 'trusted_parent', side_effect=lambda: os.open(scope, os.O_RDONLY | os.O_DIRECTORY)):
                    prefix = scope / ('marmish-bwrap-' + 'b' * 32)
                    prefix.mkdir(mode=0o700)
                    state = scope / 'state'
                    self.pointer(state, prefix)
                    if kind == 'backlink':
                        (prefix / 'owned-state').write_text('different pointer')
                    elif kind == 'symlink':
                        prefix.rename(scope / 'original')
                        prefix.symlink_to(scope / 'original', target_is_directory=True)
                    elif kind == 'mode':
                        prefix.chmod(0o750)
                    elif kind == 'inode':
                        prefix.rename(scope / 'original')
                        prefix.mkdir(mode=0o700)
                        (prefix / 'owned-state').write_text(str(state))
                        (prefix / 'owned-state').chmod(0o600)
                    elif kind == 'backlink_symlink':
                        (prefix / 'owned-state').unlink()
                        (prefix / 'owned-state').symlink_to(state)
                    with contextlib.ExitStack() as stack:
                        if kind == 'owner':
                            stack.enter_context(patch.object(bwrap.os, 'fstat', return_value=Mock(st_uid=999999, st_mode=stat.S_IFDIR | 0o700)))
                        with self.assertRaises((RuntimeError, OSError)):
                            bwrap.cleanup(state)
                    self.assertTrue(os.path.lexists(prefix))
                    self.assertTrue(state.exists())

    def test_diagnostic_reports_first_failure_without_private_path(self):
        # Synthetic descriptor walk avoids trusting the host TMPDIR ancestors.
        for unsafe in (True, False):
            with self.subTest(unsafe=unsafe), patch.dict(os.environ, {'MARMISH_BWRAP': '/safe/private/bwrap'}, clear=True), \
                    patch.object(bwrap.os, 'open', side_effect=[100, 101, 102, FileNotFoundError(2, 'private host detail')]), \
                    patch.object(bwrap.os, 'close'), patch.object(bwrap.os, 'fstat', side_effect=[
                        Mock(st_uid=0, st_mode=stat.S_IFDIR | 0o755),
                        Mock(st_uid=0, st_mode=stat.S_IFDIR | 0o755),
                        Mock(st_uid=0, st_mode=stat.S_IFDIR | (0o777 if unsafe else 0o755))]):
                row = bwrap.validation_diagnostic()
            self.assertEqual(row['category'], 'ancestor_mode' if unsafe else 'executable_io')
            self.assertEqual(set(row), {'category', 'index', 'uid', 'mode', 'errno'})
            self.assertNotIn('private', json.dumps(row))
            self.assertEqual(row['index'], 2 if unsafe else 3)
            self.assertEqual(row['errno'], None if unsafe else 2)

    def test_tmp_filesystem_reports_only_bounded_public_mount_facts(self):
        body = (b'1 0 0:1 / / rw,relatime shared:1 - ext4 /private/source rw,secret=hidden\n'
                b'2 1 0:2 / /tmp rw,nosuid,nodev,noexec - tmpfs private-source rw,size=secret\n'
                b'3 1 0:3 / /tmp/child ro - tmpfs ignored ro\n')
        for platform_name, value, expected in (
                ('Linux', body, {'fstype': 'tmpfs', 'flags': ['nodev', 'noexec', 'nosuid', 'rw']}),
                ('Linux', body + b'4 1 0:4 / /tmp ro,nosuid - overlay private-source rw,secret=hidden\n',
                    {'fstype': 'overlay', 'flags': ['nosuid', 'ro']}),
                ('Darwin', body, {'fstype': 'unknown', 'flags': []}),
                ('Linux', b'x' * (256 * 1024 + 1), {'fstype': 'unknown', 'flags': []}),
                ('Linux', b'malformed', {'fstype': 'unknown', 'flags': []}),
                ('Linux', b'1 0 0:1 / / rw - unsafe:type source rw', {'fstype': 'unknown', 'flags': []})):
            with self.subTest(platform=platform_name, size=len(value)), \
                    patch.object(bwrap.platform, 'system', return_value=platform_name), \
                    patch('builtins.open', return_value=io.BytesIO(value)) as opened:
                self.assertEqual(bwrap.tmp_filesystem(), expected)
                if platform_name == 'Darwin':
                    opened.assert_not_called()
        with patch.object(bwrap.platform, 'system', return_value='Linux'), \
                patch('builtins.open', side_effect=OSError('private host detail')):
            self.assertEqual(bwrap.tmp_filesystem(), {'fstype': 'unknown', 'flags': []})

    @unittest.skipUnless(sys.platform.startswith('linux'), 'literal Linux /tmp production contract')
    def test_real_tmp_prefix_passes_production_validation_and_cleans(self):
        parent = bwrap.trusted_parent()
        name = 'marmish-bwrap-' + os.urandom(16).hex()
        identity = None
        try:
            os.mkdir(name, 0o700, dir_fd=parent)
            root = bwrap.TRUSTED_PARENT / name
            root.joinpath('bwrap').write_bytes(b'offline executable fixture')
            root.joinpath('bwrap').chmod(0o700)
            info = root.stat()
            identity = (info.st_dev, info.st_ino)
            with bwrap.selected_environment(root / 'bwrap', bwrap.digest(root / 'bwrap')):
                self.assertEqual(bwrap.checked_selection(), root / 'bwrap')
        finally:
            if identity is not None:
                bwrap.remove_prefix(parent, name, identity)
            os.close(parent)


if __name__ == '__main__':
    unittest.main()
