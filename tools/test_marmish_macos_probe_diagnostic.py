"""Offline fixture/report contracts; none of these tests proves macOS confinement."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import marmish_macos_probe_diagnostic as diagnostic
from marmish_transport_macos import DarwinConfinedDaemon, PROFILE


class ProbeDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.workspace = tempfile.TemporaryDirectory()
        self.addCleanup(self.workspace.cleanup)
        self.root = Path(self.workspace.name)
        self.output = self.root / 'report.json'

    def test_redaction_before_bounds_and_partial_boundary(self):
        path = '/Users/private-person/private-fixture'
        raw = ('a' * 4080 + path + '\x00\x1b\n\t').encode()
        value, truncated = diagnostic.redact_stderr(raw[:4097], [(path, '<fixture>')])
        self.assertTrue(truncated)
        self.assertNotIn('/Users', value)
        self.assertLessEqual(len(value.encode()), 4096)
        value, truncated = diagnostic.redact_stderr((path + '\x00\x1b\n\t').encode(), [(path, '<fixture>')])
        self.assertEqual(value, '<fixture>\n\t')
        self.assertFalse(truncated)

    def test_utf8_bound_is_bytes(self):
        value, truncated = diagnostic.redact_stderr(('雪' * 1400).encode(), [])
        self.assertTrue(truncated)
        self.assertLessEqual(len(value.encode()), 4096)

    def test_fixture_exact_profile_and_sibling_paths(self):
        daemon, profile = diagnostic.make_fixture(self.root, ('127.0.0.1', 12345))
        self.assertIsInstance(daemon, DarwinConfinedDaemon)
        self.assertEqual(profile.read_bytes(), PROFILE.encode())
        self.assertEqual(daemon.home.parent, daemon._directory.parent)
        self.assertNotIn(daemon.home, profile.parents)
        self.assertEqual(profile.stat().st_mode & 0o777, 0o600)
        self.assertEqual(daemon._broker.address, ('127.0.0.1', 12345))

    def test_case_exact_command_builder_and_continue_invalid_control(self):
        daemon, _ = diagnostic.make_fixture(self.root, ('127.0.0.1', 12345))
        probe = daemon._directory / 'marmish-probe'
        calls = []
        def run(case_id, command, redactions, control):
            calls.append((case_id, command, control))
            return {'id': case_id, 'category': 'completed',
                    'exit_code': 2 if case_id == 'read_control' else 0}
        with patch.object(diagnostic, 'run_case', side_effect=run):
            results = diagnostic.collect_cases(daemon, probe, [])
        self.assertEqual(len(calls), 5)
        self.assertEqual(calls[1][1], daemon._command(probe, 'read', '/dev/null'))
        self.assertEqual(calls[3][1], daemon._command(probe, 'tcp', '127.0.0.1', '12345'))
        self.assertEqual(calls[4][1], daemon._command('/usr/bin/true'))
        self.assertEqual(calls[4][2], 'absent')
        self.assertEqual(results[1]['category'], 'control_invalid')
        self.assertEqual(results[3]['category'], 'completed')

    def test_case_signal_bounded_stderr_and_launch_contract(self):
        def invoke(command, **kwargs):
            self.assertEqual(command, ['trusted-probe'])
            self.assertEqual(kwargs['stdin'], subprocess.DEVNULL)
            self.assertEqual(kwargs['stdout'], subprocess.DEVNULL)
            self.assertEqual(kwargs['env'], {'PATH': '/usr/bin:/bin'})
            self.assertEqual(kwargs['timeout'], 5)
            self.assertTrue(kwargs['close_fds'])
            kwargs['stderr'].write(b'sandbox denial\x00\n')
            return SimpleNamespace(returncode=-6)
        with patch.object(diagnostic.subprocess, 'run', side_effect=invoke):
            result = diagnostic.run_case('read_confined', ['trusted-probe'], [], 'read_control')
        self.assertEqual(result['exit_code'], -6)
        self.assertEqual(result['signal'], 6)
        self.assertEqual(result['stderr'], 'sandbox denial\n')
        self.assertFalse(diagnostic.succeeded(result))

    def test_timeout_and_oserror_categories_never_leak_exception(self):
        for error, category in [(subprocess.TimeoutExpired(['secret'], 5), 'timeout'),
                                (OSError('private-sensitive-path'), 'launch_failed')]:
            with self.subTest(category=category), patch.object(diagnostic.subprocess, 'run', side_effect=error):
                result = diagnostic.run_case('read_control', ['trusted-probe'], [], 'self')
            self.assertEqual(result['category'], category)
            self.assertEqual(result['timeout'], category == 'timeout')
            self.assertNotIn('secret', json.dumps(result))
            self.assertNotIn('private-sensitive', json.dumps(result))

    def test_exact_build_recipe_and_failure_categories(self):
        probe = self.root / 'probe'
        with patch.object(diagnostic.subprocess, 'run', return_value=SimpleNamespace(returncode=0)) as run:
            self.assertIsNone(diagnostic.build_probe(probe))
        self.assertEqual([call.args[0] for call in run.call_args_list], [
            ['clang', *diagnostic.FLAGS, str(diagnostic.SOURCE), '-o', str(probe)],
            ['/usr/bin/codesign', '--force', '--sign', '-', str(probe)],
            ['/usr/bin/codesign', '--verify', '--strict', str(probe)]])
        with patch.object(diagnostic.subprocess, 'run', return_value=SimpleNamespace(returncode=8)) as run:
            self.assertEqual(diagnostic.build_probe(probe), {'category': 'compile_failed', 'exit_code': 8})
            self.assertEqual(run.call_count, 1)
        with patch.object(diagnostic.subprocess, 'run', side_effect=OSError('secret')):
            self.assertEqual(diagnostic.build_probe(probe), {'category': 'compile_unavailable', 'exit_code': None})
        with patch.object(diagnostic.subprocess, 'run', side_effect=subprocess.TimeoutExpired(['secret'], 30)):
            self.assertEqual(diagnostic.build_probe(probe), {'category': 'compile_timeout', 'exit_code': None})

    def test_report_exclusive_private_and_no_symlink(self):
        diagnostic.write_report(self.output, {'ok': False})
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            diagnostic.write_report(self.output, {'ok': True})
        link = self.root / 'link'
        link.symlink_to(self.output)
        with self.assertRaises(FileExistsError):
            diagnostic.write_report(link, {'ok': True})
        self.assertFalse(json.loads(self.output.read_text())['ok'])

    def test_platform_root_and_old_python_refusals(self):
        for category, system, euid, version in [
                ('darwin_required', 'Linux', 1000, (3, 11)),
                ('ordinary_user_required', 'Darwin', 0, (3, 11)),
                ('python_too_old', 'Darwin', 1000, (3, 9))]:
            with self.subTest(category=category), patch.object(diagnostic.platform, 'system', return_value=system), \
                    patch.object(diagnostic.os, 'geteuid', return_value=euid), \
                    patch.object(diagnostic.sys, 'version_info', version):
                output = self.root / category
                self.assertEqual(diagnostic.diagnose(output), 1)
                self.assertEqual(json.loads(output.read_text())['failure']['category'], category)

    def native_run(self, run_effect=None, build_failure=None):
        native_fixture = self.root / 'native'
        native_fixture.mkdir()
        @contextlib.contextmanager
        def owned_fixture(**kwargs):
            self.assertEqual(kwargs['dir'], '/private/tmp')
            try:
                yield str(native_fixture)
            finally:
                self.assertTrue(self.output.exists(), 'report must precede fixture cleanup')
                import shutil
                shutil.rmtree(native_fixture)
        with patch.object(diagnostic.platform, 'system', return_value='Darwin'), \
                patch.object(diagnostic.os, 'geteuid', return_value=1000), \
                patch.object(diagnostic.tempfile, 'TemporaryDirectory', side_effect=owned_fixture), \
                patch.object(diagnostic, 'build_probe', return_value=build_failure), \
                patch.object(diagnostic.subprocess, 'run', side_effect=run_effect,
                             return_value=SimpleNamespace(returncode=0)):
            status = diagnostic.diagnose(self.output)
        self.assertFalse(native_fixture.exists())
        return status, json.loads(self.output.read_text())

    def test_native_report_success_hashes_and_cleanup(self):
        status, report = self.native_run()
        self.assertEqual(status, 0)
        self.assertTrue(report['ok'])
        self.assertEqual(report['profile_sha256'], hashlib.sha256(PROFILE.encode()).hexdigest())
        self.assertEqual(len(report['cases']), 5)

    def test_native_report_signal_and_timeout_survive_cleanup(self):
        for result in ('signal', 'timeout'):
            with self.subTest(result=result):
                if self.output.exists():
                    self.output.unlink()
                effect = (lambda *args, **kwargs: SimpleNamespace(returncode=-6)) if result == 'signal' else \
                    subprocess.TimeoutExpired(['trusted-probe'], 5)
                status, report = self.native_run(run_effect=effect)
                self.assertEqual(status, 1)
                self.assertFalse(report['ok'])
                self.assertEqual(len(report['cases']), 5)
                self.assertEqual(report['cases'][0]['signal'], 6 if result == 'signal' else None)
                self.assertEqual(report['cases'][0]['timeout'], result == 'timeout')

    def test_native_build_failure_report_and_cleanup(self):
        status, report = self.native_run(build_failure={'category': 'compile_failed', 'exit_code': 9})
        self.assertEqual(status, 1)
        self.assertEqual(report['failure']['category'], 'compile_failed')
        self.assertEqual(report['cases'], [])

    def test_main_report_failure_is_fixed_and_cli_required(self):
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(diagnostic.main(['--output', str(self.root / 'missing' / 'report')]), 1)
        self.assertEqual(stderr.getvalue(), 'marmish probe diagnostic: report_or_cleanup_failed\n')
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as error:
            diagnostic.main([])
        self.assertEqual(error.exception.code, 2)


if __name__ == '__main__':
    unittest.main()
