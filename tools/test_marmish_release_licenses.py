"""Distribution notices preserve original texts and fail closed on missing material."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import marmish_release_licenses as licenses


class LicenseCollectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='marmish-license-test-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / 'LICENSE').write_text('Original project license and copyright\n')
        (self.root / 'Cargo.lock').write_text('version = 4\n[[package]]\nname="marmish-engine"\nversion="0.1.0"\n')
        self.package = {'id': 'engine', 'name': 'marmish-engine', 'version': '0.1.0',
            'manifest_path': str(self.root / 'Cargo.toml'), 'source': None,
            'license': 'MIT', 'license_file': None}

    def metadata(self):
        packages = [dict(self.package, id=name, name=name) for name in licenses.ROOTS['marmish']]
        return {'packages': packages, 'resolve': {'nodes': [
            {'id': p['id'], 'deps': []} for p in packages]}}

    def collect(self):
        with patch.object(licenses, '_metadata', return_value=self.metadata()), \
                patch.object(licenses.subprocess, 'check_output', return_value='1' * 40 + '\n'):
            licenses.collect(self.root, self.root / 'out', 'marmish')
        return self.root / 'out/marmish'

    def test_original_text_deduplication_manifest_and_validation(self):
        directory = self.collect()
        manifest = json.loads((directory / 'manifest.json').read_bytes())
        self.assertEqual(len(manifest['packages']), 3)
        texts = list((directory / 'texts').iterdir())
        self.assertEqual(len(texts), 1)
        self.assertEqual(texts[0].read_bytes(), (self.root / 'LICENSE').read_bytes())
        licenses.validate_graph(directory)

    def test_corrupted_or_unreferenced_text_rejected(self):
        directory = self.collect()
        text = next((directory / 'texts').iterdir())
        text.write_bytes(b'changed')
        with self.assertRaisesRegex(RuntimeError, 'license_digest_mismatch'):
            licenses.validate_graph(directory)
        text.write_bytes((self.root / 'LICENSE').read_bytes())
        (directory / 'private-state').write_text('must never be bundled')
        with self.assertRaisesRegex(RuntimeError, 'unexpected_license_output'):
            licenses.validate_graph(directory)

    def test_missing_or_unreviewed_license_fails_before_output(self):
        self.package['license'] = 'GPL-3.0-only'
        with self.assertRaisesRegex(RuntimeError, 'unreviewed_license_expression'):
            self.collect()
        self.assertFalse((self.root / 'out/marmish').exists())
        self.package['license'] = None
        with self.assertRaisesRegex(RuntimeError, 'missing_license_metadata'):
            self.collect()

    def test_production_closure_includes_build_dependencies_excludes_dev(self):
        metadata = self.metadata()
        for name, kind in [('build-dependency', 'build'), ('test-dependency', 'dev')]:
            metadata['packages'].append(dict(self.package, id=name, name=name))
            metadata['resolve']['nodes'].append({'id': name, 'deps': []})
            metadata['resolve']['nodes'][0]['deps'].append({'pkg': name,
                'dep_kinds': [{'kind': kind}]})
        names = {p['name'] for p in licenses._production_packages(metadata, 'marmish')}
        self.assertIn('build-dependency', names)
        self.assertNotIn('test-dependency', names)

    def test_native_notice_is_mandatory(self):
        package = dict(self.package, name='libsqlite3-sys')
        with self.assertRaisesRegex(RuntimeError, 'missing_native_license'):
            licenses._package_notices(package, self.root)
        (self.root / 'sqlcipher').mkdir()
        (self.root / 'sqlcipher/LICENSE').write_text('Zetetic copyright and distribution conditions\n')
        paths = licenses._package_notices(package, self.root)
        self.assertIn('sqlcipher/LICENSE', [relative for _, relative in paths])

    def test_internal_symlink_license_copied_but_escape_rejected(self):
        nested = self.root / 'crate'
        nested.mkdir()
        (nested / 'LICENSE').symlink_to('../LICENSE')
        package = dict(self.package, manifest_path=str(nested / 'Cargo.toml'))
        paths = licenses._package_notices(package, self.root)
        self.assertIn((self.root / 'LICENSE', 'LICENSE'), paths)
        (nested / 'LICENSE').unlink()
        (nested / 'LICENSE').symlink_to('/etc/passwd')
        with self.assertRaisesRegex(RuntimeError, 'external_license_text'):
            licenses._package_notices(package, self.root)

    def test_upstream_fallback_is_immutable_and_records_original_bytes(self):
        (self.root / 'LICENSE').unlink()
        (self.root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': '2' * 40}}))
        package = dict(self.package, source='registry+https://github.com/rust-lang/crates.io-index',
                       repository='https://github.com/example/original')
        lock = {(package['name'], package['version'], package['source']): {'checksum': '3' * 64}}
        texts = {}
        with patch.dict(licenses.UPSTREAM_CACHE, {}, clear=True), \
                patch.object(licenses, '_download_notice', return_value=b'Original upstream notice\n') as download:
            record = licenses._package_record(package, self.root,
                {'url': licenses.SOURCES['marmish'], 'revision': '1' * 40}, lock, texts)
        self.assertEqual(download.call_count, len(licenses.UPSTREAM_FILES))
        for notice in record['notices']:
            self.assertIn('/' + '2' * 40 + '/', notice['source_url'])
            self.assertEqual(texts[notice['sha256']], b'Original upstream notice\n')

    def test_missing_or_mutable_fallback_source_rejected(self):
        package = dict(self.package, repository='http://github.com/example/original')
        with self.assertRaisesRegex(RuntimeError, 'unreviewed_license_source'):
            licenses._upstream_identity(package)
        package['repository'] = 'https://github.com/example/original'
        (self.root / '.cargo_vcs_info.json').write_text('{"git":{"sha1":"main"}}')
        with self.assertRaisesRegex(RuntimeError, 'invalid_license_source_revision'):
            licenses._upstream_identity(package)


if __name__ == '__main__':
    unittest.main()
