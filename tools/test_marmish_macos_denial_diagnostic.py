"""Offline finite-format and owned-process contracts; never queries unified logs."""
from contextlib import ExitStack, redirect_stderr, redirect_stdout
from datetime import datetime, timedelta, timezone
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import uuid

import marmish_macos_denial_diagnostic as diagnostic

SECRET = 'SECRET-host-private-path-9dd9caf8'
NAME = 'mp-a123b456'
PID = 1234
START = datetime(2026, 10, 30, 12, 0, 0, tzinfo=timezone.utc)
WINDOW = (START, START + timedelta(seconds=2))


def record(message=None, **changes):
    value = {'timestamp': '2026-10-30 12:00:01+0000', 'processImagePath': '/kernel',
             'eventMessage': message or 'Sandbox: ' + NAME + '(' + str(PID) + ') deny(1) file-map-executable /usr/lib/dyld',
             'ignored': SECRET}
    value.update(changes)
    return value


def parse(values):
    raw = values if isinstance(values, bytes) else json.dumps(values).encode()
    return diagnostic.parse_denials(raw, NAME, PID, WINDOW)


class CollectionTests(unittest.TestCase):
    def assert_rejected(self, value, category='format_unknown'):
        result = parse(value)
        self.assertEqual(result['category'], category)
        self.assertEqual(result['denials'], [])
        self.assertIsNone(result['matched_count'])
        self.assertNotIn(SECRET, json.dumps(result))
        return result

    def test_valid_known_unknown_and_unclassified_are_finite(self):
        values = [record(), record(), record('Sandbox: ' + NAME + '(1234) deny(2) mach-lookup ' + SECRET),
                  record('Sandbox: ' + NAME + '(1234) deny(3) private-operation ' + SECRET)]
        result = parse(values)
        self.assertEqual(result['category'], 'operation_unknown')
        self.assertEqual(result['matched_count'], 3)
        self.assertEqual(result['denials'], [
            {'operation': 'file-map-executable', 'resource_class': 'system_dyld'},
            {'operation': 'mach-lookup', 'resource_class': 'unclassified'},
            {'operation': 'unknown', 'resource_class': 'unclassified'}])
        encoded = json.dumps(result)
        for forbidden in (SECRET, NAME, '1234', '/usr/lib/dyld', 'private-operation'):
            self.assertNotIn(forbidden, encoded)
        self.assertEqual(parse([record()])['category'], 'owned_denial_found')
        self.assertEqual(parse([])['category'], 'no_owned_denial')
        self.assertEqual(parse([])['matched_count'], 0)

    def test_each_unattributed_record_invalidates_whole_collection(self):
        changes = [
            {'eventMessage': 'Sandbox: mp-a123b457(1234) deny(1) file-map-executable ' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(12345) deny(1) mach-lookup ' + SECRET},
            {'eventMessage': 'Sandbox: x' + NAME + '(1234) deny(1) mach-lookup ' + SECRET},
            {'eventMessage': 'Sandbox: sandbox-exec(1234) deny(1) mach-lookup ' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(1234) deny(0) mach-lookup ' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(1234) deny(01) mach-lookup ' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(1234) deny(1234567890) mach-lookup ' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(1234) deny(1) mach-lookup\n' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(1234) deny(1) mach-lookup \x00' + SECRET},
            {'eventMessage': 'Sandbox: ' + NAME + '(1234) deny(1) mach-lookup é' + SECRET},
            {'eventMessage': 'prefix Sandbox: ' + NAME + '(1234) deny(1) mach-lookup ' + SECRET},
            {'processImagePath': '/usr/libexec/sandboxd'},
            {'processImagePath': '/kernel/' + SECRET},
            {'timestamp': '2026-10-30 11:59:59+0000'},
            {'timestamp': '2026-10-30 12:00:03+0000'},
            {'timestamp': '2026-10-30 12:00:01Z'},
            {'timestamp': '2026-10-30 12:00:01 UTC'},
            {'timestamp': '2026-10-30 12:00:60+0000'},
            {'timestamp': '2026-10-30 12:00:01.1234567+0000'},
        ]
        for change in changes:
            with self.subTest(change=change):
                self.assert_rejected([record(), record(**change)])
        # Name and PID split over two records do not combine into a match.
        self.assert_rejected([record('Sandbox: ' + NAME + '(1235) deny(1) mach-lookup ' + SECRET),
                              record('Sandbox: mp-a123b457(1234) deny(1) mach-lookup ' + SECRET)])

    def test_field_bounds_missing_types_and_extra_keys(self):
        for key, cap in (('timestamp', 64), ('processImagePath', 4096), ('eventMessage', 8192)):
            for invalid in (None, 3, {}, [SECRET], SECRET + 'x' * cap):
                with self.subTest(key=key, invalid_type=type(invalid)):
                    self.assert_rejected([record(**{key: invalid})])
            missing = record()
            missing.pop(key)
            self.assert_rejected([missing])
        self.assertEqual(parse([record(extra={'secret': SECRET})])['category'], 'owned_denial_found')

    def test_strict_decoding_shapes_and_duplicate_keys(self):
        values = [b'', b'\xff' + SECRET.encode(), b'{}', b'null', b'NaN', b'Infinity',
            b'[{"timestamp":"x","timestamp":"' + SECRET.encode() + b'"}]',
            json.dumps(record()).encode() + b'\n' + json.dumps(record()).encode(),
            b'[' * 40 + json.dumps(SECRET).encode() + b']' * 40,
            json.dumps([record()]).encode() + b'\n{"count":1,"finished":true}',
            ('notice ' + SECRET + '\n[]').encode()]
        for value in values:
            with self.subTest(value_prefix=value[:20]):
                self.assert_rejected(value)
        shape = parse(json.dumps([record()]).encode() + b'\n{"count":1,"finished":true}')['format_shape']
        self.assertTrue(shape['trailing_summary'])
        self.assertEqual(shape['top_level'], 'array')
        self.assertTrue(parse(('notice ' + SECRET).encode())['format_shape']['leading_text'])
        self.assertEqual(parse(b'\xff')['format_shape'], diagnostic.shape_default())
        with patch.object(diagnostic.json.JSONDecoder, 'raw_decode', side_effect=RecursionError):
            self.assert_rejected(b'[]')

    def test_caps_and_precedence(self):
        self.assert_rejected(b'x' * (diagnostic.OUT_CAP + 1), 'query_output_limit')
        self.assert_rejected([record()] * 129, 'record_limit')
        operations = sorted(diagnostic.OPERATIONS)[:9]
        values = [record('Sandbox: ' + NAME + '(1234) deny(1) ' + op + ' ' + SECRET) for op in operations]
        self.assert_rejected(values, 'matching_limit')
        self.assert_rejected(values + [record(eventMessage=SECRET)])

    def test_resource_classifiers_are_exact(self):
        for target, expected in diagnostic.RESOURCES.items():
            self.assertEqual(diagnostic.resource_class(target), expected)
            self.assertEqual(diagnostic.resource_class(target + '/'), 'unclassified')
        for prefix in ('/System/Volumes/Preboot/Cryptexes/OS', '/System/Cryptexes/OS', '/private/preboot/Cryptexes/OS'):
            for architecture in ('arm64e', 'x86_64'):
                target = prefix + '/System/Library/dyld/dyld_shared_cache_' + architecture
                self.assertEqual(diagnostic.resource_class(target), 'system_cryptex_shared_cache')
                self.assertEqual(diagnostic.resource_class(target + '.001'), 'system_cryptex_shared_cache')
                for suffix in ('.1234', '.x', '/private', '/../' + SECRET):
                    self.assertEqual(diagnostic.resource_class(target + suffix), 'unclassified')
        self.assertEqual(diagnostic.resource_class(None), 'none')
        self.assertEqual(diagnostic.resource_class(SECRET), 'unclassified')

    def test_timestamp_offsets_boundaries_and_window_rollback(self):
        for value in ('2026-10-30T12:00:00+00:00', '2026-10-30 12:00:02.000000+0000',
                      '2026-10-30 13:00:01+0100', '2026-10-30 11:00:01-01:00'):
            self.assertEqual(parse([record(timestamp=value)])['category'], 'owned_denial_found')
        window = diagnostic.query_window(START + timedelta(microseconds=1),
            START + timedelta(seconds=1, microseconds=1), 1)
        self.assertEqual(window, WINDOW)
        for end, elapsed in ((START - timedelta(seconds=1), 1), (START + timedelta(seconds=8), 8),
                             (START + timedelta(seconds=2), 0)):
            with self.assertRaises(ValueError):
                diagnostic.query_window(START, end, elapsed)

    def test_query_argv_is_fixed_and_identity_validated(self):
        argv = diagnostic.query_command(NAME, PID, WINDOW)
        self.assertEqual(argv[:4], ['/usr/bin/log', 'show', '--style', 'json'])
        self.assertEqual(argv[5], '2026-10-30 12:00:00+0000')
        self.assertEqual(argv[7], '2026-10-30 12:00:02+0000')
        self.assertIn('process == "kernel"', argv[9])
        self.assertIn(NAME + '(1234) deny(', argv[9])
        for name, pid in ((NAME + 'x', PID), ('mp-$(id)', PID), (NAME, 0), (NAME, 2 ** 31), (NAME, True)):
            with self.assertRaises(ValueError):
                diagnostic.query_command(name, pid, WINDOW)


class ProcessTests(unittest.TestCase):
    def test_real_success_timeout_output_limit_and_private_capture(self):
        with tempfile.TemporaryDirectory() as root:
            result = diagnostic.run_private([sys.executable, '-c', 'print("owned")'], root)
            self.assertEqual(result['exit_code'], 0)
            self.assertEqual(result['stdout'], b'owned\n')
            timeout = diagnostic.run_private([sys.executable, '-c', 'import time; time.sleep(10)'], root, timeout=0.1)
            self.assertEqual(timeout['category'], 'timeout')
            self.assertEqual(timeout['signal'], signal.SIGKILL)
            overflow = diagnostic.run_private([sys.executable, '-c', 'print("x" * 100000)'], root, stdout_cap=100)
            self.assertEqual(overflow['category'], 'output_limit')
            self.assertLessEqual(len(overflow['stdout']), 101)
            stderr = diagnostic.run_private([sys.executable, '-c', 'import sys; sys.stderr.write("x" * 10000)'], root)
            self.assertEqual(stderr['category'], 'output_limit')
            self.assertLessEqual(len(stderr['stderr']), diagnostic.ERR_CAP + 1)
            self.assertEqual(list(Path(root).iterdir()), [])

    def test_group_kill_precedes_leader_reap_and_capture_is_0600(self):
        events = []
        real_popen, real_kill = subprocess.Popen, os.killpg
        def spawn(*args, **kwargs):
            self.assertTrue(kwargs['start_new_session'])
            self.assertTrue(kwargs['close_fds'])
            self.assertEqual(kwargs['env'], {'PATH': '/usr/bin:/bin'})
            self.assertEqual(os.fstat(kwargs['stderr'].fileno()).st_mode & 0o777, 0o600)
            child = real_popen(*args, **kwargs)
            wait = child.wait
            def reap(*a, **kw):
                events.append('reap')
                return wait(*a, **kw)
            child.wait = reap
            return child
        def kill(*args):
            events.append('kill')
            return real_kill(*args)
        with tempfile.TemporaryDirectory() as root, patch.object(diagnostic.subprocess, 'Popen', side_effect=spawn), patch.object(diagnostic.os, 'killpg', side_effect=kill):
            diagnostic.run_private(['/usr/bin/true'], root)
        self.assertEqual(events, ['kill', 'reap'])

    def test_interruption_and_observation_failure_cleanup_owned_group(self):
        real_popen = subprocess.Popen
        children = []
        def spawn(*args, **kwargs):
            child = real_popen(*args, **kwargs)
            children.append(child)
            return child
        real_sleep = time.sleep
        interrupts = []
        def interrupt_once(delay):
            if not interrupts:
                interrupts.append(True)
                raise KeyboardInterrupt
            return real_sleep(delay)
        with tempfile.TemporaryDirectory() as root, patch.object(diagnostic.subprocess, 'Popen', side_effect=spawn):
            with patch.object(diagnostic.time, 'sleep', side_effect=interrupt_once):
                with self.assertRaises(KeyboardInterrupt):
                    diagnostic.run_private([sys.executable, '-c', 'import time; time.sleep(10)'], root)
            self.assertEqual(children[-1].returncode, -signal.SIGKILL)
            with self.assertRaises(ProcessLookupError):
                os.killpg(children[-1].pid, 0)
            with patch.object(diagnostic, 'observe', side_effect=OSError(22, SECRET)):
                result = diagnostic.run_private(['/usr/bin/true'], root)
            self.assertEqual(result['category'], 'observation_failed')
            self.assertIsNotNone(children[-1].returncode)

    def test_query_failures_never_export_captured_private_text(self):
        fixture = {'exit_code': 1, 'signal': None, 'category': 'completed',
                   'stdout': SECRET.encode(), 'stderr': b'permission denied ' + SECRET.encode()}
        categories = [('completed', 1, 'query_unavailable'), ('output_limit', 0, 'query_output_limit'),
                      ('timeout', None, 'query_timeout'), ('observation_failed', None, 'query_failed')]
        for category, code, expected in categories:
            private = {**fixture, 'category': category, 'exit_code': code}
            with patch.object(diagnostic, 'run_private', return_value=private):
                result = diagnostic.query_logs(Path('/unused'), NAME, PID, WINDOW)
            self.assertEqual(result['category'], expected)
            self.assertIsNone(result['matched_count'])
            self.assertEqual(result['denials'], [])
            self.assertNotIn(SECRET, json.dumps(result))

    @unittest.skipUnless(sys.platform == 'linux', 'Linux /proc distinguishes dead descendants from zombies')
    def test_descendant_process_is_stopped_with_its_owned_group(self):
        script = ('import os,time; child=os.fork(); '
                  'print(child, flush=True) if child else None; time.sleep(10)')
        with tempfile.TemporaryDirectory() as root:
            result = diagnostic.run_private([sys.executable, '-c', script], root, timeout=0.2)
        self.assertEqual(result['category'], 'timeout')
        descendant = int(result['stdout'])
        self.assertGreater(descendant, 0)
        # A descendant is not ours to reap. On Linux a dead, unreaped descendant
        # can remain a zombie; that does not mean it survived the group signal.
        status = Path('/proc') / str(descendant) / 'stat'
        if sys.platform == 'linux' and status.exists():
            self.assertEqual(status.read_text().split(') ', 1)[1][0], 'Z')
        else:
            with self.assertRaises(ProcessLookupError):
                os.kill(descendant, 0)

    def test_preflight_unsupported_and_query_time_failure_are_distinct(self):
        with tempfile.TemporaryDirectory() as root:
            self.assertTrue(diagnostic.observation_supported(root))
            with patch.object(diagnostic, 'observe', side_effect=NotImplementedError):
                self.assertFalse(diagnostic.observation_supported(root))
            with patch.object(diagnostic, 'observe', side_effect=NotImplementedError), patch.object(diagnostic, 'query_command', return_value=['/usr/bin/true']):
                result = diagnostic.query_logs(root, NAME, PID, WINDOW)
                # Only a benign synthetic child is ever launched by this test.
                self.assertEqual(result['category'], 'query_failed')

    def test_failed_group_signal_still_reaps_and_reports_cleanup_failure(self):
        real_popen = subprocess.Popen
        children = []
        def spawn(*args, **kwargs):
            child = real_popen(*args, **kwargs)
            children.append(child)
            return child
        with tempfile.TemporaryDirectory() as root, patch.object(diagnostic.subprocess, 'Popen', side_effect=spawn), patch.object(diagnostic.os, 'killpg', side_effect=PermissionError(SECRET)):
            with self.assertRaises(diagnostic.CleanupFailed):
                diagnostic.run_private(['/usr/bin/true'], root)
        self.assertEqual(children[0].returncode, 0)

    def test_reaping_observation_is_unsupported_and_never_signals_reused_group(self):
        def reaping_observe(child):
            pid, status = os.waitpid(child.pid, os.WNOHANG)
            if pid == 0:
                return None
            return SimpleNamespace(si_pid=pid,
                si_code=os.CLD_EXITED if os.WIFEXITED(status) else os.CLD_KILLED,
                si_status=os.WEXITSTATUS(status) if os.WIFEXITED(status) else os.WTERMSIG(status))
        with tempfile.TemporaryDirectory() as root, patch.object(diagnostic, 'observe', side_effect=reaping_observe), patch.object(diagnostic.os, 'killpg') as kill:
            self.assertFalse(diagnostic.observation_supported(root))
            result = diagnostic.run_private(['/usr/bin/false'], root)
            self.assertEqual(result['category'], 'observation_failed')
            self.assertIsNone(result['exit_code'])
            report = {'log': diagnostic.log_result(), 'failure': None, 'ok': False}
            with patch.object(diagnostic.base, 'build_probe') as build:
                diagnostic.collect(Path(root), report)
            self.assertEqual(report['failure']['category'], 'query_observation_unsupported')
            self.assertEqual(report['log']['category'], 'not_queried')
            build.assert_not_called()
            kill.assert_not_called()

    def test_repeat_observation_identity_status_and_final_wait_must_match(self):
        real_observe, real_popen = diagnostic.observe, subprocess.Popen
        for field in ('si_pid', 'si_status', 'si_code'):
            observations = []
            def changed_second(child):
                status = real_observe(child)
                if status is None:
                    return None
                observations.append(status)
                if len(observations) == 1:
                    return status
                result = SimpleNamespace(si_pid=status.si_pid, si_status=status.si_status, si_code=status.si_code)
                setattr(result, field, getattr(result, field) + 1)
                return result
            with self.subTest(field=field), tempfile.TemporaryDirectory() as root, patch.object(diagnostic, 'observe', side_effect=changed_second), patch.object(diagnostic.os, 'killpg') as kill:
                result = diagnostic.run_private(['/usr/bin/false'], root)
                self.assertEqual(result['category'], 'observation_failed')
                self.assertIsNone(result['exit_code'])
                kill.assert_not_called()
        def wrong_final_status(*args, **kwargs):
            child = real_popen(*args, **kwargs)
            wait = child.wait
            def wrong_wait(*a, **kw):
                wait(*a, **kw)
                return 0
            child.wait = wrong_wait
            return child
        with tempfile.TemporaryDirectory() as root, patch.object(diagnostic.subprocess, 'Popen', side_effect=wrong_final_status):
            result = diagnostic.run_private(['/usr/bin/false'], root)
        self.assertEqual(result['category'], 'observation_failed')
        self.assertIsNone(result['exit_code'])


def private_result(code, pid=PID, start=START):
    return {'exit_code': code, 'signal': -code if code < 0 else None, 'category': 'completed',
            'pid': pid, 'start': start, 'end': start + timedelta(seconds=1), 'elapsed': 1,
            'stdout': b'', 'stderr': SECRET.encode()}


class GateTests(unittest.TestCase):
    def collect_report(self, context=None, control=0, confined=-6, clock_bad=False, supported=True):
        report = {'log': diagnostic.log_result(), 'failure': None, 'ok': False}
        def fixture(root, _):
            home, directory = root / 'home', root / 'control'
            home.mkdir()
            directory.mkdir()
            profile = directory / 'profile.sb'
            profile.write_text('owned profile')
            return SimpleNamespace(_directory=directory, _command=lambda *a: ['sandbox-exec', *map(str, a)]), profile
        selected = context or {'category': 'matched', 'executable': 'probe',
                              'attribution': 'probe_uuid_redacted', 'path_shape': {'uuid_image_match': True}}
        private = private_result(confined)
        if clock_bad:
            private['end'] = START - timedelta(seconds=1)
        identity = (uuid.uuid4(), 'arm64')
        with tempfile.TemporaryDirectory() as root, ExitStack() as stack:
            stack.enter_context(patch.object(diagnostic, 'observation_supported', return_value=supported))
            stack.enter_context(patch.object(diagnostic.base, 'make_fixture', side_effect=fixture))
            build = stack.enter_context(patch.object(diagnostic.base, 'build_probe', return_value=None))
            stack.enter_context(patch.object(diagnostic.crash, 'read_owned_uuid', return_value=identity))
            stack.enter_context(patch.object(diagnostic.crash, 'snapshot', return_value=(set(), 'available')))
            context_call = stack.enter_context(patch.object(diagnostic.crash, 'wait_context', return_value=selected))
            stack.enter_context(patch.object(diagnostic.base, 'replacements_for', return_value=[]))
            stack.enter_context(patch.object(diagnostic, 'run_private', side_effect=[private_result(control, pid=PID + 77, start=START - timedelta(seconds=60)), private]))
            query = stack.enter_context(patch.object(diagnostic, 'query_logs', return_value=parse([record()])))
            diagnostic.collect(Path(root), report)
            if context_call.called:
                args = context_call.call_args.args
                self.assertEqual(args[0], Path.home() / 'Library/Logs/DiagnosticReports')
                self.assertEqual(args[2:4], (set(), PID))
                self.assertEqual(args[4], Path(root) / 'control' / args[1])
                self.assertEqual(args[5:], ([], identity))
            if query.called:
                self.assertEqual(query.call_args.args, (Path(root), context_call.call_args.args[1],
                    PID, (START, START + timedelta(seconds=1))))
        self.assertNotIn(SECRET, json.dumps(report))
        return report, query.call_count, build.call_count

    def test_uuid_attribution_and_execution_gates(self):
        contexts = [
            {'category': 'matched', 'executable': 'sandbox_exec', 'attribution': 'sandbox_exec_exact', 'path_shape': {'uuid_image_match': True}},
            {'category': 'matched', 'executable': 'probe', 'attribution': 'probe_exact', 'path_shape': {'uuid_image_match': False}},
            {'category': 'matched', 'executable': 'probe', 'attribution': 'probe_alias', 'path_shape': {}},
            {'category': 'context_timed_out'},
        ]
        for context in contexts:
            with self.subTest(context=context):
                report, calls, _ = self.collect_report(context=context)
                self.assertEqual(report['log']['category'], 'owned_crash_unavailable')
                self.assertEqual(calls, 0)
        for kwargs, expected in (({'control': 2}, 'control_invalid'), ({'confined': 2}, 'not_sigabrt'),
                                 ({'clock_bad': True}, 'clock_window_invalid')):
            report, calls, _ = self.collect_report(**kwargs)
            self.assertEqual(report['log']['category'], expected)
            self.assertEqual(calls, 0)
        report, calls, builds = self.collect_report(supported=False)
        self.assertEqual(report['failure']['category'], 'query_observation_unsupported')
        self.assertEqual((calls, builds), (0, 0))
        self.assertEqual(report['log']['category'], 'not_queried')

    def test_denial_candidate_keeps_abort_failure_and_pass_is_not_full_matrix(self):
        report, calls, _ = self.collect_report()
        self.assertEqual(calls, 1)
        self.assertEqual(report['log']['category'], 'owned_denial_found')
        self.assertFalse(report['ok'])
        self.assertEqual(report['cases'][1]['signal'], 6)
        passed, calls, _ = self.collect_report(confined=0)
        self.assertEqual(calls, 0)
        self.assertEqual(passed['log']['category'], 'not_sigabrt')
        self.assertFalse(passed['ok'])

    def test_platform_root_python_refuse_before_fixture_or_query(self):
        for system, uid, version, category in [('Linux', 1000, (3, 11), 'darwin_required'),
                ('Darwin', 0, (3, 11), 'ordinary_user_required'), ('Darwin', 1000, (3, 10), 'python_too_old')]:
            with tempfile.TemporaryDirectory() as root, patch.object(diagnostic.platform, 'system', return_value=system), patch.object(diagnostic.os, 'geteuid', return_value=uid), patch.object(diagnostic.sys, 'version_info', version), patch.object(diagnostic, 'collect') as collect:
                output = Path(root) / 'report.json'
                self.assertEqual(diagnostic.diagnose(output), 1)
                self.assertEqual(json.loads(output.read_text())['failure']['category'], category)
                collect.assert_not_called()
                self.assertEqual(output.stat().st_mode & 0o777, 0o600)
                with self.assertRaises(FileExistsError):
                    diagnostic.diagnose(output)

    def test_fixture_cleanup_and_sanitized_exception_console(self):
        original = tempfile.TemporaryDirectory
        with original() as root:
            fixtures = []
            def temporary(*args, **kwargs):
                value = original(prefix='owned-', dir=root)
                fixtures.append(Path(value.name))
                return value
            with patch.object(diagnostic.platform, 'system', return_value='Darwin'), patch.object(diagnostic.os, 'geteuid', return_value=1000), patch.object(diagnostic.tempfile, 'TemporaryDirectory', side_effect=temporary), patch.object(diagnostic, 'collect', side_effect=ValueError(SECRET)):
                output = Path(root) / 'report.json'
                self.assertEqual(diagnostic.diagnose(output), 1)
            self.assertTrue(all(not fixture.exists() for fixture in fixtures))
            self.assertEqual(json.loads(output.read_text())['failure']['category'], 'fixture_failed')
            stdout, stderr = io.StringIO(), io.StringIO()
            with patch.object(diagnostic, 'diagnose', side_effect=OSError(SECRET)), redirect_stdout(stdout), redirect_stderr(stderr):
                self.assertEqual(diagnostic.main(['--output', str(output)]), 1)
            self.assertNotIn(SECRET, stdout.getvalue() + stderr.getvalue() + output.read_text())

    def test_sigterm_runs_owned_process_and_fixture_cleanup(self):
        original = tempfile.TemporaryDirectory
        real_popen = subprocess.Popen
        children, fixtures = [], []
        previous = signal.getsignal(signal.SIGTERM)
        with original() as root:
            def temporary(*args, **kwargs):
                value = original(prefix='owned-', dir=root)
                fixtures.append(Path(value.name))
                return value
            def spawn(*args, **kwargs):
                child = real_popen(*args, **kwargs)
                children.append(child)
                return child
            def collect(directory, _report):
                command = ('import os,signal,time; time.sleep(0.05); '
                           'os.kill(' + str(os.getpid()) + ',signal.SIGTERM); time.sleep(10)')
                diagnostic.run_private([sys.executable, '-c', command], directory)
            with patch.object(diagnostic.platform, 'system', return_value='Darwin'), patch.object(diagnostic.os, 'geteuid', return_value=1000), patch.object(diagnostic.tempfile, 'TemporaryDirectory', side_effect=temporary), patch.object(diagnostic.subprocess, 'Popen', side_effect=spawn), patch.object(diagnostic, 'collect', side_effect=collect):
                output = Path(root) / 'report.json'
                self.assertEqual(diagnostic.main(['--output', str(output)]), 1)
            self.assertEqual(json.loads(output.read_text())['failure']['category'], 'fixture_failed')
            self.assertTrue(all(not fixture.exists() for fixture in fixtures))
            self.assertEqual(children[0].returncode, -signal.SIGKILL)
            self.assertEqual(signal.getsignal(signal.SIGTERM), previous)

    def test_python_minor_is_finite(self):
        for version, expected in (((3, 13), '3.13'), ((4, 99), 'other'), ((3, 10), 'other')):
            with tempfile.TemporaryDirectory() as root, patch.object(diagnostic.sys, 'version_info', version), patch.object(diagnostic.platform, 'system', return_value='Linux'):
                output = Path(root) / 'report.json'
                diagnostic.diagnose(output)
                self.assertEqual(json.loads(output.read_text())['python_minor'], expected)


if __name__ == '__main__':
    unittest.main()
