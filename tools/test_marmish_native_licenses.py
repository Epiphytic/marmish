"""Offline checks for the CI-only native inventory adapter."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import marmish_native_licenses as native
import marmish_release_licenses as licenses


class NativeLicenseInventory(unittest.TestCase):
    def fixture(self, parent):
        directory = parent / 'native'
        (directory / 'texts').mkdir(parents=True)
        raw = b'Original license notice\n'
        digest = hashlib.sha256(raw).hexdigest()
        (directory / 'texts' / (digest + '.txt')).write_bytes(raw)
        source = {'url': licenses.SOURCES['native'], 'revision': 'a' * 40}
        packages = [{'name': 'marmish-native', 'version': '0.2.0', 'license': 'MIT',
            'source': source, 'license_override': None, 'notices': [
                {'original_path': 'LICENSE', 'sha256': digest,
                 'text': 'texts/' + digest + '.txt'}]}]
        manifest = {'format': 1, 'graph': 'native', 'target': 'x86_64-unknown-linux-gnu',
            'source': source, 'cargo_lock_sha256': 'b' * 64, 'packages': packages}
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        (directory / 'SOURCES.txt').write_text(licenses._source_notice(source, packages, True))
        return directory, digest

    def test_assembles_validated_source_inventory_and_original_texts(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            directory, digest = self.fixture(root)
            output = root / 'THIRD-PARTY-LICENSES'
            native.assemble(directory, output)
            content = output.read_text()
            self.assertIn('marmish-native', content)
            self.assertIn(digest, content)
            self.assertIn('Original license notice', content)
            self.assertNotIn('patches/mdk/', content)
            with self.assertRaises(FileExistsError):
                native.assemble(directory, output)

    def test_refuses_tampered_notice_before_creating_asset(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            directory, digest = self.fixture(root)
            (directory / 'texts' / (digest + '.txt')).write_text('changed')
            output = root / 'asset'
            with self.assertRaisesRegex(RuntimeError, 'license_digest_mismatch'):
                native.assemble(directory, output)
            self.assertFalse(output.exists())

    def test_arti_original_notices_use_exact_published_vcs_revision(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            revision = 'c' * 40
            (root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': revision}}))
            package = {'name': 'arti-client', 'version': '0.47.0',
                       'repository': 'https://gitlab.torproject.org/tpo/core/arti.git/',
                       'manifest_path': str(root / 'Cargo.toml')}
            licenses.UPSTREAM_CACHE.clear()
            with mock.patch.object(licenses, '_download_notice', return_value=b'Original upstream notice') as fetch:
                notices = licenses._upstream_notices(package)
                self.assertEqual(len(notices), len(licenses.UPSTREAM_FILES))
                expected = 'https://gitlab.torproject.org/tpo/core/arti/-/raw/' + revision + '/'
                self.assertEqual({call.args[0] for call in fetch.call_args_list},
                                 {expected + name for name in licenses.UPSTREAM_FILES})
            package['repository'] = 'https://gitlab.torproject.org/tpo/core/another-project'
            with self.assertRaisesRegex(RuntimeError, 'unreviewed_license_source'):
                licenses._upstream_identity(package)
            package['repository'] = 'https://gitlab.torproject.org/tpo/core/arti'
            (root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': revision, 'dirty': True}}))
            with self.assertRaisesRegex(RuntimeError, 'invalid_license_source_revision'):
                licenses._upstream_identity(package)
            licenses.UPSTREAM_CACHE.clear()

    def test_explicit_missing_vcs_overrides_and_second_tor_repository(self):
        for (name, version), identity in licenses.SOURCE_OVERRIDES.items():
            self.assertEqual(licenses._upstream_identity({'name': name, 'version': version}), identity)
            self.assertRegex(identity[1], r'^[0-9a-f]{40}$')
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': 'd' * 40}}))
            package = {'name': 'derive-deftly-macros', 'version': '1.12.1',
                       'repository': 'https://gitlab.torproject.org/Diziet/rust-derive-deftly',
                       'manifest_path': str(root / 'Cargo.toml')}
            self.assertEqual(licenses._upstream_identity(package), (package['repository'], 'd' * 40))
            self.assertIn('LICENCE', licenses.UPSTREAM_FILES)
            licenses.UPSTREAM_CACHE.clear()
            with mock.patch.object(licenses, '_download_notice', return_value=b'Original notice') as fetch:
                licenses._upstream_notices(package)
                self.assertTrue(all(call.args[0].startswith(package['repository'] + '/-/raw/' + 'd' * 40 + '/')
                                    for call in fetch.call_args_list))
            licenses.UPSTREAM_CACHE.clear()

    def test_reuse_notice_is_original_text_at_exact_published_revision(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            revision = 'e' * 40
            (root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': revision}}))
            package = {'name': 'cookie-factory', 'version': '0.3.3',
                       'repository': 'https://github.com/rust-bakery/cookie-factory',
                       'manifest_path': str(root / 'Cargo.toml')}
            expected = 'https://raw.githubusercontent.com/rust-bakery/cookie-factory/' + revision + '/LICENSES/MIT.txt'
            licenses.UPSTREAM_CACHE.clear()
            with mock.patch.object(licenses, '_download_notice',
                    side_effect=lambda url: b'Original REUSE notice' if url == expected else None):
                notices = licenses._upstream_notices(package)
                self.assertEqual(len(notices), 1)
                self.assertIn(b'Original REUSE notice', [item[1] for item in notices])
            licenses.UPSTREAM_CACHE.clear()

    def test_exact_optional_license_choice_requires_original_expression_and_text(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            package = {'name': 'priority-queue', 'version': '2.7.0',
                'license': 'LGPL-3.0-or-later OR MPL-2.0', 'source': None,
                'manifest_path': str(root / 'Cargo.toml')}
            (root / 'MPL-2.0.txt').write_text('Original selected MPL notice')
            # Upstream-only spelling is deliberately handled by the fixed fetch list.
            notices = [('MPL-2.0.txt', b'Original selected MPL notice', 'https://example.invalid/pinned')]
            with mock.patch.object(licenses, '_upstream_notices', return_value=notices):
                record = licenses._package_record(package, root, {'url': licenses.SOURCES['native'], 'revision': 'a'*40}, {}, {})
                self.assertEqual(record['license'], 'MPL-2.0')
                self.assertIn(package['license'], record['license_override'])
            package['version'] = '2.7.1'
            with self.assertRaisesRegex(RuntimeError, 'unreviewed_license_expression'):
                licenses._package_record(package, root, {}, {}, {})
            package['version'] = '2.7.0'
            package['license'] = 'LGPL-3.0-only OR MPL-2.0'
            with self.assertRaisesRegex(RuntimeError, 'license_choice_expression_mismatch'):
                licenses._package_record(package, root, {}, {}, {})
            package['license'] = 'LGPL-3.0-or-later OR MPL-2.0'
            with mock.patch.object(licenses, '_upstream_notices', return_value=[('LICENSE', b'Other notice', 'https://example.invalid/pinned')]):
                with self.assertRaisesRegex(RuntimeError, 'missing_selected_license_text'):
                    licenses._package_record(package, root, {}, {}, {})

    def test_void_declaration_is_exact_source_and_separate_canonical_template(self):
        package = {'name': 'void', 'version': '1.0.2', 'license': 'MIT',
            'source': 'registry+https://github.com/rust-lang/crates.io-index',
            'repository': 'https://github.com/reem/rust-void.git'}
        originals = [b'Original upstream author and MIT declaration', b'Canonical SPDX template']
        pins = [(name, url, hashlib.sha256(raw).hexdigest())
                for (name, url, _), raw in zip(licenses.VOID_NOTICES, originals)]
        with mock.patch.object(licenses, 'VOID_NOTICES', pins), mock.patch.object(licenses,
                '_download_notice', side_effect=originals):
            notices = licenses._upstream_notices(package)
            self.assertEqual([item[1] for item in notices], originals)
            self.assertEqual(notices[1][0], 'canonical-license-template/MIT.txt')
        for key, value in [('repository', 'https://github.com/other/void'),
                           ('source', None), ('license', 'MIT OR Apache-2.0')]:
            altered = dict(package, **{key: value})
            with self.assertRaisesRegex(RuntimeError, 'void_declaration_source_mismatch'):
                licenses._upstream_notices(altered)
        with mock.patch.object(licenses, '_download_notice', return_value=b'changed'):
            with self.assertRaisesRegex(RuntimeError, 'void_declaration_digest_mismatch'):
                licenses._upstream_notices(package)
        with mock.patch.object(licenses, '_upstream_identity', side_effect=RuntimeError('ordinary_source_path')):
            with self.assertRaisesRegex(RuntimeError, 'ordinary_source_path'):
                licenses._upstream_notices(dict(package, version='1.0.3'))

    def test_license_redirects_restrict_tor_host_and_repository(self):
        import urllib.request
        handler = licenses._RawRedirect()
        request = urllib.request.Request('https://gitlab.torproject.org/tpo/core/arti/-/raw/' + 'a' * 40 + '/LICENSE')
        allowed = request.full_url
        self.assertIsNotNone(handler.redirect_request(request, None, 302, '', {}, allowed))
        for url in ('http://gitlab.torproject.org/tpo/core/arti/-/raw/x/LICENSE',
                    'https://gitlab.torproject.org/other/arti/-/raw/x/LICENSE',
                    'https://gitlab.torproject.org.evil.invalid/tpo/core/arti/-/raw/x/LICENSE'):
            with self.assertRaisesRegex(RuntimeError, 'unsafe_license_redirect'):
                handler.redirect_request(request, None, 302, '', {}, url)

    def test_preserves_exact_rust_standard_library_notices_and_provenance(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            docs = root / 'share/doc/rust'
            (docs / 'licenses').mkdir(parents=True)
            for name in ('MIT.txt', 'Apache-2.0.txt'):
                (docs / 'licenses' / name).write_text('Original ' + name)
            (docs / 'COPYRIGHT-library.html').write_text('Original standard library copyrights')
            result = native.rust_notices(root, b'rustc fixture\ncommit-hash: ' + b'a' * 40 + b'\n')
            self.assertIn(b'Original standard library copyrights', result)
            self.assertIn(b'Original MIT.txt', result)
            self.assertIn(b'https://github.com/rust-lang/rust/tree/' + b'a' * 40, result)
            (docs / 'licenses' / 'MIT.txt').unlink()
            with self.assertRaisesRegex(RuntimeError, 'missing_rust_toolchain_notices'):
                native.rust_notices(root, b'commit-hash: ' + b'a' * 40 + b'\n')


if __name__ == '__main__':
    unittest.main()
