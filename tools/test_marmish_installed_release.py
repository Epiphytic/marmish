"""Host-independent pin resolution and adversarial installed-tree verification."""
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

import marmish_installed_release as release
from test_marmish_install import INSTALLER, archive_bytes, load_installer

CTX = {'run': '12345', 'attempt': '2', 'sha': 'a' * 40}


class InstalledReleaseTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.base = Path(temporary.name).resolve()
        self.module = load_installer()
        self.archives = {}
        self.metadata = {'tag_name': 'v' + release.VERSION, 'draft': False,
                         'prerelease': True, 'assets': []}
        for role, target in release.ROLE_TARGETS.items():
            name = f'marmish-{release.VERSION}-{target}.tar.gz'
            self.archives[name] = archive_bytes(self.module, version=release.VERSION, target=target)
            self.metadata['assets'].append({'name': name, 'state': 'uploaded',
                'browser_download_url': 'https://github.com/Epiphytic/marmish/releases/download/' + name})
        self.metadata['assets'].append({'name': 'SHA256SUMS', 'state': 'uploaded',
            'browser_download_url': 'https://github.com/Epiphytic/marmish/releases/download/SHA256SUMS'})
        self.downloads = []

    def download(self, url, destination, limit):
        self.downloads.append((url, destination))
        name = url.rsplit('/', 1)[-1]
        if name == 'v' + release.VERSION:
            raw = json.dumps(self.metadata).encode()
        elif name == 'SHA256SUMS':
            raw = ''.join(hashlib.sha256(raw).hexdigest() + '  ' + name + '\n'
                          for name, raw in self.archives.items()).encode()
        else:
            raw = self.archives[name]
        self.assertLessEqual(len(raw), limit)
        with destination.open('xb') as output:
            output.write(raw)

    def resolve(self, output=None):
        output = self.base / 'pin' if output is None else output
        with patch.dict(os.environ, {}, clear=True), \
                patch.object(release, 'installer_from_bytes', return_value=self.module), \
                patch.object(self.module, 'download', side_effect=self.download):
            return release.resolve_release(release.REPO, output, CTX, version=release.VERSION)

    def load(self):
        return release.load_pin(self.base / 'pin', CTX)

    def manifests(self):
        return {role: (self.base / 'pin' / (role + '.release.json')).read_bytes()
                for role in release.ROLES}

    def test_resolves_three_targets_without_loading_foreign_libraries(self):
        with patch.object(self.module.ctypes, 'CDLL', side_effect=AssertionError('foreign library')):
            pin = self.resolve()
        self.assertEqual(pin, self.load())
        self.assertEqual(set(pin['targets']), set(release.ROLES))
        self.assertEqual(len(self.downloads), 5)
        self.assertEqual(len(list((self.base / 'pin').iterdir())), 4)
        self.assertFalse(any(self.base.rglob('*.tar.gz')))
        raw = (self.base / 'pin/record.json').read_bytes()
        self.assertEqual(raw, release.canonical_bytes(pin))
        self.assertEqual(release.digest_file(self.base / 'pin/record.json'), hashlib.sha256(raw).hexdigest())

    def test_real_darwin_library_paths_are_schema_only_during_resolution(self):
        name = f'marmish-{release.VERSION}-{release.ROLE_TARGETS["macos-arm64"]}.tar.gz'
        self.archives[name] = archive_bytes(self.module, version=release.VERSION,
            target=release.ROLE_TARGETS['macos-arm64'], change_manifest=lambda value:
            value['prerequisites'].update(shared_libraries=[
                '/System/Library/Frameworks/Security.framework/Versions/A/Security']))
        with patch.object(self.module.ctypes, 'CDLL', side_effect=AssertionError('foreign library')):
            self.resolve()

    def test_mixed_sources_and_missing_or_duplicate_assets_do_not_publish_pin(self):
        for scenario in ('mixed', 'missing', 'duplicate'):
            with self.subTest(scenario=scenario):
                old = copy.deepcopy(self.metadata)
                archive_name = next(iter(self.archives))
                original = self.archives[archive_name]
                if scenario == 'mixed':
                    self.archives[archive_name] = archive_bytes(self.module, version=release.VERSION,
                        change_manifest=lambda value: value.update(source_commit='b' * 40))
                elif scenario == 'missing':
                    self.metadata['assets'].pop()
                else:
                    self.metadata['assets'].append(dict(self.metadata['assets'][0]))
                output = self.base / scenario
                with self.assertRaises((ValueError, self.module.InstallError)):
                    self.resolve(output)
                self.assertFalse(output.exists())
                self.metadata, self.archives[archive_name] = old, original

    def test_pin_rejects_binding_types_noncanonical_bytes_and_changed_sidecars(self):
        pin = self.resolve()
        for identifier in ('0', '01', '1' * 21, '\u0661', True):
            with self.subTest(identifier=identifier), self.assertRaises(ValueError):
                release.validate_context(dict(CTX, run=identifier))
        for mutate in (lambda p: p.update(attempt='3'), lambda p: p.update(format=True),
                       lambda p: p.update(sha='b' * 40), lambda p: p.update(extra='secret'),
                       lambda p: p['targets']['linux-x64'].update(archive_name='../escape')):
            bad = copy.deepcopy(pin)
            mutate(bad)
            with self.assertRaises(ValueError):
                release.validate_pin(bad, self.manifests(), CTX)
        path = self.base / 'pin/record.json'
        path.write_text(json.dumps(pin))
        with self.assertRaises(ValueError):
            self.load()
        path.write_bytes(release.canonical_bytes(pin))
        (self.base / 'pin/macos-arm64.release.json').write_bytes(b'{}')
        with self.assertRaises(ValueError):
            self.load()

    def test_pin_rejects_extra_files_symlinks_duplicate_fields_and_nonfinite_numbers(self):
        self.resolve()
        extra = self.base / 'pin/unexpected'
        extra.touch()
        with self.assertRaises(ValueError):
            self.load()
        extra.unlink()
        record = self.base / 'pin/record.json'
        saved = record.read_bytes()
        record.unlink()
        record.symlink_to(self.base / 'outside')
        with self.assertRaises(OSError):
            self.load()
        record.unlink()
        for invalid in (b'{"format":1,"format":1}', b'{"bad":NaN}', b'\xef\xbb\xbf' + saved):
            record.write_bytes(invalid)
            with self.assertRaises(ValueError):
                self.load()

    def test_retries_only_transient_downloads_with_fresh_paths(self):
        attempts = []
        def transient(url, path, limit):
            attempts.append(path)
            if len(attempts) == 1:
                path.write_bytes(b'partial')
                cause = urllib.error.HTTPError(url, 429, 'private', {}, None)
                raise self.module.InstallError('redacted') from cause
            self.download(url, path, limit)
        destination = self.base / 'download'
        with patch.object(self.module, 'download', side_effect=transient), patch.object(release.time, 'sleep'):
            release._download_retry(self.module, self.module.API + '/tags/v' + release.VERSION,
                                    destination, release.MAX_MANIFEST, release.continuous_time() + 10)
        self.assertTrue(destination.exists())
        self.assertNotEqual(attempts[0].parent, attempts[1].parent)
        with patch.object(self.module, 'download', side_effect=self.module.InstallError('invalid data')) as download:
            with self.assertRaisesRegex(ValueError, 'release_resolve_failed'):
                release._download_retry(self.module, 'https://github.com/asset', self.base / 'other',
                                        1024, release.continuous_time() + 10)
            self.assertEqual(download.call_count, 1)

    def installed_fixture(self):
        pin = self.resolve()
        prefix = self.base / "install with spaces ' $;"
        archive = self.base / 'fixture.tar.gz'
        archive.write_bytes(next(iter(self.archives.values())))
        with patch.object(self.module.platform, 'libc_ver', return_value=('glibc', '9.9')):
            self.module.install_archive(archive, prefix, prefix / 'bin', release.VERSION,
                                        release.ROLE_TARGETS['linux-x64'])
        return prefix, pin, self.manifests()['linux-x64']

    def verify(self, prefix, pin, raw):
        with patch.object(release, 'installer_from_bytes', return_value=self.module), \
                patch.object(self.module, 'native_target', return_value=release.ROLE_TARGETS['linux-x64']), \
                patch.object(self.module.platform, 'libc_ver', return_value=('glibc', '9.9')):
            return release.verify_installed(prefix, pin, raw, 'linux-x64')

    def test_verifies_real_installer_layout_including_lock_and_current_symlink(self):
        prefix, pin, raw = self.installed_fixture()
        result = self.verify(prefix, pin, raw)
        self.assertEqual(result['client'], prefix / 'bin/marmish')
        self.assertEqual(result['installation']['archive_sha256'], pin['targets']['linux-x64']['archive_sha256'])
        self.assertEqual((prefix / 'bin').stat().st_mode & 0o777, 0o700)

    def test_rejects_extra_tree_entries_payload_changes_and_owned_metadata_changes(self):
        prefix, pin, raw = self.installed_fixture()
        root = prefix / 'current'
        changes = [(prefix / '.marmish-stage-leftover', b'extra'),
                   (prefix / 'bin/extra', b'extra'),
                   (prefix / 'versions/extra', b'extra'),
                   (root / 'unexpected', b'extra'),
                   (root / 'LICENSE', b'changed'),
                   (prefix / '.marmish-install.lock', b'not empty'),
                   (prefix / 'bin/marmish', b'#!/bin/sh\nfalse\n')]
        for path, data in changes:
            old = path.read_bytes() if path.exists() else None
            with self.subTest(path=path.name):
                path.write_bytes(data)
                with self.assertRaises((ValueError, self.module.InstallError)):
                    self.verify(prefix, pin, raw)
            if old is None:
                path.unlink()
            else:
                path.write_bytes(old)
        for path in (prefix, prefix / 'bin', prefix / '.marmish-install.json',
                     prefix / 'versions', root / 'LICENSE', root / 'tools'):
            mode = path.stat().st_mode & 0o777
            with self.subTest(mode_path=path.name):
                path.chmod(mode ^ 0o040)
                with self.assertRaises((ValueError, self.module.InstallError)):
                    self.verify(prefix, pin, raw)
                path.chmod(mode)
        current = prefix / 'current'
        target = os.readlink(current)
        current.unlink()
        current.symlink_to('/tmp')
        with self.assertRaises((ValueError, self.module.InstallError)):
            self.verify(prefix, pin, raw)
        current.unlink()
        current.symlink_to(target)
        self.verify(prefix, pin, raw)

    def test_helper_load_uses_supplied_exact_bytes(self):
        raw = INSTALLER.read_bytes()
        changed = raw.replace(b'MAX_FILES = 4096', b'MAX_FILES = 17')
        self.assertNotEqual(changed, raw)
        helper = release.installer_from_bytes(changed)
        self.assertEqual(helper.MAX_FILES, 17)
        with self.assertRaises(ValueError):
            release.installer_from_bytes(raw + b'\n# END MARMISH INSTALLER PYTHON')
        with self.assertRaises(ValueError):
            release.installer_from_bytes((release.REPO / 'scripts/install.sh').read_bytes())

    def test_rejects_wrong_marker_types_hardlinks_and_release_drift(self):
        prefix, pin, raw = self.installed_fixture()
        marker = prefix / '.marmish-install.json'
        saved = marker.read_bytes()
        value = json.loads(saved)
        value['format'] = True
        marker.write_text(json.dumps(value))
        with self.assertRaises(ValueError):
            self.verify(prefix, pin, raw)
        marker.write_bytes(saved)
        license_file = prefix / 'current/LICENSE'
        os.link(license_file, self.base / 'linked-license')
        with self.assertRaises(ValueError):
            self.verify(prefix, pin, raw)
        (self.base / 'linked-license').unlink()
        changed = copy.deepcopy(pin)
        changed['installer_sha256'] = '0' * 64
        with self.assertRaises(ValueError):
            self.verify(prefix, changed, raw)
        with self.assertRaises(ValueError):
            self.verify(prefix, pin, raw + b'\n')
        extra = prefix / 'current/empty-directory'
        extra.mkdir(mode=0o755)
        with self.assertRaises(ValueError):
            self.verify(prefix, pin, raw)


if __name__ == '__main__':
    unittest.main()
