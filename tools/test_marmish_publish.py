"""Publication gates reject mixed builds and missing installed execution proof."""
import json
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import tarfile
import unittest
from unittest.mock import patch

import marmish_publish as publication
from test_marmish_install import archive_bytes, load_installer

VERSION = '0.1.0-experimental.1'
SOURCE = 'a' * 40


class PublicationTests(unittest.TestCase):
    def setUp(self):
        environment = patch.dict(os.environ, {'GITHUB_RUN_ID': '12345'})
        environment.start()
        self.addCleanup(environment.stop)
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.inputs = self.base / 'inputs'
        self.inputs.mkdir()
        installer = load_installer()
        for target in publication.TARGETS:
            base = 'marmish-' + VERSION + '-' + target
            data = archive_bytes(installer, target=target)
            path = self.inputs / (base + '.tar.gz')
            path.write_bytes(data)
            with tarfile.open(path) as archive:
                record = archive.extractfile(base + '/release.json').read()
            (self.inputs / (base + '.release.json')).write_bytes(record)
        self.report = self.inputs / 'installed-e2e.json'
        self.report.write_text(json.dumps({'outcome': 'passed', 'local_cleanup': 'complete',
            'distribution_source_commit': SOURCE,
            'distribution_archive_sha256': publication.file_digest(self.inputs /
                ('marmish-' + VERSION + '-x86_64-unknown-linux-gnu.tar.gz')),
            'wnd_sha256': hashlib.sha256(b'fixture libexec/marmish/wnd\n').hexdigest(),
            'assertions': ['installed_launchers_and_bundled_defaults_preserve_special_character_argv']}))

    def tearDown(self):
        self.temporary.cleanup()

    def aggregate(self):
        return publication.aggregate(self.inputs, self.base / 'output', VERSION, SOURCE)

    def test_complete_same_source_builds_have_checksums(self):
        records = self.aggregate()
        self.assertEqual(set(records), publication.TARGETS)
        self.assertEqual(len((self.base / 'output/SHA256SUMS').read_text().splitlines()), 7)

    def test_mixed_source_fails_before_staging(self):
        path = next(self.inputs.glob('*.release.json'))
        record = json.loads(path.read_text())
        record['source_commit'] = 'b' * 40
        path.write_text(json.dumps(record))
        with self.assertRaisesRegex(ValueError, 'build_identity_mismatch'):
            self.aggregate()
        self.assertFalse((self.base / 'output').exists())

    def test_missing_or_failed_live_cleanup_blocks_publication(self):
        self.report.write_text(json.dumps({'outcome': 'passed', 'local_cleanup': 'incomplete'}))
        with self.assertRaisesRegex(ValueError, 'invalid_installed_live_evidence'):
            self.aggregate()
        self.report.unlink()
        with self.assertRaisesRegex(ValueError, 'missing_installed_live_evidence'):
            self.aggregate()

    def test_payload_corruption_blocks_publication(self):
        path = next(self.inputs.glob('*.tar.gz'))
        path.write_bytes(b'corrupted archive')
        with self.assertRaises(Exception):
            self.aggregate()

    def test_live_evidence_must_match_the_exact_archive(self):
        report = json.loads(self.report.read_text())
        for digest in (None, '0' * 64):
            with self.subTest(digest=digest):
                report['distribution_archive_sha256'] = digest
                self.report.write_text(json.dumps(report))
                with self.assertRaisesRegex(ValueError, 'invalid_installed_live_evidence'):
                    self.aggregate()
                self.assertFalse((self.base / 'output').exists())

    def test_extensions_and_unsafe_directory_modes_block_publication(self):
        path = next(self.inputs.glob('*.tar.gz'))
        target = path.name[len('marmish-' + VERSION + '-'):-len('.tar.gz')]
        root = 'marmish-' + VERSION + '-' + target
        extension = tarfile.TarInfo(root + '/licenses/extension')
        extension.type = tarfile.XHDTYPE
        directory = tarfile.TarInfo(root + '/licenses')
        directory.type, directory.mode = tarfile.DIRTYPE, 0o700
        extended_directory = tarfile.TarInfo(root + '/licenses')
        extended_directory.type, extended_directory.mode = tarfile.DIRTYPE, 0o755
        extended_directory.pax_headers = {'comment': 'ignored by permissive tar readers'}
        installer = load_installer()
        for member, archive_format in ((extension, tarfile.USTAR_FORMAT),
                (directory, tarfile.USTAR_FORMAT), (extended_directory, tarfile.PAX_FORMAT)):
            with self.subTest(kind=member.type):
                path.write_bytes(archive_bytes(installer, target=target,
                    extras=[(member, b'')], archive_format=archive_format))
                with self.assertRaisesRegex(ValueError, 'invalid_release_archive'):
                    self.aggregate()
                self.assertFalse((self.base / 'output').exists())

    def test_noncanonical_gzip_trailer_blocks_publication(self):
        path = next(self.inputs.glob('*.tar.gz'))
        path.write_bytes(path.read_bytes() + b'untrusted trailing bytes')
        with self.assertRaisesRegex(ValueError, 'invalid_release_archive:trailing gzip data'):
            self.aggregate()
        self.assertFalse((self.base / 'output').exists())

    def test_failed_upload_keeps_release_draft(self):
        output = self.base / 'output'
        output.mkdir()
        with patch.object(publication, 'verify_remote_tag'), \
                patch.object(publication, 'verify_master_source'), \
                patch.object(publication, 'refuse_existing_release'), \
                patch.object(publication.subprocess, 'run') as run:
            run.side_effect = [None, RuntimeError('upload failed')]
            with self.assertRaises(RuntimeError):
                publication.publish(output, VERSION, SOURCE, {})
            self.assertEqual(run.call_count, 2)
            self.assertIn('--draft', run.call_args_list[0].args[0])
            self.assertNotIn('edit', run.call_args_list[1].args[0])

    def test_leftover_draft_refused_before_any_release_mutation(self):
        result = subprocess.CompletedProcess([], 0,
            stdout=json.dumps([{'tag_name': 'v' + VERSION, 'draft': True}]))
        with patch.object(publication, 'verify_master_source'), \
                patch.object(publication.subprocess, 'run', return_value=result) as run:
            with self.assertRaisesRegex(ValueError, 'release_already_exists'):
                publication.publish(self.base / 'output', VERSION, SOURCE, {})
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[0][:2], ['gh', 'api'])
        self.assertFalse((self.base / 'release-notes.md').exists())

    def test_inventory_api_error_refused_before_any_release_mutation(self):
        result = subprocess.CompletedProcess([], 1, stdout='', stderr='API failure')
        with patch.object(publication, 'verify_master_source'), \
                patch.object(publication.subprocess, 'run', return_value=result) as run:
            with self.assertRaisesRegex(ValueError, 'release_inventory_unavailable'):
                publication.publish(self.base / 'output', VERSION, SOURCE, {})
            self.assertEqual(run.call_count, 1)
        self.assertFalse((self.base / 'release-notes.md').exists())

    def test_stale_master_refuses_before_release_mutation(self):
        result = subprocess.CompletedProcess([], 0,
            stdout='b' * 40 + '\trefs/heads/master\n')
        with patch.object(publication.subprocess, 'run', return_value=result) as run:
            with self.assertRaisesRegex(ValueError, 'release_master_source_mismatch'):
                publication.publish(self.base / 'output', VERSION, SOURCE, {})
            self.assertEqual(run.call_count, 1)
            self.assertEqual(run.call_args.args[0][:2], ['git', 'ls-remote'])
        self.assertFalse((self.base / 'release-notes.md').exists())

    def test_master_advance_during_upload_keeps_release_draft(self):
        output = self.base / 'output'
        output.mkdir()
        with patch.object(publication, 'verify_master_source',
                side_effect=[None, ValueError('release_master_source_mismatch')]) as master, \
                patch.object(publication, 'refuse_existing_release'), \
                patch.object(publication, 'verify_remote_tag'), \
                patch.object(publication.subprocess, 'run') as run:
            with self.assertRaisesRegex(ValueError, 'release_master_source_mismatch'):
                publication.publish(output, VERSION, SOURCE, {})
            self.assertEqual(master.call_count, 2)
            self.assertEqual([call.args[0][2] for call in run.call_args_list],
                             ['create', 'upload', 'download'])

    def test_release_notes_identify_hosted_build_and_fixed_run_url(self):
        with patch.dict(os.environ, {'GITHUB_RUN_ID': '12345',
                'GITHUB_SERVER_URL': 'https://untrusted.invalid',
                'GITHUB_REPOSITORY': 'injected/repository'}):
            notes = publication.release_notes({}, SOURCE)
        self.assertIn('Native GitHub-hosted builds', notes)
        self.assertIn('https://github.com/Epiphytic/marmish/actions/runs/12345', notes)
        self.assertNotIn('untrusted.invalid', notes)
        for run_id in ('', '123\nInjected Markdown', '../123', '0'):
            with patch.dict(os.environ, {'GITHUB_RUN_ID': run_id}):
                with self.assertRaisesRegex(ValueError, 'invalid_publication_run'):
                    publication.release_notes({}, SOURCE)


