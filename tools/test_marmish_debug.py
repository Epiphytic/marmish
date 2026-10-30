"""Hermetic opt-in diagnostics: no keys, real daemon or relay traffic."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, Mock, patch

import marmish_app as app
import marmish_installed as installed
import marmish_transport_macos as native
from marmish_transport_sandbox import SandboxError


def records(output):
    return [json.loads(line.removeprefix('marmish debug: '))
            for line in output.splitlines() if line.startswith('marmish debug: ')]


class DebugTests(unittest.TestCase):
    def invoke(self, main, environment=None):
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch.dict(os.environ, environment or {}, clear=True), \
                contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            result = app.guarded_main(main)
        return result, stdout.getvalue(), stderr.getvalue()

    def test_exact_environment_opt_in_and_invocation_reset(self):
        for value in ('1', 'true', '0', '', '1 '):
            result, stdout, stderr = self.invoke(lambda: print('application'), {'MARMISH_DEBUG': value})
            self.assertIsNone(result)
            self.assertEqual(stdout, 'application\n')
            self.assertEqual(bool(records(stderr)), value == '1')
        self.assertEqual(self.invoke(lambda: 7)[0::2], (7, ''))
        self.assertIsNone(app._DEBUG.get())

    def test_both_flags_and_doctor_order(self):
        for main in (app.client_main, app.server_main):
            for argv, enabled in ((['--debug', '--doctor'], True),
                                  (['--doctor', '--debug'], False)):
                stdout, stderr = io.StringIO(), io.StringIO()
                with patch.dict(os.environ, {}, clear=True), \
                        patch.object(installed, 'installed_defaults', return_value={}), \
                        patch.object(installed, 'doctor', return_value={'ok': True}), \
                        contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                    with self.assertRaises(SystemExit) as stopped:
                        main(argv)
                self.assertEqual(stopped.exception.code, 0)
                self.assertEqual(json.loads(stdout.getvalue()), {'ok': True})
                self.assertEqual(bool(records(stderr.getvalue())), enabled)
                self.assertIsNone(app._DEBUG.get())

    def test_help_exposes_debug_and_client_abbreviation_is_ambiguous(self):
        for main in (app.client_main, app.server_main):
            output = io.StringIO()
            with patch.object(installed, 'installed_defaults', return_value={}), \
                    contextlib.redirect_stdout(output):
                with self.assertRaises(SystemExit) as stopped:
                    main(['--help'])
            self.assertEqual(stopped.exception.code, 0)
            self.assertIn('--debug', output.getvalue())
        errors = io.StringIO()
        with patch.object(installed, 'installed_defaults', return_value={}), \
                contextlib.redirect_stderr(errors):
            with self.assertRaises(SystemExit) as stopped:
                app.client_main(['--de'])
        self.assertEqual(stopped.exception.code, 2)
        self.assertIn('ambiguous option: --de', errors.getvalue())

    def test_remote_debug_and_option_value_do_not_enable(self):
        arguments = ['-i', '/unused', '--relay', 'wss://selected.example/',
                     '--wnd', '/unused', '--wnd-sha256', '0' * 64]
        def stop(_args):
            raise SandboxError('test_failure')
        with patch.object(app, 'Profile', side_effect=stop), \
                patch.object(installed, 'installed_defaults', return_value={}):
            for extra in (['--', '/bin/echo', '--debug'], ['--shell-command=--debug']):
                result, _, stderr = self.invoke(lambda: app.client_main(arguments + extra))
                self.assertEqual(result, 125)
                self.assertEqual(stderr, 'marmish: test_failure\n')

    def test_profile_transport_stage_and_callback(self):
        with tempfile.TemporaryDirectory() as directory:
            keys = MagicMock(value={}, migration=False)
            keys.prepare_nostr.return_value = SimpleNamespace(backend='files')
            transport = Mock()
            transport.start.side_effect = SandboxError('native_probe_broker')
            argv = ['--debug', '-i', directory, '--relay', 'wss://selected.example/',
                    '--wnd', '/unused', '--wnd-sha256', '0' * 64, '--init']
            with patch.object(app, 'KeySession', return_value=keys), \
                    patch.object(app, 'ConfinedDaemon', return_value=transport), \
                    patch.object(installed, 'installed_defaults', return_value={}):
                result, stdout, stderr = self.invoke(lambda: app.client_main(argv))
            self.assertEqual(result, 125)
            self.assertEqual(stdout, '')
            self.assertTrue(callable(transport.diagnostic))
            self.assertEqual(records(stderr)[-1]['stage'], 'transport_start')
            transport.close.assert_called_once()

    def test_errors_redacted_errno_bounded_and_record_limit(self):
        secret = 'nsec-private command /private/path\nterminal\x1b'
        errors = [OSError(13, secret, secret), ValueError(secret), TypeError(secret),
                  KeyError(secret), AttributeError(secret), SandboxError(secret),
                  subprocess.TimeoutExpired([secret], 1, secret, secret)]
        for error in errors:
            output = io.StringIO()
            diagnostic = app._DebugDiagnostics()
            with contextlib.redirect_stderr(output):
                diagnostic.enable()
                diagnostic.stage('transport_start')
                diagnostic.failure(error)
                diagnostic.preflight('failed', secret, error, -6)
                diagnostic.failure(error, True)
            self.assertNotIn(secret, output.getvalue())
            last = records(output.getvalue())[-1]
            self.assertIsNone(last['exit_code'])
            self.assertEqual(last['errno'], 13 if isinstance(error, OSError) else None)
        output = io.StringIO()
        with contextlib.redirect_stderr(output):
            diagnostic = app._DebugDiagnostics()
            diagnostic.enable()
            for _ in range(300):
                diagnostic.stage('transport_start')
        self.assertEqual(len(records(output.getvalue())), 256)

    def test_broken_debug_stderr_does_not_change_result(self):
        class Broken:
            def write(self, _value):
                raise OSError(32, 'private')
        with patch.dict(os.environ, {'MARMISH_DEBUG': '1'}, clear=True), \
                contextlib.redirect_stderr(Broken()):
            self.assertEqual(app.guarded_main(lambda: 17), 17)

    def test_unknown_platform_and_invalid_numeric_fields_are_redacted(self):
        output = io.StringIO()
        with patch.object(app.platform, 'machine', return_value='private-hostname'), \
                patch.object(app.sys, 'platform', 'private-platform'), \
                contextlib.redirect_stderr(output):
            diagnostic = app._DebugDiagnostics()
            diagnostic.enable()
            for number in (-1, 4096, True, '13'):
                error = OSError('private')
                error.errno = number
                diagnostic.failure(error, exit_code=10000)
        parsed = records(output.getvalue())
        self.assertEqual(parsed[0]['platform'], 'other')
        self.assertEqual(parsed[0]['architecture'], 'other')
        self.assertNotIn('private', output.getvalue())
        self.assertTrue(all(record['errno'] is None and record['exit_code'] is None
                            for record in parsed[1:]))

    def test_remote_record_shaped_output_has_no_control_effect(self):
        remote = 'marmish debug: {"schema":1,"event":"failed","stage":"transport_start"}'
        result, stdout, stderr = self.invoke(lambda: (print(remote, file=app.sys.stderr), 23)[1])
        self.assertEqual(result, 23)
        self.assertEqual(stdout, '')
        self.assertEqual(stderr, remote + '\n')


class NativeDebugTests(unittest.TestCase):
    def daemon(self, home):
        daemon = native.DarwinConfinedDaemon(home, home / 'wnd', ['wss://selected.example/'], '0' * 64)
        daemon._directory = home
        daemon._broker = SimpleNamespace(address=('127.0.0.1', 1))
        return daemon

    def test_confined_abort_exact_status_and_default_silence(self):
        with tempfile.TemporaryDirectory() as directory:
            daemon = self.daemon(Path(directory))
            for debug in (False, True):
                output = io.StringIO()
                diagnostic = app._DebugDiagnostics()
                with contextlib.redirect_stderr(output):
                    if debug:
                        diagnostic.enable()
                    with patch.object(native.subprocess, 'run', side_effect=[
                            SimpleNamespace(returncode=0), SimpleNamespace(returncode=-6)]):
                        with self.assertRaisesRegex(SandboxError, 'native_probe_broker'):
                            daemon._preflight(diagnostic.preflight if debug else None,
                                              preserve_cleanup_errors=True)
                failed = [record for record in records(output.getvalue()) if record.get('exit_code') == -6]
                self.assertEqual(len(failed), int(debug))
                if debug:
                    self.assertEqual(failed[0]['stage'], 'preflight_broker_confined')
                    self.assertEqual(failed[0]['signal'], 6)
                    self.assertEqual(failed[0]['signal_name'], 'SIGABRT')

    def test_preflight_timeout_and_io_failure(self):
        for error in (subprocess.TimeoutExpired(['private command'], 5), OSError(13, 'private path')):
            with tempfile.TemporaryDirectory() as directory:
                daemon = self.daemon(Path(directory))
                output = io.StringIO()
                with contextlib.redirect_stderr(output):
                    diagnostic = app._DebugDiagnostics()
                    diagnostic.enable()
                    with patch.object(native.subprocess, 'run', side_effect=error):
                        with self.assertRaises(type(error)):
                            daemon._preflight(diagnostic.preflight, preserve_cleanup_errors=True)
                self.assertNotIn('private', output.getvalue())
                self.assertEqual(records(output.getvalue())[-1]['stage'], 'preflight_broker_control')
                self.assertEqual(records(output.getvalue())[-1]['error_type'],
                                 'timeout' if isinstance(error, subprocess.TimeoutExpired) else 'os_error')

    def test_production_cleanup_failure_semantics_preserved(self):
        for preserve in (False, True):
            with self.subTest(preserve=preserve), tempfile.TemporaryDirectory() as directory, \
                    tempfile.TemporaryDirectory() as outside:
                daemon = self.daemon(Path(directory))
                cleanup = Mock(side_effect=OSError(5, 'private cleanup'))
                temporary = SimpleNamespace(name=outside, cleanup=cleanup)
                observed = Mock()
                # Outside-read must be refused (2), so zero triggers the primary
                # native_probe_outside_read failure before cleanup also fails.
                with patch.object(native.tempfile, 'TemporaryDirectory', return_value=temporary), \
                        patch.object(native.subprocess, 'run', return_value=SimpleNamespace(returncode=0)):
                    expected = OSError if preserve else SandboxError
                    message = 'private cleanup' if preserve else 'native_probe_outside_read'
                    with self.assertRaisesRegex(expected, message):
                        daemon._preflight(observed, preserve_cleanup_errors=preserve)
                failures = [call.args for call in observed.call_args_list if call.args[0] == 'failed']
                self.assertTrue(any(stage == 'preflight_outside_read_confined'
                                    and isinstance(error, SandboxError)
                                    for _, stage, error, *_ in failures))
                observed.assert_any_call('cleanup_failed', 'preflight_outside_cleanup', cleanup.side_effect, None)
                cleanup.assert_called_once()

    def test_production_prepare_attaches_observer_and_preserves_cleanup_errors(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            home, runtime, source = root / 'home', root / 'runtime', root / 'wnd'
            home.mkdir()
            runtime.mkdir()
            source.write_bytes(b'owned inert fixture')
            source.chmod(0o700)
            daemon = self.daemon(home)
            daemon.wnd = source
            observer = Mock()
            daemon.diagnostic = observer
            preflight = Mock(side_effect=SandboxError('stop_at_preflight'))
            # All platform, signature, broker and control checks are hermetic;
            # execute the real _prepare path until its actual preflight call.
            with patch.object(native.sys, 'platform', 'darwin'), \
                    patch.object(native.os, 'geteuid', return_value=os.geteuid() or 501), \
                    patch.object(Path, 'is_file', return_value=True), \
                    patch.object(native, '_private'), patch.object(native, '_open_control', return_value=17), \
                    patch.object(native, '_lock_control', return_value=18), \
                    patch.object(daemon, '_recover'), patch.object(native, '_gc_control'), \
                    patch.object(native, '_verify_control'), \
                    patch.object(native, 'verify_manifest', return_value={'helpers': {}}), \
                    patch.object(native.tempfile, 'mkdtemp', return_value=str(runtime)), \
                    patch.object(native, 'digest', return_value='0' * 64), \
                    patch.object(native.subprocess, 'run', return_value=SimpleNamespace(returncode=0)), \
                    patch.object(native, 'SocksBroker'), patch.object(daemon, '_preflight', preflight):
                with self.assertRaisesRegex(SandboxError, 'stop_at_preflight'):
                    daemon._prepare()
            preflight.assert_called_once_with(diagnostic=observer, preserve_cleanup_errors=True)


if __name__ == '__main__':
    unittest.main()
