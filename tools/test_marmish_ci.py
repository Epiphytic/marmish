"""Digest bootstrap, owned destinations and trusted artifact boundary tests."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch

import marmish_ci as ci
import marmish_installed_bootstrap as owned

CONTEXT = {'run': '123', 'attempt': '1', 'sha': 'a' * 40}


class CiTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.destination = self.root / 'output'
        self.addCleanup(patch.stopall)
        patch.object(ci, 'context', return_value=CONTEXT).start()
        patch.object(ci, 'tool_versions', return_value={'python': '3.13', 'rustc': '1.97.1', 'cc': 'clang'}).start()

    def files(self):
        files = {name: b'native-executable' for name in ('wnd', 'fixture', 'marmish-worker',
                  'marmish-engine-bridge', 'wnd.manifest.json')}
        if ci.sys.platform == 'darwin':
            files.update({'marmish-probe': b'probe', 'marmish-lifetime': b'lifetime'})
        return files

    def archive(self, files=None, record=None, extra=None):
        files = self.files() if files is None else files
        if record is None:
            record = ci.bundle_record({name: hashlib.sha256(data).hexdigest() for name, data in files.items()})
        path = self.root / 'bundle.tar'
        with tarfile.open(path, 'w') as archive:
            for name, data in [*files.items(), ('bundle.json', json.dumps(record).encode())]:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = 0o755
                archive.addfile(info, io.BytesIO(data))
            if extra:
                info, data = extra
                archive.addfile(info, io.BytesIO(data))
        return path

    def test_bundle_build_uses_unique_engine_source_and_legacy_destination(self):
        artifact = self.root / 'target/release'
        artifact.mkdir(parents=True)
        (artifact / 'marmish-engine-worker').write_bytes(b'engine-worker')
        (artifact / 'marmish-worker').write_bytes(b'wrong-native-worker')
        (artifact / 'marmish-engine-bridge').write_bytes(b'bridge')
        self.destination.mkdir()
        with patch.object(ci, 'REPO', self.root), patch.object(ci, 'checked') as checked:
            ci._build_bundle(self.destination)
        self.assertEqual((self.destination / 'marmish-worker').read_bytes(), b'engine-worker')
        self.assertFalse((self.destination / 'marmish-engine-worker').exists())
        self.assertEqual((self.destination / 'marmish-worker').stat().st_mode & 0o777, 0o755)
        self.assertEqual(checked.call_args_list[0].args[0],
                         ['cargo', 'build', '--locked', '--release', '-p',
                          'marmish-engine', '-p', 'marmish-prototype'])
        record = json.loads((self.destination / 'bundle.json').read_text())
        self.assertEqual(record['hashes']['marmish-worker'],
                         hashlib.sha256(b'engine-worker').hexdigest())

    def test_unpack_preserves_executable_modes_and_context(self):
        ci.unpack(self.archive(), self.destination)
        self.assertEqual((self.destination / 'wnd').stat().st_mode & 0o777, 0o755)
        self.assertEqual(json.loads((self.destination / 'bundle.json').read_text())['context'], CONTEXT)

    def test_preexisting_destination_never_deleted(self):
        self.destination.mkdir()
        marker = self.destination / 'user-data'
        marker.write_text('keep')
        for operation in (lambda: ci.unpack(self.archive(), self.destination),
                          lambda: ci.bundle(self.destination)):
            with self.assertRaisesRegex(RuntimeError, 'native_bundle_destination_exists'):
                operation()
            self.assertEqual(marker.read_text(), 'keep')

    def test_tar_rejection_removes_only_new_partial_directory(self):
        for name, kind in [('../escape', tarfile.REGTYPE), ('wnd', tarfile.REGTYPE),
                           ('unknown', tarfile.REGTYPE), ('link', tarfile.SYMTYPE)]:
            with self.subTest(name=name):
                info = tarfile.TarInfo(name)
                info.type, info.linkname = kind, 'wnd'
                with self.assertRaisesRegex(RuntimeError, 'invalid_native_bundle'):
                    ci.unpack(self.archive(extra=(info, b'')), self.destination)
                self.assertFalse(self.destination.exists())

    def test_manifest_mismatch_missing_files_and_wrong_attempt(self):
        good = ci.bundle_record({name: hashlib.sha256(data).hexdigest() for name, data in self.files().items()})
        bad_records = [dict(good, context=dict(CONTEXT, attempt='2')),
                       dict(good, hashes=dict(good['hashes'], wnd='0' * 64)),
                       dict(good, hashes=dict(good['hashes'], wnd='not-a-digest'))]
        for record in bad_records:
            with self.subTest(record=record), self.assertRaises(RuntimeError):
                ci.unpack(self.archive(record=record), self.destination)
            self.assertFalse(self.destination.exists())
        path = self.root / 'empty.tar'
        with tarfile.open(path, 'w'):
            pass
        with self.assertRaisesRegex(RuntimeError, 'invalid_native_bundle'):
            ci.unpack(path, self.destination)
        self.assertFalse(self.destination.exists())

    def test_bootstrap_capability_and_origin_and_digest(self):
        with patch.object(tarfile, 'data_filter', create=False) as capability:
            del tarfile.data_filter
            with self.assertRaisesRegex(RuntimeError, 'python_capability_required'):
                ci.capabilities()
        with self.assertRaisesRegex(RuntimeError, 'invalid_bootstrap_origin'):
            ci.download('https://evil.example/rust', '0' * 64)
        response = Mock()
        response.__enter__ = Mock(return_value=response)
        response.__exit__ = Mock(return_value=None)
        response.read.return_value = b'wrong'
        with patch.object(ci.urllib.request, 'urlopen', return_value=response):
            with self.assertRaisesRegex(RuntimeError, 'bootstrap_digest_mismatch'):
                ci.download('https://static.rust-lang.org/rust', '0' * 64)

    def test_tokens_refused_but_actions_services_not_passed_to_builds(self):
        for name in ('GITHUB_TOKEN', 'GH_TOKEN', 'GITHUB_ENTERPRISE_TOKEN',
                     'GH_ENTERPRISE_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN'):
            with patch.dict(os.environ, {name: 'credential'}), patch.object(ci, 'checked') as run:
                with self.assertRaisesRegex(RuntimeError, 'unexpected_api_token'):
                    ci.bundle(self.destination)
                run.assert_not_called()
        with patch.dict(os.environ, {'ACTIONS_RUNTIME_TOKEN': 'artifact-only', 'PATH': '/bin'}):
            self.assertNotIn('ACTIONS_RUNTIME_TOKEN', ci.build_environment())

    def test_managed_stale_toolchain_reinstalls_atomically(self):
        self.destination.mkdir()
        (self.destination / '.marmish-toolchain.json').write_text(json.dumps({'managed': True, 'rust': '1.96'}))
        (self.destination / 'old').write_text('old-version')
        target = {('Linux', 'x86_64'): 'x86_64-unknown-linux-gnu',
                  ('Linux', 'aarch64'): 'aarch64-unknown-linux-gnu',
                  ('Darwin', 'arm64'): 'aarch64-apple-darwin'}[(ci.platform.system(), ci.platform.machine())]
        manifest = '[pkg.rust.target.' + target + ']\nxz_url="https://static.rust-lang.org/archive"\nxz_hash="digest"\n'
        payload = io.BytesIO()
        with tarfile.open(fileobj=payload, mode='w:xz') as archive:
            metadata = ['install.sh', 'components', 'rust-installer-version']
            metadata.extend(component + '/manifest.in' for component in ci._bootstrap_components(target))
            for name in metadata:
                info = tarfile.TarInfo('rust-' + ci.RUST + '-' + target + '/' + name)
                archive.addfile(info, io.BytesIO())
        def install(command):
            prefix = Path(next(value.split('=', 1)[1] for value in command if value.startswith('--prefix=')))
            (prefix / 'bin').mkdir(parents=True)
            for name in ('rustc', 'cargo', 'cargo-clippy', 'rustfmt'):
                (prefix / 'bin' / name).write_text('native-' + name)
        with patch.object(ci, 'download', side_effect=[manifest.encode(), payload.getvalue()]), \
             patch.object(ci, 'checked', side_effect=install), \
             patch.object(ci.subprocess, 'check_output', return_value='rustc 1.97.1 (test)'):
            ci.bootstrap(self.destination)
        self.assertFalse((self.destination / 'old').exists())
        self.assertTrue(ci.valid_toolchain(self.destination))
        with patch.object(ci, 'download') as download:
            ci.bootstrap(self.destination)
            download.assert_not_called()
        (self.destination / 'bin/cargo').write_text('corrupted')
        self.assertFalse(ci.valid_toolchain(self.destination))


class ShellBootstrapTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.environment = dict(os.environ, PATH=str(self.bin) + ':/usr/bin:/bin',
                                RUNNER_TEMP=str(self.root), GITHUB_PATH=str(self.root / 'paths'))

    def script(self, name, body):
        path = self.bin / name
        path.write_text('#!/bin/sh\n' + body + '\n')
        path.chmod(0o755)

    def run_shell(self):
        return subprocess.run(['sh', str(Path(__file__).with_name('bootstrap_ci_python.sh'))],
                              env=self.environment, capture_output=True)

    def test_repeated_mac_attempts_use_distinct_owned_prefixes(self):
        self.script('uname', 'if [ "$1" = -s ]; then echo Darwin; else echo arm64; fi')
        self.script('curl', 'while [ "$1" != --output ]; do shift; done; touch "$2"')
        self.script('shasum', 'echo 9c985cbf327425873469896ce748c7134b6deca8edb81e93c589ad2fc5aff445 archive')
        self.script('tar', 'mkdir -p "$4/python/bin"; printf "#!/bin/sh\\nexit 0\\n" > "$4/python/bin/python3"; chmod +x "$4/python/bin/python3"')
        self.assertEqual(self.run_shell().returncode, 0)
        self.assertEqual(self.run_shell().returncode, 0)
        paths = (self.root / 'paths').read_text().splitlines()
        self.assertEqual(len(set(paths)), 2)
        self.assertFalse(list(self.root.glob('marmish-python.*/bootstrap.tar.gz')))
        self.script('shasum', 'echo invalid archive')
        self.assertNotEqual(self.run_shell().returncode, 0)
        self.assertEqual(len(list(self.root.glob('marmish-python.*'))), 2)

    def test_rosetta_and_missing_python_capability_fail_clearly(self):
        self.script('uname', 'if [ "$1" = -s ]; then echo Darwin; else echo x86_64; fi')
        result = self.run_shell()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'macos_arm64_required', result.stderr)
        self.script('uname', 'echo Linux')
        self.script('python3', 'echo python_capability_required >&2; exit 1')
        result = self.run_shell()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b'python_capability_required', result.stderr)


if __name__ == '__main__':
    unittest.main()


class BootstrapDisk(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.target = 'aarch64-apple-darwin'

    def archive(self, extra=None, omit=None):
        root = 'rust-' + ci.RUST + '-' + self.target
        files = {'install.sh': b'installer', 'components': b'components',
                 'rust-installer-version': b'3'}
        files.update({name + '/manifest.in': b'file:payload' for name in ci._bootstrap_components(self.target)})
        files.update({name + '/payload': b'payload' for name in ci._bootstrap_components(self.target)})
        files['rust-docs/not-selected'] = b'D' * 3000
        files['rustc-dev/not-selected'] = b'D' * 3000
        files['rust-std-' + self.target + 'x/not-selected'] = b'D' * 3000
        if omit:
            files.pop(omit)
        result = io.BytesIO()
        with tarfile.open(fileobj=result, mode='w:xz') as archive:
            for name, raw in files.items():
                info = tarfile.TarInfo(root + '/' + name)
                info.size = len(raw)
                archive.addfile(info, io.BytesIO(raw))
            if extra:
                info = tarfile.TarInfo(extra[0])
                info.type = extra[1]
                info.linkname = root + '/cargo/payload'
                archive.addfile(info)
        return result.getvalue()

    def test_selective_stream_extraction_skips_unrequested_components(self):
        with patch.object(ci, '_bootstrap_space') as space:
            ci._extract_bootstrap(self.archive(), self.target, self.root)
        root = self.root / ('rust-' + ci.RUST + '-' + self.target)
        self.assertTrue((root / 'rustc/payload').is_file())
        self.assertFalse((root / 'rust-docs').exists())
        self.assertFalse((root / 'rustc-dev').exists())
        self.assertFalse((root / ('rust-std-' + self.target + 'x')).exists())
        self.assertEqual(space.call_count, 2)
        self.assertFalse((self.root / 'rust.tar.xz').exists())

    def test_bad_names_unselected_traversal_links_and_missing_metadata_refused(self):
        root = 'rust-' + ci.RUST + '-' + self.target
        cases = [(root + '/rust-docs/../outside', tarfile.REGTYPE),
                 ('/absolute', tarfile.REGTYPE), (root + '/cargo/link', tarfile.SYMTYPE),
                 (root + '/rustc/link', tarfile.LNKTYPE), (root + '/rustc//bad', tarfile.REGTYPE)]
        for extra in cases:
            with self.subTest(extra=extra), self.assertRaisesRegex(RuntimeError, 'invalid_bootstrap_archive'):
                ci._bootstrap_inventory(self.archive(extra=extra), self.target, 4096)
        with self.assertRaisesRegex(RuntimeError, 'invalid_bootstrap_archive'):
            ci._bootstrap_inventory(self.archive(omit='rustc/manifest.in'), self.target, 4096)

    def test_budget_uses_available_blocks_and_emits_only_finite_counts(self):
        from types import SimpleNamespace
        info = SimpleNamespace(f_bavail=100, f_frsize=4096, f_blocks=200)
        for required, passes in ((409600, True), (409601, False)):
            with patch.object(ci.os, 'statvfs', return_value=info), \
                    patch.object(ci.sys, 'stderr', new_callable=io.StringIO) as stderr:
                if passes:
                    ci._bootstrap_space(self.root, required, 8192, 'extraction')
                else:
                    with self.assertRaisesRegex(RuntimeError, 'insufficient_bootstrap_space'):
                        ci._bootstrap_space(self.root, required, 8192, 'extraction')
                record = json.loads(stderr.getvalue())
            self.assertEqual(set(record), {'kind', 'stage', 'free_bytes', 'total_bytes',
                                          'required_bytes', 'selected_allocated_bytes'})
            self.assertNotIn(str(self.root), json.dumps(record))
        self.assertEqual(ci._bootstrap_inventory(self.archive(), self.target, 4096), 13 * 4096)


class OwnedBootstrap(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.harness = Path(self.temporary.name).resolve()
        self.harness.chmod(0o700)
        self.destination = self.harness / 'bootstrap/toolchain'
        self.marker = self.harness / 'bootstrap-owner.json'

    def create(self):
        with patch.object(owned, 'bootstrap', side_effect=lambda path: path.mkdir(mode=0o700)):
            owned.bootstrap_owned(self.destination, CONTEXT)

    def test_current_owned_tree_removed_and_repeated_cleanup_is_absent(self):
        self.create()
        self.assertEqual(self.destination.parent.stat().st_mode & 0o777, 0o700)
        self.assertEqual(self.marker.stat().st_mode & 0o777, 0o600)
        self.assertEqual(owned.cleanup_bootstrap(self.destination, CONTEXT)['outcome'], 'complete')
        self.assertFalse(self.marker.exists())
        self.assertFalse(self.destination.parent.exists())
        self.assertEqual(owned.cleanup_bootstrap(self.destination, CONTEXT)['outcome'], 'absent')

    @unittest.skipUnless(ci.sys.platform == 'darwin', 'Darwin /tmp alias')
    def test_darwin_tmp_alias_uses_one_canonical_record(self):
        with tempfile.TemporaryDirectory(prefix='marmish-bootstrap-alias-', dir='/tmp') as temporary:
            alias = Path(temporary) / 'bootstrap/toolchain'
            with patch.object(owned, 'bootstrap', side_effect=lambda path: path.mkdir(mode=0o700)):
                owned.bootstrap_owned(alias, CONTEXT)
            marker = Path(temporary) / 'bootstrap-owner.json'
            self.assertEqual(json.loads(marker.read_bytes())['destination'], str(alias.resolve()))
            self.assertEqual(owned.cleanup_bootstrap(alias, CONTEXT)['outcome'], 'complete')

    def test_record_present_directory_absent_is_verified_and_removed(self):
        import shutil
        self.create()
        shutil.rmtree(self.destination.parent)
        self.assertEqual(owned.cleanup_bootstrap(self.destination, CONTEXT)['outcome'], 'already-removed')
        self.assertFalse(self.marker.exists())

    def test_wrong_context_identity_path_and_symlinks_refused(self):
        self.create()
        original = json.loads(self.marker.read_bytes())
        for record in (dict(original, context=dict(CONTEXT, attempt='2')),
                       dict(original, destination=str(self.destination.parent)),
                       dict(original, root_identity=dict(original['root_identity'], ino=0))):
            self.marker.write_text(json.dumps(record))
            with self.subTest(record=record), self.assertRaises(Exception):
                owned.cleanup_bootstrap(self.destination, CONTEXT)
            self.assertTrue(self.destination.exists())
        self.marker.write_text(json.dumps(original))
        alias = self.harness / 'bootstrap-alias'
        alias.symlink_to(self.destination.parent)
        with self.assertRaises(Exception):
            owned.cleanup_bootstrap(alias / 'toolchain', CONTEXT)
        alias.unlink()
        self.marker.unlink()
        self.marker.symlink_to(self.destination / 'unowned')
        with self.assertRaises(Exception):
            owned.cleanup_bootstrap(self.destination, CONTEXT)

    def test_existing_unowned_or_unsafe_parent_never_deleted(self):
        self.destination.parent.mkdir()
        (self.destination.parent / 'keep').write_text('keep')
        for action in (lambda: owned.bootstrap_owned(self.destination, CONTEXT),
                       lambda: owned.cleanup_bootstrap(self.destination, CONTEXT)):
            with self.assertRaises(Exception):
                action()
        self.assertEqual((self.destination.parent / 'keep').read_text(), 'keep')
        self.harness.chmod(0o750)
        with self.assertRaises(Exception):
            owned.cleanup_bootstrap(self.destination, CONTEXT)

    def test_failed_install_removes_only_current_owned_partial_tree(self):
        with patch.object(owned, 'bootstrap', side_effect=RuntimeError('install failed')):
            with self.assertRaisesRegex(RuntimeError, 'install failed'):
                owned.bootstrap_owned(self.destination, CONTEXT)
        self.assertFalse(self.destination.parent.exists())
        self.assertFalse(self.marker.exists())
