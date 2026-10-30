"""Offline adversarial tests for the self-contained distribution installer."""
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import types
import unittest
from unittest import mock

INSTALLER = Path(__file__).resolve().parents[1] / 'scripts' / 'install.sh'
VERSION = '0.1.0-experimental.1'
TARGET = 'x86_64-unknown-linux-gnu'


def load_installer():
    text = INSTALLER.read_text()
    source = text.split('# BEGIN MARMISH INSTALLER PYTHON\n', 1)[1].split('# END MARMISH INSTALLER PYTHON', 1)[0]
    module = types.ModuleType('marmish_test_installer')
    exec(compile(source, str(INSTALLER), 'exec'), module.__dict__)
    return module


def fixture_files(installer, target=TARGET):
    fixed = installer.FIXED_FILES | (installer.DARWIN_EXECUTABLES if target.endswith('darwin') else set())
    files = {name: ('fixture ' + name + '\n').encode() for name in fixed}
    files['licenses/dependencies.json'] = b'{}\n'
    files['licenses/example/LICENSE'] = b'Test fixture license\n'
    # Both package and installer launchers use argv; nothing evaluates the prefix.
    for name in ('marmish', 'marmishd'):
        files['bin/' + name] = b'#!/bin/sh\nexec python3 -I -B -c \'import json,sys;print(json.dumps(sys.argv[1:]))\' "$@"\n'
    return files


