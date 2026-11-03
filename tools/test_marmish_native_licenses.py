"""Offline checks for the CI-only native inventory adapter."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

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
