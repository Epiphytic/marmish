"""Hermetic ownership/selection tests; no real macOS reports or processes."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import uuid
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
        self.identity = (uuid.UUID('1d4ddccf-a180-4bdc-a57d-f87c54a8b7ab'), 'arm64')
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
        return crash.read_context(self.fd, self.name, 123, self.probe, [], self.identity)

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
            self.assertEqual(self.read()['category'], category)

    def test_only_new_matching_names_selected(self):
        for name in (self.name, 'sandbox-exec-2026.ips', 'other.ips', 'mp-12345678.txt'):
            (self.reports / name).touch()
        names, category = crash.snapshot(self.reports, self.probe.name)
        self.assertEqual(category, 'available')
        self.assertEqual(names, {self.name, 'sandbox-exec-2026.ips'})
        with patch.object(crash.time, 'monotonic', side_effect=[0, 21]):
            context = crash.wait_context(self.reports, self.probe.name, names, 123, self.probe, [], self.identity)
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
        facts = crash.extract_crash_facts(payload, [(private, '<fixture>')])
        self.assertNotIn('/Users', json.dumps(facts))
        self.assertNotIn('secret/path', json.dumps(facts))
        self.assertLessEqual(len(facts['termination_details']), 1024)
        self.assertEqual(len(facts['asi']), 6)
        self.assertTrue(all(len(item['value']) <= 1024 for item in facts['asi']))
        self.assertEqual(len(facts['frames']), 24)
        self.assertTrue(all(len(item['symbol']) <= 256 for item in facts['frames']))
        self.assertEqual(crash.bounded('x' * 65537, [], 256), '<oversized>')
        self.assertEqual(crash.extract_crash_facts({'asi': {'unlisted': ['private']}}, [])['asi'], [])

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
            context = crash.wait_context(missing, self.probe.name, set(), 123, self.probe, [], self.identity)
        self.assertEqual(context['category'], 'matched')
        self.assertEqual(context['candidate_count'], 1)
        sleep.assert_called_once_with(1)

    def test_candidate_limit_and_timeout_categories(self):
        for index in range(9):
            (self.reports / ('mp-12345678_%d.ips' % index)).touch()
        context = crash.wait_context(self.reports, self.probe.name, set(), 123, self.probe, [], self.identity)
        self.assertEqual(context, {'category': 'candidate_limit', 'candidate_count': 9})
        with patch.object(crash.time, 'monotonic', side_effect=[0, 21]):
            context = crash.wait_context(self.root / 'missing', self.probe.name, set(), 123, self.probe, [], self.identity)
        self.assertEqual(context, {'category': 'reports_dir_absent', 'candidate_count': 0})

    def test_rejected_candidates_never_mix_shape_and_category(self):
        self.report(procPath='/bin/false')
        self.name = 'sandbox-exec-later.ips'
        self.report(pid=124)
        with patch.object(crash.time, 'monotonic', side_effect=[0, 21]):
            result = crash.wait_context(self.reports, self.probe.name, set(), 123,
                                        self.probe, [], self.identity)
        self.assertEqual(result, {'category': 'candidate_pid_rejected', 'candidate_count': 2})

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
                patch.object(crash, 'read_owned_uuid', return_value=self.identity), \
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

    def macho(self, commands=None, **header):
        if commands is None:
            commands = [struct.pack('<2I', 0x1b, 24) + self.identity[0].bytes]
        defaults = {'magic': 0xfeedfacf, 'cpu': 0x0100000c, 'subtype': 0,
                    'count': len(commands), 'size': sum(map(len, commands))}
        defaults.update(header)
        return struct.pack('<8I', defaults['magic'], defaults['cpu'], defaults['subtype'], 2,
                           defaults['count'], defaults['size'], 0, 0) + b''.join(commands)

    def test_macho_uuid_exact_and_cpu_subtype_ignored(self):
        self.assertEqual(crash.parse_macho_uuid(self.macho(subtype=2), 'arm64'), self.identity)
        x64 = crash.parse_macho_uuid(self.macho(cpu=0x01000007), 'x86_64')
        self.assertEqual(x64, (self.identity[0], 'x86_64'))
        self.probe.write_bytes(self.macho())
        with patch.object(crash.platform, 'machine', return_value='arm64'):
            self.assertEqual(crash.read_owned_uuid(self.probe), self.identity)

    def test_macho_invalid_formats_commands_and_uuid(self):
        command = struct.pack('<2I', 0x1b, 24) + self.identity[0].bytes
        cases = [b'', self.macho() + bytes(4096), self.macho(magic=0xcafebabe), self.macho(magic=0xcffaedfe),
                 self.macho(cpu=0x01000007), self.macho(count=129), self.macho(size=4096),
                 self.macho(size=0), self.macho(count=2), self.macho(commands=[]),
                 self.macho(commands=[command, command]),
                 self.macho(commands=[struct.pack('<2I', 0x1b, 16) + b'0' * 8]),
                 self.macho(commands=[struct.pack('<2I', 0x1b, 24) + bytes(16)]),
                 self.macho(commands=[struct.pack('<2I', 0x1b, 8)]),
                 self.macho(commands=[struct.pack('<2I', 1, 0)])]
        for raw in cases:
            with self.subTest(length=len(raw)), self.assertRaises(ValueError):
                crash.parse_macho_uuid(raw, 'arm64')
        with self.assertRaises(ValueError):
            crash.parse_macho_uuid(self.macho(), 'unsupported')
        self.probe.write_bytes(self.macho())
        alias = self.root / 'probe-alias'
        alias.symlink_to(self.probe)
        with self.assertRaises(OSError):
            crash.read_owned_uuid(alias)
        with patch.object(crash.os, 'getuid', return_value=os.getuid() + 1), self.assertRaises(ValueError):
            crash.read_owned_uuid(self.probe)

    def image(self, **changes):
        value = {'name': self.probe.name, 'arch': 'arm64', 'uuid': str(self.identity[0]).upper(),
                 'path': '/private/tmp/fixture/control/' + self.probe.name}
        value.update(changes)
        return value

    def test_exact_alias_and_four_redacted_shapes(self):
        self.probe.touch()
        alias = self.root / 'probe-alias'
        alias.symlink_to(self.probe)
        for value, expected in [(str(self.probe), 'probe_exact'), (str(alias), 'probe_alias'),
                                ('/usr/bin/sandbox-exec', 'sandbox_exec_exact')]:
            attribution, _ = crash.attribute_executable({'procPath': value}, self.probe, self.identity)
            self.assertEqual(attribution, expected)
        for prefix in ('/private/tmp/*/', '/tmp/*/', '/private/tmp/*/control/', '/tmp/*/control/'):
            payload = {'procPath': prefix + self.probe.name, 'procName': self.probe.name,
                       'usedImages': [self.image()]}
            attribution, shape = crash.attribute_executable(payload, self.probe, self.identity)
            self.assertEqual(attribution, 'probe_uuid_redacted')
            self.assertEqual(shape['star_component_count'], 1)
            self.assertTrue(shape['uuid_image_match'])
            self.assertTrue(shape['proc_name_match'])
            self.assertNotIn(str(self.identity[0]), json.dumps(shape).lower())

    def test_redacted_negative_uuid_name_arch_and_image_identity(self):
        template = {'procPath': '/private/tmp/*/control/' + self.probe.name,
                    'procName': self.probe.name, 'usedImages': [self.image()]}
        changes = [{'procName': 'different'}, {'usedImages': []},
                   {'usedImages': [self.image(uuid=None)]}, {'usedImages': [self.image(uuid='invalid')]},
                   {'usedImages': [self.image(uuid=str(uuid.UUID(int=0)))]},
                   {'usedImages': [self.image(uuid=str(uuid.UUID(int=1)))]},
                   {'usedImages': [self.image(arch='x86_64')]},
                   {'usedImages': [self.image(name='different')]},
                   {'usedImages': [self.image(path='/different')]},
                   {'usedImages': [self.image(), self.image()]}]
        for change in changes:
            with self.subTest(change=change):
                payload = dict(template, **change)
                self.assertIsNone(crash.attribute_executable(payload, self.probe, self.identity)[0])
        for path in ('/private/tmp/*/*/' + self.probe.name, '/tmp/a*/control/' + self.probe.name,
                     '/tmp/*/../' + self.probe.name, '/tmp//*/' + self.probe.name,
                     '/tmp/*/other/' + self.probe.name, '/Users/*/' + self.probe.name,
                     '/tmp/*/' + self.probe.name + '-other', 'relative/' + self.probe.name,
                     '/tmp/*/\x00' + self.probe.name, '/' + 'a' * 4096):
            with self.subTest(path=path):
                self.assertIsNone(crash.attribute_executable(dict(template, procPath=path), self.probe, self.identity)[0])

    def test_path_shape_capped_enums_and_private_report(self):
        payload = {'procPath': '/Users/' + '*/' * 12 + self.probe.name,
                   'procName': self.probe.name, 'usedImages': [self.image()]}
        attribution, shape = crash.attribute_executable(payload, self.probe, self.identity)
        self.assertIsNone(attribution)
        self.assertEqual(shape['prefix'], 'users')
        self.assertEqual(shape['component_count'], 8)
        self.assertEqual(shape['star_component_count'], 8)
        self.assertNotIn('Users', json.dumps(shape))
        self.report(procPath='/private/tmp/*/control/' + self.probe.name,
                    procName=self.probe.name, usedImages=[self.image()])
        context = self.read()
        self.assertEqual(context['attribution'], 'probe_uuid_redacted')
        self.assertEqual(context['category'], 'matched')
        for private in (str(self.probe), str(self.identity[0])):
            self.assertNotIn(private.lower(), json.dumps(context).lower())

    def test_uuid_failure_is_fatal_before_children(self):
        fixture = self.root / 'uuid-fixture'
        fixture.mkdir()
        report = {}
        with patch.object(crash.base, 'build_probe', return_value=None), \
                patch.object(crash, 'read_owned_uuid', side_effect=ValueError('private-uuid')), \
                patch.object(crash, 'run_child') as child:
            crash.collect(fixture, report)
        child.assert_not_called()
        self.assertEqual(report['failure'], {'category': 'probe_uuid_invalid', 'exit_code': None})
        self.assertNotIn('private-uuid', json.dumps(report))

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