def archive_bytes(installer, files=None, version=VERSION, target=TARGET, change_manifest=None, extras=(), archive_format=tarfile.USTAR_FORMAT):
    files = fixture_files(installer, target) if files is None else dict(files)
    manifest = {'format': 1, 'version': version, 'target': target, 'source_commit': 'a' * 40, 'experimental': True, 'limitations': ['Experimental fixture: no runtime validation.'], 'prerequisites': {'python': '3.11', 'glibc': '2.17' if target.endswith('linux-gnu') else None, 'shared_libraries': []}, 'files': {name: {'sha256': hashlib.sha256(data).hexdigest(), 'mode': installer.payload_mode(name, target)} for name, data in files.items()}}
    if change_manifest:
        change_manifest(manifest)
    files['release.json'] = json.dumps(manifest).encode()
    root = 'marmish-' + version + '-' + target
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w', format=archive_format) as tar:
        for name, data in sorted(files.items()):
            info = tarfile.TarInfo(root + '/' + name)
            info.mode = installer.payload_mode(name, target)
            info.size = len(data)
            tar.addfile(info, io.BytesIO(data))
        for info, data in extras:
            tar.addfile(info, io.BytesIO(data))
    return gzip.compress(output.getvalue(), mtime=0)


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.installer = load_installer()
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name).resolve()
        self.prefix = self.base / 'install with spaces \' and $dollar;$(touch NEVER)'
        self.launcher_dir = self.prefix / 'bin'
        self.archive = self.base / 'release.tar.gz'
        self.archive.write_bytes(archive_bytes(self.installer))
        self.uid = mock.patch.object(self.installer.os, 'geteuid', return_value=os.getuid())
        self.uid.start()
        self.libc = mock.patch.object(self.installer.platform, 'libc_ver', return_value=('glibc', '2.39'))
        self.libc.start()

    def tearDown(self):
        self.libc.stop()
        self.uid.stop()
        self.temporary.cleanup()

    def install(self, **kwargs):
        return self.installer.install_archive(self.archive, self.prefix, self.launcher_dir, kwargs.get('version', VERSION), kwargs.get('target', TARGET))

    def assert_rejected(self, data, message=None):
        self.archive.write_bytes(data)
        with self.assertRaises(self.installer.InstallError, msg=message):
            self.install()
        self.assertFalse((self.prefix / 'current').exists())
        self.assertFalse((self.launcher_dir / 'marmish').exists())

    def test_help_is_self_contained_and_ignores_pythonpath_and_cwd(self):
        decoys = self.base / 'decoys'
        decoys.mkdir()
        (decoys / 'argparse.py').write_text('raise RuntimeError("CWD imported")')
        (decoys / 'marmish_keys.py').write_text('raise RuntimeError("provider imported")')
        environment = dict(os.environ, PYTHONPATH=str(decoys), PYTHONSTARTUP=str(decoys / 'argparse.py'))
        result = subprocess.run(['sh', str(INSTALLER), '--help'], cwd=decoys, env=environment, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('--uninstall', result.stdout)
        self.assertFalse(self.prefix.exists())

    def test_install_special_characters_argv_idempotence_and_uninstall(self):
        manifest = self.install()
        self.assertTrue(manifest['experimental'])
        root = self.prefix / os.readlink(self.prefix / 'current')
        first = (self.launcher_dir / 'marmish').read_bytes()
        args = ['--help', "quote'and\"", '$HOME;$(false)', 'line\nsecond', '*', '--', '']
        output = subprocess.check_output([str(self.launcher_dir / 'marmish'), *args], text=True)
        self.assertEqual(json.loads(output), args)
        self.assertEqual(self.prefix.stat().st_mode & 0o777, 0o700)
        self.assertEqual(root.stat().st_mode & 0o777, 0o755)
        self.assertEqual((root / 'tools/marmish.py').stat().st_mode & 0o777, 0o644)
        self.install()
        self.assertEqual((self.launcher_dir / 'marmish').read_bytes(), first)
        self.installer.uninstall(self.prefix, self.launcher_dir)
        self.assertTrue(root.is_dir())
        self.assertFalse((self.prefix / 'current').is_symlink())
        self.assertFalse((self.launcher_dir / 'marmish').exists())

    def test_retains_old_version_and_immutable_launcher_path(self):
        self.install()
        first_root = self.prefix / os.readlink(self.prefix / 'current')
        version = '0.1.0-experimental.2'
        self.archive.write_bytes(archive_bytes(self.installer, version=version))
        self.install(version=version)
        self.assertTrue(first_root.exists())
        self.assertNotIn('/current/', (self.launcher_dir / 'marmish').read_text())
        self.assertIn(version, (self.launcher_dir / 'marmish').read_text())

    def test_checksum_missing_duplicate_corrupt(self):
        name = 'marmish-' + VERSION + '-' + TARGET + '.tar.gz'
        digest = hashlib.sha256(self.archive.read_bytes()).hexdigest()
        sums = self.base / 'SHA256SUMS'
        good = digest + '  ' + name + '\n'
        sums.write_text(good)
        self.installer.verify_checksum(self.archive, sums, name)
        for bad in [good + good, '0' * 64 + '  ' + name + '\n', digest + '  other.tar.gz\n', digest + '  ../bad\n']:
            sums.write_text(bad)
            with self.assertRaises(self.installer.InstallError):
                self.installer.verify_checksum(self.archive, sums, name)

    def test_version_target_source_and_schema_mismatches(self):
        mutations = [lambda m: m.update(version='0.1.0-experimental.2'), lambda m: m.update(target='aarch64-apple-darwin'), lambda m: m.update(source_commit='../bad'), lambda m: m.update(experimental=False), lambda m: m.update(format=True), lambda m: m.update(extra='surprise'), lambda m: m['files'].pop('LICENSE'), lambda m: m['files']['LICENSE'].update(mode=0o755), lambda m: m['files']['LICENSE'].update(sha256='0' * 64)]
        for mutation in mutations:
            with self.subTest(mutation=mutation):
                self.assert_rejected(archive_bytes(self.installer, change_manifest=mutation))

    def test_incomplete_payload_and_unexpected_files(self):
        files = fixture_files(self.installer)
        files.pop('bin/marmishd')
        self.assert_rejected(archive_bytes(self.installer, files=files))
        files = fixture_files(self.installer)
        files['tools/sitecustomize.py'] = b'print("bad")'
        self.assert_rejected(archive_bytes(self.installer, files=files))

    def test_tar_link_traversal_absolute_modes_duplicates_and_extensions(self):
        root = 'marmish-' + VERSION + '-' + TARGET
        cases = [('../outside', tarfile.REGTYPE, 0o644), ('/absolute', tarfile.REGTYPE, 0o644), (root + '/licenses/../escape', tarfile.REGTYPE, 0o644), (root + '/licenses//bad', tarfile.REGTYPE, 0o644), (root + '/licenses/link', tarfile.SYMTYPE, 0o644), (root + '/licenses/hard', tarfile.LNKTYPE, 0o644), (root + '/licenses/pipe', tarfile.FIFOTYPE, 0o644), (root + '/licenses/setuid', tarfile.REGTYPE, 0o4755), (root + '/LICENSE', tarfile.REGTYPE, 0o644), (root + '/empty-unexpected', tarfile.DIRTYPE, 0o755), (root + '/licenses/pax', tarfile.XHDTYPE, 0o644)]
        for name, kind, mode in cases:
            with self.subTest(name=name, kind=kind):
                info = tarfile.TarInfo(name)
                info.type, info.mode = kind, mode
                if kind in (tarfile.SYMTYPE, tarfile.LNKTYPE):
                    info.linkname = 'LICENSE'
                self.assert_rejected(archive_bytes(self.installer, extras=[(info, b'')]))
        files = fixture_files(self.installer)
        files['licenses/' + 'long' * 70] = b'extension'
        self.assert_rejected(archive_bytes(self.installer, files=files, archive_format=tarfile.PAX_FORMAT))

    def test_truncation_trailing_tar_gzip_and_bad_header_checksums(self):
        good = archive_bytes(self.installer)
        tar = gzip.decompress(good)
        bad_header = bytearray(tar)
        bad_header[0] ^= 1
        cases = [good[:-5], good + b'nonzero', good + gzip.compress(b'next member'), gzip.compress(tar + b'nonzero'), gzip.compress(bytes(bad_header)), gzip.compress(tar[:512]), gzip.compress(tar[:10])]
        for data in cases:
            with self.subTest(size=len(data)):
                self.assert_rejected(data)

    def test_size_and_member_limits(self):
        with mock.patch.object(self.installer, 'MAX_EXPANDED', 1024):
            self.assert_rejected(archive_bytes(self.installer))
        with mock.patch.object(self.installer, 'MAX_FILES', 2):
            self.assert_rejected(archive_bytes(self.installer))
        with mock.patch.object(self.installer, 'MAX_DOWNLOAD', 2):
            self.assert_rejected(archive_bytes(self.installer))

    def test_rejects_symlink_prefix_components_current_and_launchers(self):
        real = self.base / 'real'
        real.mkdir()
        self.prefix.symlink_to(real, target_is_directory=True)
        with self.assertRaises(self.installer.InstallError):
            self.install()
        self.prefix.unlink()
        self.prefix.mkdir(mode=0o700)
        (self.prefix / 'current').symlink_to('missing')
        with self.assertRaises(self.installer.InstallError):
            self.install()
        (self.prefix / 'current').unlink()
        self.launcher_dir.mkdir(exist_ok=True)
        (self.launcher_dir / 'marmish').symlink_to('missing')
        with self.assertRaises(self.installer.InstallError):
            self.install()

    def test_unrelated_launchers_and_broad_prefix_permissions_refused(self):
        self.prefix.mkdir(mode=0o755)
        with self.assertRaises(self.installer.InstallError):
            self.install()
        self.prefix.chmod(0o700)
        self.launcher_dir.mkdir()
        launcher = self.launcher_dir / 'marmish'
        launcher.write_bytes(b'unrelated')
        with self.assertRaises(self.installer.InstallError):
            self.install()
        self.assertEqual(launcher.read_bytes(), b'unrelated')

    def test_existing_version_collision_does_not_replace_working_install(self):
        self.install()
        current = os.readlink(self.prefix / 'current')
        launcher = (self.launcher_dir / 'marmish').read_bytes()
        files = fixture_files(self.installer)
        files['LICENSE'] = b'different'
        self.archive.write_bytes(archive_bytes(self.installer, files=files))
        with self.assertRaises(self.installer.InstallError):
            self.install()
        self.assertEqual(os.readlink(self.prefix / 'current'), current)
        self.assertEqual((self.launcher_dir / 'marmish').read_bytes(), launcher)

    def test_selector_failure_rolls_back_both_launchers(self):
        self.install()
        old_root = os.readlink(self.prefix / 'current')
        old_files = {name: (self.launcher_dir / name).read_bytes() for name in ('marmish', 'marmishd')}
        version = '0.1.0-experimental.2'
        self.archive.write_bytes(archive_bytes(self.installer, version=version))
        real = self.installer.replace_selector
        calls = 0
        def fail_once(*args):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise OSError('injected selector failure')
            return real(*args)
        with mock.patch.object(self.installer, 'replace_selector', side_effect=fail_once):
            with self.assertRaises(OSError):
                self.install(version=version)
        self.assertEqual(os.readlink(self.prefix / 'current'), old_root)
        for name, data in old_files.items():
            self.assertEqual((self.launcher_dir / name).read_bytes(), data)
        self.assertFalse(list(self.prefix.glob('.marmish-stage-*')))

    def test_first_install_failure_leaves_no_live_install(self):
        with mock.patch.object(self.installer, 'replace_selector', side_effect=OSError('injected')):
            with self.assertRaises(OSError):
                self.install()
        self.assertFalse((self.launcher_dir / 'marmish').exists())
        self.assertFalse((self.prefix / self.installer.MARKER).exists())

    def test_tampered_launcher_and_uninstall_refuse_without_deletion(self):
        self.install()
        launcher = self.launcher_dir / 'marmish'
        launcher.write_bytes(b'#!/bin/sh\n# marmish installer format 1\nforeign content')
        with self.assertRaises(self.installer.InstallError):
            self.installer.uninstall(self.prefix, self.launcher_dir)
        self.assertTrue((self.launcher_dir / 'marmishd').exists())
        self.assertTrue((self.prefix / 'current').is_symlink())

    def test_root_and_relative_prefix_are_refused(self):
        with mock.patch.object(self.installer.os, 'geteuid', return_value=0):
            with self.assertRaises(self.installer.InstallError):
                self.install()
        for path in [Path('relative'), Path('/'), self.base / '..' / 'escape']:
            with self.assertRaises(self.installer.InstallError):
                self.installer.safe_path(path)

    def test_download_origin_and_redirect_allowlist(self):
        for url in ['http://github.com/a', 'https://evil.example/a', 'https://github.com:444/a', 'https://user:password@github.com/a', 'https://github.com/a#fragment']:
            with self.assertRaises(self.installer.InstallError):
                self.installer.checked_url(url)
        redirect = self.installer.ReleaseRedirect()
        request = self.installer.urllib.request.Request('https://github.com/a')
        with self.assertRaises(self.installer.InstallError):
            redirect.redirect_request(request, None, 302, 'Found', {}, 'https://evil.example/a')
        self.installer.checked_url('https://release-assets.githubusercontent.com/a?token=redacted')

    def test_release_selection_includes_prereleases_and_skips_draft(self):
        records = [{'tag_name': 'v0.2.0', 'draft': False, 'prerelease': False}, {'tag_name': 'v0.2.0-experimental.1', 'draft': True, 'prerelease': True}, {'tag_name': 'v' + VERSION, 'draft': False, 'prerelease': True}]
        def fake_download(url, path, limit):
            self.assertEqual(url, self.installer.API + '?per_page=100')
            path.write_text(json.dumps(records))
        with mock.patch.object(self.installer, 'download', side_effect=fake_download):
            selected, record = self.installer.release_metadata(None, self.base)
        self.assertEqual(selected, VERSION)
        self.assertEqual(record['tag_name'], 'v' + VERSION)

    def test_glibc_and_missing_shared_library_rejected(self):
        prerequisites = {'python': '3.11', 'glibc': '99.1', 'shared_libraries': []}
        with self.assertRaises(self.installer.InstallError):
            self.installer.validate_prerequisites(prerequisites, TARGET)
        prerequisites.update(glibc='2.17', shared_libraries=['libmissing_marmish_test.so.1'])
        with self.assertRaises(self.installer.InstallError):
            self.installer.validate_prerequisites(prerequisites, TARGET)

    def test_library_paths_are_system_only_and_rejected_before_loading(self):
        accepted = ['libc.so.6', 'libstdc++.so.6', '/usr/lib/libSystem.B.dylib', '/System/Library/Frameworks/Security.framework/Security']
        rejected = ['.', '..', 'lib..so', '/tmp/evil.so', './evil.so', 'relative/evil.so', '@rpath/libevil.dylib', '/usr/lib/../evil.so', '/usr/lib//evil.so', '/usr/lib/./evil.so', '/usr/lib/', '/System/Library//evil', 'libevil.so\n', 'libevil.so?x', 'libé.so']
        for library in accepted:
            self.assertTrue(self.installer.valid_library(library), library)
        for library in rejected:
            with self.subTest(library=library), mock.patch.object(self.installer.ctypes, 'CDLL') as load:
                prerequisites = {'python': '3.11', 'glibc': '2.2.5', 'shared_libraries': [library]}
                with self.assertRaises(self.installer.InstallError):
                    self.installer.validate_prerequisites(prerequisites, TARGET)
                load.assert_not_called()
        self.assertEqual(self.installer.version_tuple('2.2.5'), (2, 2, 5))

    def test_malformed_release_urls_have_a_fixed_safe_error(self):
        for url in ['https://github.com:secret-port/path', 'https://github.com:999999/path', 'https://[malformed/path']:
            with self.subTest(url=url), self.assertRaises(self.installer.InstallError) as caught:
                self.installer.checked_url(url)
            self.assertEqual(str(caught.exception), 'download URL outside approved HTTPS release hosts')

    def test_unsafe_directory_error_identifies_directory_and_remediation(self):
        self.prefix.mkdir(mode=0o777)
        self.prefix.chmod(0o777)
        with self.assertRaises(self.installer.InstallError) as caught:
            self.installer.owned_directory(self.prefix)
        self.assertIn(str(self.prefix), str(caught.exception))
        self.assertIn('chmod go-w', str(caught.exception))
        self.prefix.chmod(0o755)
        with self.assertRaises(self.installer.InstallError) as caught:
            self.installer.owned_directory(self.prefix, private=True)
        self.assertIn('chmod go-rwx', str(caught.exception))

    def test_case_colliding_license_paths_are_rejected(self):
        files = fixture_files(self.installer)
        files['licenses/Example/OTHER'] = b'case collision in parent directory'
        self.assert_rejected(archive_bytes(self.installer, files=files))

    def test_lock_refuses_concurrent_install_without_changes(self):
        self.prefix.mkdir(mode=0o700)
        with self.installer.installation_lock(self.prefix):
            with self.assertRaises(self.installer.InstallError):
                self.install()
        self.assertFalse((self.prefix / 'current').exists())
        self.install()

    def test_hardlinked_launchers_are_rejected(self):
        self.install()
        launcher = self.launcher_dir / 'marmish'
        os.link(launcher, self.base / 'unexpected-hardlink')
        with self.assertRaises(self.installer.InstallError):
            self.install()
        with self.assertRaises(self.installer.InstallError):
            self.installer.uninstall(self.prefix, self.launcher_dir)

    def test_asset_duplicates_and_unsupported_versions_are_rejected(self):
        asset = {'name': 'SHA256SUMS', 'state': 'uploaded', 'browser_download_url': 'https://github.com/test/sums'}
        with self.assertRaises(self.installer.InstallError):
            self.installer.release_assets({'assets': [asset, asset]}, 'archive.tar.gz')
        for version in ['0.1.0', 'v0.1.0-experimental.1', '../0.1.0-experimental.1', '0.1.0-experimental.01', '0.1.0-experimental.1;false', '1' * 100 + '.0.0-experimental.1']:
            with self.assertRaises(self.installer.InstallError):
                self.installer.validate_version(version)

    def test_download_disables_proxies_bounds_response_and_hides_error_details(self):
        response = mock.MagicMock()
        response.__enter__.return_value = response
        response.read.side_effect = [b'1234', b'']
        opener = mock.Mock()
        opener.open.return_value = response
        with mock.patch.object(self.installer.urllib.request, 'build_opener', return_value=opener) as build:
            with self.assertRaises(self.installer.InstallError):
                self.installer.download('https://github.com/a', self.base / 'download', 2)
        self.assertEqual(build.call_args.args[0].proxies, {})
        opener.open.side_effect = self.installer.urllib.error.URLError('secret response data')
        with mock.patch.object(self.installer.urllib.request, 'build_opener', return_value=opener):
            with self.assertRaises(self.installer.InstallError) as caught:
                self.installer.download('https://github.com/a', self.base / 'download2', 20)
        self.assertNotIn('secret response data', str(caught.exception))

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaises(self.installer.InstallError):
            self.installer.read_json_bytes(b'{"format":1,"format":1}')


if __name__ == '__main__':
    unittest.main()
