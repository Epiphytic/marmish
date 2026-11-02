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

class LoaderMatrixTests(unittest.TestCase):
    def setUp(self):
        self.workspace = tempfile.TemporaryDirectory()
        self.addCleanup(self.workspace.cleanup)
        self.root = Path(self.workspace.name)
        self.output = self.root / 'report.json'

    def test_fixed_profiles_and_unchanged_production_inputs(self):
        profiles = list(diagnostic.loader_profiles())
        self.assertEqual(len(profiles), 8)
        self.assertEqual(profiles[0], ('baseline', [], diagnostic.loader_base_profile()))
        self.assertEqual(profiles[1][2], PROFILE.encode())
        self.assertEqual(hashlib.sha256(diagnostic.loader_base_profile()).hexdigest(),
                         '14b1ab31a589f3f52b7b2ff93fadb35d26139c1a5cb154a0fdb50ed90a310d9a')
        self.assertEqual(hashlib.sha256(diagnostic.SOURCE.read_bytes()).hexdigest(),
                         '47977b39f2496f3f614125a78e3e174c8a9de2d8321ee0d87632d20962e7ce17')
        self.assertEqual(profiles[-1][1], ['root', 'map', 'cryptex'])
        self.assertEqual(len({raw for _, _, raw in profiles}), 8)
        self.assertIn(b'(allow file-read-data (literal "/"))', profiles[1][2])
        self.assertNotIn(b'(subpath "/")', profiles[-1][2])
        self.assertEqual(profiles[-1][2], diagnostic.loader_base_profile() +
                         ''.join(rule for _, rule in diagnostic.LOADER_RULES).encode())

    def test_historical_base_accepts_only_exact_old_and_new_profiles(self):
        historical = diagnostic.loader_base_profile()
        root = diagnostic.LOADER_RULES[0][1]
        for text in (historical.decode(), historical.decode() + root):
            with patch('marmish_transport_macos.PROFILE', text):
                self.assertEqual(diagnostic.loader_base_profile(), historical)
        for text in (PROFILE + root, root + historical.decode(), PROFILE + '\n',
                     historical.decode().replace('(deny default)', '(allow default)')):
            with self.subTest(text_digest=hashlib.sha256(text.encode()).hexdigest()), \
                    patch('marmish_transport_macos.PROFILE', text):
                with self.assertRaisesRegex(ValueError, '^loader_base_profile_changed$'):
                    diagnostic.loader_base_profile()

    def matrix(self, *, baseline=-6, negatives=2, controls=0, positive=0,
               stderr=''):
        daemon, profile = diagnostic.make_fixture(self.root, ('127.0.0.1', 12345))
        probe = daemon._directory / 'marmish-probe'
        seen = []
        def invoke(command, **kwargs):
            confined = command[0] == '/usr/bin/sandbox-exec'
            if confined:
                self.assertEqual(command[command.index('-f') + 1], str(profile))
                raw = profile.read_bytes()
                seen.append(raw)
                args = command[command.index('-f') + 3:]
                self.assertEqual(command, daemon._command(probe, *args))
                outside = args[-1] == str(self.root / 'outside/synthetic')
                tcp_negative = args[0] == 'tcp' and args[-1] != '12345'
                code = negatives if outside or tcp_negative else positive
                if raw == diagnostic.loader_base_profile():
                    code = baseline
            else:
                code = controls
            kwargs['stderr'].write(stderr.encode())
            return SimpleNamespace(returncode=code)
        with patch.object(diagnostic.subprocess, 'run', side_effect=invoke) as run:
            report = diagnostic.collect_loader_matrix(daemon, probe, [])
        self.assertEqual(run.call_count, 45)
        self.assertEqual(len(seen), 40)
        for index, (_, _, raw) in enumerate(diagnostic.loader_profiles()):
            self.assertEqual(seen[index * 5:(index + 1) * 5], [raw] * 5)
        target = self.root / 'outside/synthetic'
        self.assertEqual(target.read_bytes(), b'marmish synthetic outside fixture\n')
        self.assertEqual(target.stat().st_mode & 0o777, 0o600)
        self.assertNotIn(daemon.home, target.parents)
        self.assertNotIn(daemon._directory, target.parents)
        return report

    def test_matrix_exact_success_with_negative_controls(self):
        report = self.matrix()
        self.assertTrue(report['ok'])
        self.assertTrue(report['baseline_reproduced'])
        self.assertEqual(report['variants'][0]['classification'], 'loader_abort')
        self.assertEqual(report['variants'][1]['classification'], 'passed')
        self.assertEqual([case['exit_code'] for case in report['variants'][1]['cases']], [0, 0, 2, 2, 2])
        self.assertTrue(all(case['exit_code'] == 0 for case in report['controls']))
        self.assertEqual(report['variants'][1]['profile_sha256'],
                         hashlib.sha256(list(diagnostic.loader_profiles())[1][2]).hexdigest())

    def test_baseline_pass_cannot_credit_factors(self):
        report = self.matrix(baseline=0)
        self.assertFalse(report['baseline_reproduced'])
        self.assertFalse(report['ok'])

    def test_negative_crash_or_access_never_counts_as_denial(self):
        for code in (-6, 0, 1):
            with self.subTest(code=code), tempfile.TemporaryDirectory() as directory:
                self.root = Path(directory)
                report = self.matrix(negatives=code)
                self.assertFalse(report['ok'])
                self.assertTrue(all(not variant['ok'] for variant in report['variants']))

    def test_invalid_controls_continue_all_variants_fail_closed(self):
        report = self.matrix(controls=2)
        self.assertFalse(report['ok'])
        self.assertTrue(all(case['category'] == 'control_invalid'
                            for variant in report['variants'] for case in variant['cases']))

    def test_all_loader_failures_remain_reported(self):
        report = self.matrix(positive=-6)
        self.assertFalse(report['ok'])
        self.assertTrue(report['baseline_reproduced'])
        self.assertEqual([v['classification'] for v in report['variants']], ['loader_abort'] * 8)

    def test_maximum_stderr_utf8_and_escaping_report_bounds(self):
        for index, stderr in enumerate(('雪' * 1400, '\n\t' * 2500)):
            with self.subTest(index=index), tempfile.TemporaryDirectory() as directory:
                self.root = Path(directory)
                report = self.matrix(stderr=stderr)
                report.update({'schema': 2, 'mode': 'loader-matrix'})
                cases = report['controls'] + [c for v in report['variants'] for c in v['cases']]
                self.assertEqual(len(cases), 45)
                self.assertTrue(all(c['stderr_truncated'] for c in cases))
                self.assertTrue(all(len(c['stderr'].encode()) <= 512 for c in cases))
                output = self.root / 'report.json'
                diagnostic.write_report(output, report)
                self.assertLessEqual(output.stat().st_size, 65536)
                self.assertEqual(json.loads(output.read_text())['ok'], report['ok'])

    def test_oversized_report_fixed_fallback_and_exclusive_write(self):
        report = {'schema': 2, 'mode': 'loader-matrix', 'ok': True, 'private': '雪' * 65536}
        diagnostic.write_report(self.output, report)
        self.assertEqual(report, {'schema': 2, 'mode': 'loader-matrix', 'ok': False,
                                 'failure': {'category': 'report_too_large', 'exit_code': None}})
        self.assertLessEqual(self.output.stat().st_size, 65536)
        self.assertNotIn('private', self.output.read_text())
        self.assertEqual(self.output.stat().st_mode & 0o777, 0o600)
        with self.assertRaises(FileExistsError):
            diagnostic.write_report(self.output, report)

    def test_default_oversize_fallback_preserves_report_kind(self):
        report = {'schema': 1, 'ok': True, 'private': 'x' * 65536}
        diagnostic.write_report(self.output, report)
        self.assertEqual(report, {'schema': 1, 'ok': False,
                                 'failure': {'category': 'report_too_large', 'exit_code': None}})
        self.assertNotIn('mode', json.loads(self.output.read_text()))

    def test_timeout_cannot_satisfy_exit_or_abort(self):
        case = {'category': 'completed', 'exit_code': -6, 'signal': 6, 'timeout': True}
        self.assertFalse(diagnostic.exact_exit(case, -6))
        self.assertFalse(diagnostic.loader_abort([case, case]))

    def test_opt_in_cli_and_refusal_schema(self):
        with patch.object(diagnostic, 'diagnose', return_value=1) as diagnose:
            self.assertEqual(diagnostic.main(['--loader-matrix', '--output', str(self.output)]), 1)
        diagnose.assert_called_once_with(self.output.absolute(), loader_matrix=True)
        with patch.object(diagnostic.platform, 'system', return_value='Linux'):
            self.assertEqual(diagnostic.diagnose(self.output, loader_matrix=True), 1)
        report = json.loads(self.output.read_text())
        self.assertEqual(report['schema'], 2)
        self.assertEqual(report['variants'], [])
        self.assertEqual(report['controls'], [])
        self.assertFalse(report['baseline_reproduced'])

    def test_native_matrix_build_failure_and_drift_report_before_cleanup(self):
        native = self.root / 'native'
        @contextlib.contextmanager
        def fixture(**kwargs):
            native.mkdir()
            try:
                yield str(native)
            finally:
                self.assertTrue(self.output.exists())
                import shutil
                shutil.rmtree(native)
        for drift in (False, True):
            with self.subTest(drift=drift), contextlib.ExitStack() as stack:
                stack.enter_context(patch.object(diagnostic.platform, 'system', return_value='Darwin'))
                stack.enter_context(patch.object(diagnostic.os, 'geteuid', return_value=1000))
                stack.enter_context(patch.object(diagnostic.tempfile, 'TemporaryDirectory', side_effect=fixture))
                build = stack.enter_context(patch.object(diagnostic, 'build_probe',
                    return_value={'category': 'sign_failed', 'exit_code': 1}))
                if drift:
                    stack.enter_context(patch('marmish_transport_macos.PROFILE', PROFILE + '\n'))
                self.assertEqual(diagnostic.diagnose(self.output, loader_matrix=True), 1)
                report = json.loads(self.output.read_text())
                self.assertEqual(report['failure']['category'], 'fixture_failed' if drift else 'sign_failed')
                self.assertEqual(report['variants'], [])
                if drift:
                    build.assert_not_called()
                    self.assertIsNone(report['base_profile_sha256'])
                else:
                    self.assertEqual(report['base_profile_sha256'], diagnostic.LOADER_BASE_SHA256)
                self.assertFalse(native.exists())
                self.output.unlink()


if __name__ == '__main__':
    unittest.main()
