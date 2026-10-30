"""Offline controlling-terminal acceptance for the real installer wrapper.

Only install_release is replaced in a disposable copy; real archive installation,
launcher ownership checks, entry point, prompts and child lifecycle are retained.
No production network override, keychain calls or real identities are used.
"""
import errno
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

from test_marmish_install import INSTALLER, VERSION, archive_bytes, fixture_files, load_installer


class TerminalProcess:
    def __init__(self, command, environment):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            try:
                # The wrapper itself replaces stdin with embedded Python source.
                # No user answer can come from parent stdin, even before that.
                null = os.open(os.devnull, os.O_RDONLY)
                os.dup2(null, 0)
                os.close(null)
                os.execve(command[0], command, environment)
            finally:
                os._exit(127)
        self.output = b''
        self.cursor = 0
        self.status = None

    def read(self, timeout=0.05):
        if select.select([self.fd], [], [], timeout)[0]:
            try:
                self.output += os.read(self.fd, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise

    def expect(self, text):
        expected = text.encode()
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            found = self.output.find(expected, self.cursor)
            if found >= 0:
                self.cursor = found + len(expected)
                return
            self.read()
        raise AssertionError(f'missing {text!r}: {self.output.decode(errors="replace")}')

    def send(self, text):
        os.write(self.fd, text.encode())

    def wait(self):
        deadline = time.monotonic() + 12
        while self.status is None and time.monotonic() < deadline:
            self.read()
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.status = os.waitstatus_to_exitcode(status)
        if self.status is None:
            raise AssertionError(f'installer did not exit: {self.output!r}')
        self.read(0)
        return self.status

    def close(self):
        if self.status is None:
            try:
                os.killpg(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(self.pid, 0)
        os.close(self.fd)


class QuickstartTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.home = self.base / "home spaces ' $dollar;$(touch NEVER)"
        self.home.mkdir(mode=0o700)
        self.prefix = self.home / 'custom install'
        self.log = self.base / 'calls.jsonl'
        self.environment = dict(os.environ, HOME=str(self.home), QUICKSTART_TEST_LOG=str(self.log))
        installer = load_installer()
        self.target = installer.native_target()
        files = fixture_files(installer, self.target)
        fake = ('#!' + sys.executable + '\n' + '''import json, os, signal, sys, time
from pathlib import Path
args = sys.argv[1:]
name = Path(sys.argv[0]).name
with open(os.environ['QUICKSTART_TEST_LOG'], 'a') as output:
    output.write(json.dumps({'name': name, 'args': args, 'cwd': os.getcwd(), 'tty': os.isatty(0), 'pid': os.getpid(), 'debug': os.environ.get('MARMISH_DEBUG')}) + '\\n')
if os.environ.get('QUICKSTART_TEST_FAIL') in args:
    sys.exit(7)
if '--init' in args:
    print('{"account":"' + 'a' * 64 + '"}', flush=True)
if name == 'marmishd' and os.environ.get('QUICKSTART_TEST_LONG'):
    print('FAKE_SERVER_RUNNING', flush=True)
    while True:
        time.sleep(1)
if name == 'marmishd' and os.environ.get('QUICKSTART_TEST_STDIN'):
    print('CHILD_INPUT=' + sys.stdin.readline().rstrip('\\n'), flush=True)
''').encode()
        files['bin/marmish'] = fake
        files['bin/marmishd'] = fake
        archive = self.base / 'fixture.tar.gz'
        archive.write_bytes(archive_bytes(installer, files=files, target=self.target))
        injection = '\ndef install_release(options, prefix, launcher_dir):\n    install_archive(Path(' + repr(str(archive)) + '), prefix, launcher_dir, ' + repr(VERSION) + ', ' + repr(self.target) + ')\n\n'
        self.wrapper = self.base / 'install.sh'
        self.wrapper.write_text(INSTALLER.read_text().replace("if __name__ == '__main__':", injection + "if __name__ == '__main__':"))

    def command(self, *args):
        return ['/bin/sh', str(self.wrapper), '--prefix', str(self.prefix), *args]

    def start(self, debug=False, **environment):
        args = ['--quickstart', *(['--debug'] if debug else [])]
        process = TerminalProcess(self.command(*args), dict(self.environment, **environment))
        self.addCleanup(process.close)
        return process

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def identify(self, process, role):
        process.expect('Role')
        process.send(role + '\n')
        process.expect('Relay URL')
        process.send('wss://relay.example/\n')
        process.expect('Client public identity' if role == 'server' else 'Server public identity')

    def test_both_roles_custom_prefix_literal_argv_and_terminal_stdin(self):
        for role in ('server', 'client'):
            with self.subTest(role=role):
                self.log.unlink(missing_ok=True)
                process = self.start()
                self.identify(process, role)
                process.send('a' * 64 + '\n')
                self.assertEqual(process.wait(), 0, process.output)
                calls = self.calls()
                self.assertEqual(len(calls), 3)
                self.assertEqual(calls[0]['args'], ['--doctor'])
                self.assertIn('--init', calls[1]['args'])
                self.assertIn('--key-storage', calls[1]['args'])
                self.assertIn('auto', calls[1]['args'])
                self.assertIn(str(self.home / '.marmish/default'), calls[1]['args'])
                for call in calls:
                    self.assertEqual(call['cwd'], str(self.home))
                    self.assertTrue(call['tty'])
                final = calls[2]
                self.assertEqual(final['name'], 'marmishd' if role == 'server' else 'marmish')
                self.assertIn('--relay=wss://relay.example/', final['args'])
                if role == 'server':
                    self.assertIn('--allow=' + 'a' * 64, final['args'])
                    self.assertEqual(final['args'][-2:], ['--cwd', str(self.home)])
                else:
                    self.assertIn('-u' + 'a' * 64, final['args'])
                    self.assertEqual(final['args'][-4:], ['--', '/usr/bin/printf', '%s\n', 'Hello through Marmot'])
                self.assertFalse((self.home / 'NEVER').exists())

    def test_debug_forwards_only_environment_and_retains_legacy_argv(self):
        for role in ('server', 'client'):
            with self.subTest(role=role):
                self.log.unlink(missing_ok=True)
                process = self.start(debug=True)
                self.identify(process, role)
                process.send('a' * 64 + '\n')
                self.assertEqual(process.wait(), 0, process.output)
                self.assertEqual(len(self.calls()), 3)
                for call in self.calls():
                    self.assertEqual(call['debug'], '1')
                    self.assertNotIn('--debug', call['args'])
                records = self.debug_records(process.output)
                self.assertEqual(records[0]['event'], 'context')
                self.assertTrue(any(item['stage'] == role + '_command' for item in records))
                serialized = json.dumps(records)
                self.assertNotIn(str(self.home), serialized)
                self.assertNotIn('relay.example', serialized)
                self.assertNotIn('a' * 64, serialized)
                self.assertIn(b'older archives retain category-only errors', process.output)

    @staticmethod
    def debug_records(output):
        prefix = 'marmish installer debug: '
        return [json.loads(line[len(prefix):]) for line in output.decode().splitlines()
                if line.startswith(prefix)]

    def test_debug_child_failure_records_status_and_stops_setup(self):
        process = self.start(debug=True, QUICKSTART_TEST_FAIL='--init')
        process.expect('Role')
        process.send('server\n')
        process.expect('Relay URL')
        process.send('wss://relay.example/\n')
        self.assertEqual(process.wait(), 1, process.output)
        failures = [item for item in self.debug_records(process.output) if item['event'] == 'failed']
        self.assertEqual(len(failures), 1)
        self.assertEqual(failures[0]['stage'], 'identity_initialization')
        self.assertEqual(failures[0]['exit_code'], 7)
        self.assertEqual(len(self.calls()), 2)

    def test_debug_signal_still_reaps_server(self):
        process = self.start(debug=True, QUICKSTART_TEST_LONG='1')
        self.identify(process, 'server')
        process.send('a' * 64 + '\n')
        process.expect('FAKE_SERVER_RUNNING')
        child = self.calls()[-1]['pid']
        os.kill(process.pid, signal.SIGTERM)
        self.assertEqual(process.wait(), 128 + signal.SIGTERM, process.output)
        with self.assertRaises(ProcessLookupError):
            os.kill(child, 0)
        failure = [item for item in self.debug_records(process.output) if item['event'] == 'failed']
        self.assertEqual(failure[0]['stage'], 'server_command')
        self.assertEqual(failure[0]['category'], 'interrupted')

    def test_typeahead_remains_available_to_child(self):
        process = self.start(QUICKSTART_TEST_STDIN='1')
        self.identify(process, 'server')
        process.send('a' * 64 + '\nchild data\n')
        self.assertEqual(process.wait(), 0, process.output)
        self.assertIn(b'CHILD_INPUT=child data', process.output)

    def test_child_failure_stops_subsequent_steps(self):
        for step in ('--doctor', '--init'):
            with self.subTest(step=step):
                self.log.unlink(missing_ok=True)
                process = self.start(QUICKSTART_TEST_FAIL=step)
                process.expect('Role')
                process.send('client\n')
                process.expect('Relay URL')
                process.send('wss://relay.example/\n')
                self.assertNotEqual(process.wait(), 0)
                calls = self.calls()
                self.assertEqual(len(calls), 1 if step == '--doctor' else 2)
                self.assertTrue((self.prefix / '.marmish-install.json').exists())

    def test_invalid_input_and_eof_never_start_commands(self):
        for answer in ('other\n', '\n', '\x04', 'bad\t; touch SENTINEL\n'):
            with self.subTest(answer=repr(answer)):
                process = self.start()
                process.expect('Role')
                process.send(answer)
                self.assertNotEqual(process.wait(), 0)
                self.assertEqual(self.calls(), [])
                self.assertFalse((self.home / 'SENTINEL').exists())

    def test_rejected_paste_is_not_left_for_parent_shell(self):
        # Resume a shell reader after the installer; merely checking that the
        # installer did not execute the paste would miss terminal-queue leakage.
        parent = '"$@"; result=$?; printf "PARENT_RESUMED\\n"; IFS= read -r rest < /dev/tty; printf "REMAINDER=<%s>\\n" "$rest"; exit "$result"'
        command = ['/bin/sh', '-c', parent, 'quickstart-parent', *self.command('--quickstart')]
        process = TerminalProcess(command, self.environment)
        self.addCleanup(process.close)
        process.expect('Role')
        process.send('bad\t; touch SENTINEL\nunwanted queued input\n')
        process.expect('PARENT_RESUMED')
        process.send('safe continuation\n')
        self.assertNotEqual(process.wait(), 0)
        self.assertIn(b'REMAINDER=<safe continuation>', process.output)
        self.assertEqual(self.calls(), [])

    def test_relay_option_injection_is_rejected_before_doctor(self):
        process = self.start()
        process.expect('Role')
        process.send('client\n')
        process.expect('Relay URL')
        process.send('--shell-command=unexpected\n')
        self.assertNotEqual(process.wait(), 0)
        self.assertEqual(self.calls(), [])

    def test_leading_dash_peer_is_an_option_value(self):
        process = self.start()
        self.identify(process, 'client')
        process.send('-not-a-valid-key\n')
        # Fake CLI records argv; the real CLI subsequently rejects this key.
        self.assertEqual(process.wait(), 0, process.output)
        self.assertIn('-u-not-a-valid-key', self.calls()[-1]['args'])

    def test_signals_reap_foreground_server(self):
        for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            with self.subTest(signum=signum):
                self.log.unlink(missing_ok=True)
                process = self.start(QUICKSTART_TEST_LONG='1')
                self.identify(process, 'server')
                process.send('a' * 64 + '\n')
                process.expect('FAKE_SERVER_RUNNING')
                child = self.calls()[-1]['pid']
                if signum == signal.SIGINT:
                    process.send('\x03')
                else:
                    os.kill(process.pid, signum)
                self.assertEqual(process.wait(), 128 + signum, process.output)
                with self.assertRaises(ProcessLookupError):
                    os.kill(child, 0)

    def test_missing_terminal_and_invalid_options_do_not_install(self):
        cases = [(['--quickstart'], 1), (['--quickstart', '--uninstall'], 2), (['--quickstart', '--version', 'invalid'], 1)]
        for args, status in cases:
            with self.subTest(args=args):
                result = subprocess.run(self.command(*args), env=self.environment, start_new_session=True, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertFalse(self.prefix.exists())
                self.assertEqual(self.calls(), [])

    def test_signal_during_spawn_is_forwarded_after_handle_assignment(self):
        installer = load_installer()
        signals = installer.QuickstartSignals()
        child = mock.Mock()
        child.poll.return_value = None
        child.wait.return_value = -signal.SIGTERM

        def interrupted_spawn(*args, **kwargs):
            self.assertTrue(signals.spawning)
            signals.handle(signal.SIGTERM, None)
            return child

        with mock.patch.object(installer.subprocess, 'Popen', side_effect=interrupted_spawn):
            with self.assertRaises(installer.QuickstartInterrupted) as caught:
                installer.run_quickstart_child(['owned-test-child'], 0, signals, 'test')
        self.assertEqual(caught.exception.number, signal.SIGTERM)
        self.assertFalse(signals.spawning)
        child.send_signal.assert_called_once_with(signal.SIGTERM)
        child.wait.assert_called_once_with(timeout=5)

    def test_repeated_signal_does_not_interrupt_child_reaping(self):
        installer = load_installer()
        signals = installer.QuickstartSignals()
        child = mock.Mock()
        child.poll.return_value = None

        def wait_for_child(timeout=None):
            signals.handle(signal.SIGTERM, None)
            return -signal.SIGTERM

        child.wait.side_effect = wait_for_child
        with mock.patch.object(installer.subprocess, 'Popen', return_value=child):
            with self.assertRaises(installer.QuickstartInterrupted):
                installer.run_quickstart_child(['owned-test-child'], 0, signals, 'test')
        self.assertEqual(signals.first, signal.SIGTERM)
        child.send_signal.assert_called_once_with(signal.SIGTERM)
        self.assertEqual(child.wait.call_count, 2)

    def test_logging_interruption_during_failure_cannot_skip_child_reaping(self):
        installer = load_installer()
        signals = installer.QuickstartSignals()
        child = mock.Mock()
        child.poll.return_value = None
        child.wait.side_effect = [RuntimeError('private failure'), 0]
        diagnostic = mock.Mock()
        diagnostic.enabled = True
        diagnostic.child_stages = {}
        diagnostic.failure.side_effect = installer.QuickstartInterrupted(signal.SIGINT)
        with mock.patch.object(installer.subprocess, 'Popen', return_value=child):
            with self.assertRaises(installer.QuickstartInterrupted):
                installer.run_quickstart_child(['owned-test-child'], 0, signals, 'test', diagnostic)
        child.send_signal.assert_called_once_with(signal.SIGTERM)
        self.assertEqual(child.wait.call_args_list, [mock.call(), mock.call(timeout=5)])

    def test_compatibility_notice_does_not_swallow_interruption(self):
        installer = load_installer()
        diagnostic = mock.Mock()
        diagnostic.enabled = True
        signals = installer.QuickstartSignals()
        stream = mock.Mock()
        stream.write.side_effect = lambda *_: signals.handle(signal.SIGHUP, None)
        with mock.patch.object(installer.sys, 'stderr', stream), \
                mock.patch.object(installer, 'quickstart_prompt') as prompt:
            with self.assertRaises(installer.QuickstartInterrupted):
                installer.quickstart(self.prefix / 'bin', 0, signals, diagnostic)
        prompt.assert_not_called()
        self.assertEqual(signals.first, signal.SIGHUP)

    def test_unresponsive_child_is_killed_and_reaped(self):
        installer = load_installer()
        child = mock.Mock()
        child.poll.return_value = None
        child.wait.side_effect = [subprocess.TimeoutExpired('owned-test-child', 5), 0]
        installer.stop_quickstart_child(child, signal.SIGTERM)
        child.send_signal.assert_called_once_with(signal.SIGTERM)
        child.kill.assert_called_once_with()
        self.assertEqual(child.wait.call_args_list, [mock.call(timeout=5), mock.call()])

    def test_plain_install_needs_no_terminal_or_identity(self):
        result = subprocess.run(self.command(), env=self.environment, start_new_session=True, capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.prefix / '.marmish-install.json').exists())
        self.assertFalse((self.home / '.marmish').exists())
        self.assertEqual(self.calls(), [])


if __name__ == '__main__':
    unittest.main()
