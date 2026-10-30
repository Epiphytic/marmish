"""Hermetic ownership/selection tests; no real macOS reports or processes."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

import marmish_macos_crash_diagnostic as crash


class CrashDiagnosticTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.probe = self.root / 'mp-12345678'
        self.reports = self.root / 'reports'
        self.reports.mkdir()
        self.fd = os.open(self.reports, os.O_RDONLY | os.O_DIRECTORY)
        self.addCleanup(os.close, self.fd)
        self.name = 'mp-12345678_2026.ips'

    def report(self, **changes):
        payload = {'pid': 123, 'procPath': str(self.probe),
                   'exception': {'type': 'EXC_CRASH', 'signal': 'SIGABRT'},
                   'termination': {'namespace': 'DYLD', 'code': 4, 'details': 'denied'},
                   'asi': {'libsystem_secinit.dylib': ['sandbox_init failure']},
                   'faultingThread': 0,
                   'threads': [{'frames': [{'symbol': 'abort', 'imageIndex': 0, 'symbolLocation': 128}]}],
                   'usedImages': [{'path': '/usr/lib/system/libsystem_secinit.dylib', 'uuid': 'secret-uuid'}],
                   'coalitionName': 'private-host', 'environment': {'SECRET': 'private-value'}}
        payload.update(changes)
        (self.reports / self.name).write_text(json.dumps({'meta': 'private-meta'}) + '\n' + json.dumps(payload))
        return payload

    def read(self):
        return crash.read_context(self.fd, self.name, 123, self.probe, [])

    def test_matching_owned_report_extracts_only_allowlist(self):
        self.report()
        context = self.read()
        self.assertEqual(context['category'], 'matched')
        self.assertEqual(context['executable'], 'probe')
        self.assertEqual(context['facts']['frames'], [{'symbol': 'abort', 'image': 'libsystem_secinit.dylib'}])
        for private in ('private-host', 'private-value', 'private-meta', 'secret-uuid', 'symbolLocation', str(self.probe)):
            self.assertNotIn(private, json.dumps(context))
        self.report(procPath='/usr/bin/sandbox-exec')
        self.assertEqual(self.read()['executable'], 'sandbox_exec')

    def test_pid_and_path_selection_is_exact(self):
        for changes, category in [({'pid': 124}, 'candidate_pid_rejected'),
                                  ({'pid': '123'}, 'candidate_pid_rejected'),
                                  ({'procPath': str(self.probe) + '-other'}, 'candidate_path_rejected'),
                                  ({'procPath': '/bin/bash'}, 'candidate_path_rejected')]:
            self.report(**changes)
            self.assertEqual(self.read(), {'category': category})

    def test_only_new_matching_names_selected(self):
        for name in (self.name, 'sandbox-exec-2026.ips', 'other.ips', 'mp-12345678.txt'):
            (self.reports / name).touch()
        names, category = crash.snapshot(self.reports, self.probe.name)
        self.assertEqual(category, 'available')
        self.assertEqual(names, {self.name, 'sandbox-exec-2026.ips'})
        with patch.object(crash.time, 'monotonic', side_effect=[0, 21]):
            context = crash.wait_context(self.reports, self.probe.name, names, 123, self.probe, [])
        self.assertEqual(context, {'category': 'context_timed_out', 'candidate_count': 0})

    def test_regular_owned_nofollow_and_size_checks(self):
        target = self.reports / self.name
        target.symlink_to(self.probe)
        self.assertEqual(self.read()['category'], 'candidate_unavailable')
        target.unlink()
        target.mkdir()
        self.assertEqual(self.read()['category'], 'candidate_not_owned_regular')
        target.rmdir()
        self.report()
        with patch.object(crash.os, 'getuid', return_value=os.getuid() + 1):
            self.assertEqual(self.read()['category'], 'candidate_not_owned_regular')
        with target.open('wb') as stream:
            stream.truncate(crash.MAX_IPS + 1)
        self.assertEqual(self.read()['category'], 'candidate_oversized')

    def test_directory_nofollow_and_ownership(self):
        alias = self.root / 'alias'
        alias.symlink_to(self.reports)
        self.assertEqual(crash.open_reports(alias), (None, 'reports_dir_unavailable'))
        with patch.object(crash.os, 'getuid', return_value=os.getuid() + 1):
            self.assertEqual(crash.open_reports(self.reports), (None, 'reports_dir_unowned'))
        self.assertEqual(crash.open_reports(self.root / 'missing'), (None, 'reports_dir_absent'))

    def test_parse_requires_two_json_objects(self):
        for value, category in [(b'bad', 'candidate_parse_failed'),
                                 (b'{}\n{bad}', 'candidate_parse_failed'),
                                 (b'[]\n{}', 'candidate_invalid'),
                                 (b'{}\n[]', 'candidate_invalid'),
                                 (b'{}\n{}\n{}', 'candidate_parse_failed'),
                                 (b'{}\n' + b'[' * 2000 + b']' * 2000, 'candidate_invalid')]:
            (self.reports / self.name).write_bytes(value)
            self.assertEqual(self.read()['category'], category)
        with patch.object(crash.json, 'loads', side_effect=RecursionError):
            self.assertEqual(self.read()['category'], 'candidate_parse_failed')

    def test_extraction_bounds_and_redaction_before_caps(self):
        private = '/Users/private-person/secret-fixture'
        payload = {'exception': {'type': private},
                   'termination': {'details': 'x' * 1010 + private, 'byProc': private},
                   'asi': {name: [private + 'x' * 3000] * 8 for name in crash.ASI_KEYS},
                   'faultingThread': 0,
                   'threads': [{'frames': [{'symbol': 'x' * 240 + private, 'imageIndex': 0}] * 30}],
                   'usedImages': [{'path': '/secret/path/libname.dylib'}]}
        facts = crash.extract(payload, [(private, '<fixture>')])
        self.assertNotIn('/Users', json.dumps(facts))
        self.assertNotIn('secret/path', json.dumps(facts))
        self.assertLessEqual(len(facts['termination_details']), 1024)
        self.assertEqual(len(facts['asi']), 6)
        self.assertTrue(all(len(item['value']) <= 1024 for item in facts['asi']))
        self.assertEqual(len(facts['frames']), 24)
        self.assertTrue(all(len(item['symbol']) <= 256 for item in facts['frames']))
        self.assertEqual(crash.bounded('x' * 65537, [], 256), '<oversized>')
        self.assertEqual(crash.extract({'asi': {'unlisted': ['private']}}, [])['asi'], [])

    def test_missing_reports_directory_is_rechecked_and_late_report_matches(self):
        missing = self.root / 'late'
        self.assertEqual(crash.snapshot(missing, self.probe.name), (set(), 'reports_dir_absent'))
        self.report()
        first = True
        def opened(directory):
            nonlocal first
            if first:
                first = False
                return None, 'reports_dir_absent'
            return os.dup(self.fd), None
        with patch.object(crash, 'open_reports', side_effect=opened), \
                patch.object(crash.time, 'sleep') as sleep, \
                patch.object(crash.time, 'monotonic', side_effect=[0, 1]):
            context = crash.wait_context(missing, self.probe.name, set(), 123, self.probe, [])
        self.assertEqual(context['category'], 'matched')
        self.assertEqual(context['candidate_count'], 1)
        sleep.assert_called_once_with(1)

    def test_candidate_limit_and_timeout_categories(self):
        for index in range(9):
            (self.reports / ('mp-12345678_%d.ips' % index)).touch()
        context = crash.wait_context(self.reports, self.probe.name, set(), 123, self.probe, [])
        self.assertEqual(context, {'category': 'candidate_limit', 'candidate_count': 9})
        with patch.object(crash.time, 'monotonic', side_effect=[0, 21]):
            context = crash.wait_context(self.root / 'missing', self.probe.name, set(), 123, self.probe, [])
        self.assertEqual(context, {'category': 'reports_dir_absent', 'candidate_count': 0})

    def test_child_pid_signal_and_fixed_launch_contract(self):
        child = MagicMock(pid=123)
        child.wait.return_value = -6
        child.poll.return_value = -6
        with patch.object(crash.subprocess, 'Popen', return_value=child) as popen:
            result = crash.run_child('read_confined', ['probe'], [])
        self.assertEqual(result['pid'], 123)
        self.assertEqual(result['signal'], 6)
        child.wait.assert_called_once_with(timeout=5)
        child.kill.assert_not_called()
        options = popen.call_args.kwargs
        self.assertEqual(options['env'], {'PATH': '/usr/bin:/bin'})
        self.assertEqual(options['stdin'], subprocess.DEVNULL)
        self.assertEqual(options['stdout'], subprocess.DEVNULL)
        self.assertTrue(options['close_fds'])

    def test_child_timeout_kills_and_reaps(self):
        child = MagicMock(pid=123)
        child.wait.side_effect = [subprocess.TimeoutExpired(['probe'], 5), -9]
        child.poll.return_value = None
        with patch.object(crash.subprocess, 'Popen', return_value=child):
            result = crash.run_child('read_confined', ['probe'], [])
        self.assertTrue(result['timeout'])
        self.assertEqual(result['category'], 'timeout')
        child.kill.assert_called_once()
        self.assertEqual(child.wait.call_count, 2)

    def test_child_launch_error_does_not_leak(self):
        with patch.object(crash.subprocess, 'Popen', side_effect=OSError('private-path')):
            result = crash.run_child('read_confined', ['probe'], [])
        self.assertEqual(result['category'], 'launch_failed')
        self.assertNotIn('private-path', json.dumps(result))

    def test_collect_abort_context_and_no_pid_export(self):
        fixture = self.root / 'fixture'
        fixture.mkdir()
        report = {'ok': False}
        cases = [{'id': 'read_control', 'pid': 122, 'category': 'completed', 'exit_code': 0, 'signal': None},
                 {'id': 'read_confined', 'pid': 123, 'category': 'completed', 'exit_code': -6, 'signal': 6}]
        with patch.object(crash.base, 'build_probe', return_value=None) as build, \
                patch.object(crash, 'run_child', side_effect=cases), \
                patch.object(crash, 'snapshot', return_value=(set(), 'available')), \
                patch.object(crash, 'wait_context', return_value={'category': 'matched', 'candidate_count': 1}) as wait:
            crash.collect(fixture, report)
        probe = build.call_args.args[0]
        self.assertEqual(len(probe.name), 11)
        self.assertTrue(probe.name.startswith('mp-'))
        self.assertEqual(wait.call_args.args[3], 123)
        self.assertEqual(wait.call_args.args[4], probe)
        self.assertFalse(report['ok'])
        self.assertTrue(all('pid' not in case for case in report['cases']))

    def test_report_emitted_before_cleanup_on_abort(self):
        output = self.root / 'report.json'
        fixture = self.root / 'fixture'
        fixture.mkdir()
        @contextlib.contextmanager
        def owned_fixture(**kwargs):
            self.assertEqual(kwargs['dir'], '/private/tmp')
            try:
                yield str(fixture)
            finally:
                self.assertTrue(output.exists())
                shutil.rmtree(fixture)
        def collect(root, report):
            report['cases'] = [{'exit_code': -6, 'signal': 6}]
            report['context'] = {'category': 'matched', 'candidate_count': 1}
        with patch.object(crash.platform, 'system', return_value='Darwin'), \
                patch.object(crash.os, 'geteuid', return_value=1000), \
                patch.object(crash.tempfile, 'TemporaryDirectory', side_effect=owned_fixture), \
                patch.object(crash, 'collect', side_effect=collect):
            self.assertEqual(crash.diagnose(output), 1)
        self.assertFalse(fixture.exists())
        self.assertEqual(output.stat().st_mode & 0o777, 0o600)
        self.assertEqual(json.loads(output.read_text())['cases'][0]['signal'], 6)

    def test_report_alias_into_fixture_is_rejected(self):
        fixture = self.root / 'fixture'
        fixture.mkdir()
        alias = self.root / 'alias'
        alias.symlink_to(fixture)
        @contextlib.contextmanager
        def owned_fixture(**kwargs):
            yield str(fixture)
        with patch.object(crash.platform, 'system', return_value='Darwin'), \
                patch.object(crash.os, 'geteuid', return_value=1000), \
                patch.object(crash.tempfile, 'TemporaryDirectory', side_effect=owned_fixture), \
                self.assertRaisesRegex(ValueError, 'report_outside_fixture_required'):
            crash.diagnose(alias / 'report.json')
        self.assertFalse((fixture / 'report.json').exists())

    def test_refusals_and_report_failure_fixed(self):
        for system, uid, version, category in [('Linux', 1000, (3, 11), 'darwin_required'),
                ('Darwin', 0, (3, 11), 'ordinary_user_required'),
                ('Darwin', 1000, (3, 9), 'python_too_old')]:
            output = self.root / category
            with patch.object(crash.platform, 'system', return_value=system), \
                    patch.object(crash.os, 'geteuid', return_value=uid), \
                    patch.object(crash.sys, 'version_info', version):
                self.assertEqual(crash.diagnose(output), 1)
            self.assertEqual(json.loads(output.read_text())['failure']['category'], category)
        with contextlib.redirect_stderr(io.StringIO()) as stderr:
            self.assertEqual(crash.main(['--output', str(self.root / 'missing' / 'report')]), 1)
        self.assertEqual(stderr.getvalue(), 'marmish crash diagnostic: report_or_cleanup_failed\n')


if __name__ == '__main__':
    unittest.main()
