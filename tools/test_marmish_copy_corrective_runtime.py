"""Offline selected-copy qualification contract; no native processes or network."""
import contextlib
import copy
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

import marmish_copy_corrective_runtime as corrective
from marmish_copy_runtime import CopyQualification
from marmish_stream_runtime import QualificationError, arguments


class CorrectiveTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.binary = self.root / 'native'
        self.binary.write_bytes(b'not executed')
        self.binary.chmod(0o700)
        self.baseline_path = self.root / 'baseline.json'
        self.baseline = {
            'schema': 1, 'suite': 'native_copy_source', 'passed': True, 'payload_bytes': 32,
            'binary': {'sha256': corrective.file_sha256(self.binary), 'version': '0.2.3'},
            'platform': os.uname().sysname, 'architecture': os.uname().machine,
            'relay': 'wss://relay.invalid', 'relay_path_redacted': False,
            'results': [self.row(case) for case in corrective.SELECTABLE],
        }
        self.save()
        self.argv = ['--marmish', str(self.binary), '--relay', 'wss://relay.invalid/',
                     '--output', str(self.root / 'result.json'), '--bytes', '32',
                     '--baseline-report', str(self.baseline_path), '--case', 'get_raw']

    def tearDown(self):
        self.directory.cleanup()

    def row(self, case):
        if case.endswith('_existing_destination'):
            return {'case': case, 'exit': 1, 'seconds': 20.0, 'expected_error': 'transfer_conflict'}
        compressed = case.endswith('_zlib')
        return {'case': case, 'exit': 0, 'seconds': 20.0, 'compression': compressed,
                'payload_bytes': 32, 'negotiated_codec': 'zlib' if compressed else 'none',
                'sha256': 'a' * 64, 'payload_bytes_per_second': 1.6}

    def save(self):
        self.baseline_path.write_text(json.dumps(self.baseline))

    def parse(self, extra=()):
        with contextlib.redirect_stderr(io.StringIO()):
            return corrective.corrective_arguments(self.argv + list(extra))

    def reject(self, extra=()):
        with patch.object(corrective.SelectedCopyQualification, '__init__') as constructor:
            with self.assertRaises(SystemExit) as failure:
                self.parse(extra)
            self.assertEqual(failure.exception.code, 2)
            constructor.assert_not_called()
        self.assertEqual(set(self.root.iterdir()), {self.binary, self.baseline_path})

    def test_arguments_validate_selection_and_before_state(self):
        self.assertEqual(self.parse().case, ['get_raw'])
        for case in ('get_raw', 'put_missing_source', 'get_missing_source', 'unknown'):
            with self.subTest(case=case):
                self.reject(['--case', case])
        self.reject(['--timeout-seconds', '79'])
        self.assertEqual(self.parse(['--timeout-seconds', '80']).baseline['required_timeout_seconds'], 80)
        self.reject(['--output', str(self.baseline_path)])
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            corrective.corrective_arguments(self.argv[:-2])

    def test_baseline_context_codec_bytes_and_strict_types(self):
        original = copy.deepcopy(self.baseline)
        modifications = [
            lambda b: b.update(payload_bytes=33),
            lambda b: b.update(payload_bytes=True),
            lambda b: b.update(platform='different'),
            lambda b: b.update(relay_path_redacted=True),
            lambda b: b['binary'].update(sha256='b' * 64),
            lambda b: b.update(source_revision='c' * 40),
            lambda b: b['results'][1].update(exit=False),
            lambda b: b['results'][1].update(seconds=0.0),
            lambda b: b['results'][1].update(seconds=-1.0),
            lambda b: b['results'][1].update(seconds=True),
            lambda b: b['results'][1].update(seconds=float('inf')),
            lambda b: b['results'][1].update(payload_bytes=True),
            lambda b: b['results'][1].update(negotiated_codec='zlib'),
            lambda b: b['results'].append(copy.deepcopy(b['results'][1])),
        ]
        for modification in modifications:
            self.baseline = copy.deepcopy(original)
            modification(self.baseline)
            self.save()
            self.reject()
        self.baseline = original
        self.baseline['passed'] = False
        self.save()
        self.assertFalse(self.parse().baseline['overall_passed'])

    def test_baseline_bounded_json_and_nonfinite_exponent(self):
        text = json.dumps(self.baseline)
        for invalid in (text.replace('20.0', '1e999'),
                        text.replace('"schema": 1', '"schema": 1, "schema": 1'),
                        text.replace('20.0', 'NaN'),
                        '[' * 10000 + ']' * 10000,
                        'x' * (1024 * 1024 + 1)):
            self.baseline_path.write_text(invalid)
            self.reject()
        self.baseline_path.unlink()
        os.mkfifo(self.baseline_path)
        self.reject()  # Nonblocking open/fstat must reject without waiting for a writer.

    def test_refusal_baseline_requires_exact_legacy_or_optional_fields(self):
        self.argv[-1] = 'put_existing_destination'
        self.assertEqual(self.parse().baseline['selected']['put_existing_destination']['kind'],
                         'expected_refusal')
        row = self.baseline['results'][-2]
        for key, value in [('exit', 0), ('expected_error', 'other'), ('compression', True),
                           ('sha256', 'a' * 64), ('seconds', False)]:
            with self.subTest(key=key):
                saved = row.copy()
                row[key] = value
                self.save()
                self.reject()
                row.clear()
                row.update(saved)

    def fake_run(self, calls, *, wrong_codec=False, replace_empty=False):
        def run(stage, identity, *argv):
            calls.append(stage)
            position = argv.index('--')
            source, destination = Path(argv[position + 2]), Path(argv[position + 3])
            if not source.exists():
                return subprocess.CompletedProcess(argv, 1, b'', b'file_not_found')
            if destination.exists():
                if replace_empty:
                    replacement = destination.with_suffix('.replacement')
                    replacement.write_bytes(b'')
                    os.replace(replacement, destination)
                return subprocess.CompletedProcess(argv, 1, b'', b'transfer_conflict')
            shutil.copy2(source, destination)
            codec = 'zlib' if '--compress' in argv else 'none'
            if wrong_codec:
                codec = 'wrong'
            stderr = ('marmish copy: transport codec ' + codec + '\n'
                      'marmish copy: committed and verified transfer ' + 'a' * 64 + '\n').encode()
            return subprocess.CompletedProcess(argv, 0, b'', stderr)
        return run

    def qualification(self, options):
        with patch.object(Path, 'home', return_value=self.root):
            return corrective.SelectedCopyQualification(options)

    def test_get_only_local_seed_and_no_hidden_transfers(self):
        options = self.parse()
        q = self.qualification(options)
        calls = []
        q.run = self.fake_run(calls)
        q.copy_cases('remote')
        self.assertEqual(calls, ['get_raw'])
        self.assertEqual(q.report['locally_seeded_get_sources'], ['get_raw'])
        self.assertEqual(q.report['completed_cases'], ['get_raw'])
        self.assertFalse(q.report['full_gate_claimed'])
        self.assertTrue(q.report['selected_only'])
        self.assertEqual(len(q.report['omitted_cases']), 11)
        self.assertGreater(q.report['results'][0]['deadline_headroom'], 1.5)

    def test_all_six_cases_canonical_order_and_actual_refusals(self):
        self.argv[-1] = 'get_existing_destination'
        options = self.parse([item for case in reversed(corrective.SELECTABLE[:-1])
                              for item in ('--case', case)])
        q = self.qualification(options)
        calls = []
        q.run = self.fake_run(calls)
        q.copy_cases('remote')
        self.assertEqual(calls, list(corrective.SELECTABLE))
        self.assertEqual(q.report['completed_cases'], calls)
        self.assertEqual(q.report['locally_seeded_get_sources'], [])
        self.assertEqual([r['exit'] for r in q.report['results']], [0, 0, 0, 0, 1, 1])
        self.assertIn('get_missing_source', q.report['not_covered'])
        self.assertNotIn('get_existing_destination', q.report['not_covered'])

    def test_headroom_codec_and_empty_replacement_fail_closed(self):
        for kind in ('headroom', 'codec', 'replacement'):
            if kind == 'replacement':
                self.argv[-1] = 'get_existing_destination'
            options = self.parse()
            q = self.qualification(options)
            q.run = self.fake_run([], wrong_codec=kind == 'codec', replace_empty=kind == 'replacement')
            context = (patch.object(corrective.time, 'monotonic', side_effect=[0.0, 0.1, 0.2, 500.0])
                       if kind == 'headroom' else contextlib.nullcontext())
            with context, self.assertRaises(QualificationError) as failure:
                q.copy_cases('remote')
            self.assertEqual(failure.exception.category, {
                'headroom': 'corrective_headroom_insufficient',
                'codec': 'negotiated_codec_missing',
                'replacement': 'refusal_changed_operand'}[kind])
            self.assertEqual(q.report['completed_cases'], [])

    def test_binary_recheck_precedes_exec_and_version_is_pinned(self):
        options = self.parse()
        q = self.qualification(options)
        q.run = Mock()
        original = self.binary.read_bytes()
        self.binary.write_bytes(b'changed')
        with self.assertRaises(QualificationError) as failure:
            q.identify_binary()
        self.assertEqual(failure.exception.category, 'baseline_binary_changed')
        q.run.assert_not_called()
        self.binary.write_bytes(original)
        q.run.return_value = subprocess.CompletedProcess([], 0, b'marmish 0.2.4\n', b'')
        with self.assertRaises(QualificationError) as failure:
            q.identify_binary()
        self.assertEqual(failure.exception.category, 'baseline_binary_changed')
        q.run.return_value = subprocess.CompletedProcess([], 0, b'marmish 0.2.3\n', b'')
        q.identify_binary()
        self.assertEqual(q.report['binary'], self.baseline['binary'])

    def test_cli_reaches_managed_cases_and_cleans_owned_state(self):
        calls = []
        fake = self.fake_run(calls)

        def run(_self, stage, identity, *argv, **kwargs):
            if stage == 'version':
                return subprocess.CompletedProcess([], 0, b'marmish 0.2.3\n', b'')
            return fake(stage, identity, *argv)

        with contextlib.redirect_stdout(io.StringIO()), \
                patch.object(Path, 'home', return_value=self.root), \
                patch.object(corrective.SelectedCopyQualification, 'run', new=run), \
                patch.object(corrective.SelectedCopyQualification, 'initialize', return_value='remote'), \
                patch.object(corrective.SelectedCopyQualification, 'start_daemons'), \
                patch.object(corrective.SelectedCopyQualification, 'cleanup', return_value=True):
            self.assertEqual(corrective.cli(self.argv), 0)
        report = json.loads((self.root / 'result.json').read_text())
        self.assertTrue(report['passed'])
        self.assertFalse(report['private_state_retained'])
        self.assertFalse(report['full_gate_claimed'])
        self.assertEqual(report['completed_cases'], ['get_raw'])
        self.assertEqual(calls, ['get_raw'])
        self.assertFalse(any(path.name.startswith('.marmish-stream-check-') for path in self.root.iterdir()))

    def test_default_full_suite_unchanged(self):
        with patch('sys.argv', ['copy-runtime', *self.argv[:8]]):
            # Shared parser has no new required flags or changed default timeout.
            options = arguments(32)
        self.assertEqual(options.timeout_seconds, 600)
        with patch.object(Path, 'home', return_value=self.root):
            q = CopyQualification(options)
        calls = []
        q.run = self.fake_run(calls)
        q.copy_cases('remote')
        self.assertEqual(calls, list(corrective.CORE_CASES))
        self.assertEqual(q.report['suite'], 'native_copy_source')
        self.assertNotIn('selected_only', q.report)


if __name__ == '__main__':
    unittest.main()