class ReleaseTagTests(unittest.TestCase):
    def test_master_guard_requires_exact_ref_and_available_remote(self):
        cases = [(0, SOURCE + '\trefs/heads/master\n', None),
            (2, '', 'release_master_unavailable'),
            (1, '', 'release_master_unavailable'),
            (0, SOURCE + '\trefs/heads/other\n', 'release_master_source_mismatch'),
            (0, SOURCE + '\trefs/heads/master\nextra', 'release_master_source_mismatch')]
        for code, stdout, error in cases:
            with self.subTest(code=code, stdout=stdout), patch.object(publication.subprocess,
                    'run', return_value=subprocess.CompletedProcess([], code, stdout=stdout)):
                if error:
                    with self.assertRaisesRegex(ValueError, error):
                        publication.verify_master_source(SOURCE)
                else:
                    publication.verify_master_source(SOURCE)

    def test_release_inventory_checks_later_pages_and_exact_tags(self):
        first = [{'tag_name': 'v' + VERSION + '-other'}] * 100
        responses = [subprocess.CompletedProcess([], 0, stdout=json.dumps(first)),
            subprocess.CompletedProcess([], 0, stdout=json.dumps([{'tag_name': 'v' + VERSION}]))]
        with patch.object(publication.subprocess, 'run', side_effect=responses) as run:
            with self.assertRaisesRegex(ValueError, 'release_already_exists'):
                publication.refuse_existing_release('v' + VERSION)
            self.assertIn('page=2', run.call_args.args[0][2])
        with patch.object(publication.subprocess, 'run', return_value=
                subprocess.CompletedProcess([], 0, stdout=json.dumps(first[:1]))):
            publication.refuse_existing_release('v' + VERSION)

    def test_absent_tag_allowed_before_draft_but_required_after_publication(self):
        result = subprocess.CompletedProcess([], 2, stdout='')
        with patch.object(publication.subprocess, 'run', return_value=result):
            publication.verify_remote_tag('v' + VERSION, SOURCE, required=False)
            with self.assertRaisesRegex(ValueError, 'release_tag_unavailable'):
                publication.verify_remote_tag('v' + VERSION, SOURCE, required=True)

    def test_lightweight_tag_must_reference_exact_source(self):
        ref = 'refs/tags/v' + VERSION
        for oid in (SOURCE, 'b' * 40):
            with self.subTest(oid=oid), patch.object(publication.subprocess, 'run',
                    return_value=subprocess.CompletedProcess([], 0, stdout=oid + '\t' + ref + '\n')):
                if oid == SOURCE:
                    publication.verify_remote_tag('v' + VERSION, SOURCE, required=True)
                else:
                    with self.assertRaisesRegex(ValueError, 'release_tag_source_mismatch'):
                        publication.verify_remote_tag('v' + VERSION, SOURCE, required=False)

    def test_annotated_tag_checks_peeled_commit(self):
        ref = 'refs/tags/v' + VERSION
        result = subprocess.CompletedProcess([], 0,
            stdout='b' * 40 + '\t' + ref + '\n' + SOURCE + '\t' + ref + '^{}\n')
        with patch.object(publication.subprocess, 'run', return_value=result):
            publication.verify_remote_tag('v' + VERSION, SOURCE, required=True)


if __name__ == '__main__':
    unittest.main()
