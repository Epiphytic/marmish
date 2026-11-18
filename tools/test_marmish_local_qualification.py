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


if __name__ == '__main__':
    unittest.main()
