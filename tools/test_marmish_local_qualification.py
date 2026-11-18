import contextlib
import io
import json
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import marmish_local_qualification as qualification


class LocalQualificationTests(unittest.TestCase):
    def test_preflight_failure_keeps_its_sanitized_category(self):
        argv = ['qualification', '--source', '.', '--assets', '.', '--version', '0.3.1',
                '--commit', 'invalid-argument']
        output = io.StringIO()
        with (patch.object(qualification.os, 'geteuid', return_value=501),
              patch.object(qualification.signal, 'signal'), patch('sys.argv', argv),
              contextlib.redirect_stdout(output)):
            self.assertEqual(qualification.main(), 1)
        report = json.loads(output.getvalue())
        self.assertEqual(report['failure'], 'source_commit')
        self.assertFalse(report['ok'])
        self.assertNotIn('invalid-argument', output.getvalue())

    def test_empty_cargo_selection_fails(self):
        options = SimpleNamespace(source=Path('.'), terminal='local', commit='a' * 40)
        stdout = b'running 0 tests\ntest result: ok. 0 passed; 0 failed;\n'
        with (patch.object(qualification.os, 'chdir'),
              patch.object(qualification, 'execute', return_value=(0, stdout))):
            with self.assertRaisesRegex(qualification.Refused, '^fixture_test_count$'):
                qualification.terminal_cases(options, Path('/usr/bin/tmux'), {'fixture_passes': []})

    def test_terminal_failure_reports_only_fixed_diagnostics(self):
        options = SimpleNamespace(source=Path('.'), terminal='local', commit='a' * 40)
        summary = (b'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; '
                   b'969 filtered out; finished in 3.14s\n')
        started = b'running 1 test\nprivate-child-text\n'
        cases = (
            (101, started + summary + b'\n', 'selected_test_failed', 101),
            (101, started + summary + b'private-child-text\n', 'no_recognized_failure_summary', 101),
            (101, b'private-child-text\n', 'no_recognized_failure_summary', 101),
            (-6, b'', 'no_recognized_failure_summary', -6),
            (-129, b'', 'no_recognized_failure_summary', None),
            (256, b'', 'no_recognized_failure_summary', None),
            (True, b'', 'no_recognized_failure_summary', None),
        )
        for code, stdout, kind, reported_code in cases:
            with (self.subTest(code=code, kind=kind), patch.object(qualification.os, 'chdir'),
                  patch.object(qualification, 'execute', return_value=(code, stdout)) as execute):
                report = {'fixture_passes': []}
                with self.assertRaisesRegex(qualification.Refused, '^operation_failed$'):
                    qualification.terminal_cases(options, Path('/usr/bin/tmux'), report)
                self.assertEqual(report['fixture_case'], qualification.LOCAL_CASES[0])
                self.assertEqual(report['fixture_failure_kind'], kind)
                if reported_code is None:
                    self.assertNotIn('fixture_exit_code', report)
                else:
                    self.assertEqual(report['fixture_exit_code'], reported_code)
                self.assertNotIn('private-child-text', json.dumps(report))
                self.assertEqual(report['fixture_passes'], [])
                self.assertEqual(execute.call_args.kwargs, {'timeout': 1200, 'success': False})

    def test_passing_terminal_selection_keeps_success_report(self):
        options = SimpleNamespace(source=Path('.'), terminal='local', commit='a' * 40)
        stdout = b'running 1 test\ntest result: ok. 1 passed; 0 failed;\n'
        with (patch.object(qualification.os, 'chdir'),
              patch.object(qualification, 'execute', return_value=(0, stdout))):
            report = {'fixture_passes': []}
            qualification.terminal_cases(options, Path('/usr/bin/tmux'), report)
        self.assertEqual(report, {'stage': 'terminal_fixture',
                                  'fixture_passes': list(qualification.LOCAL_CASES)})


if __name__ == '__main__':
    unittest.main()
