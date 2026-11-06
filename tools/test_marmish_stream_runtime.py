"""Hermetic qualification cleanup checks: no real processes, keys, or network."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import tempfile
from types import SimpleNamespace
import signal
import subprocess
import unittest
from unittest.mock import MagicMock, patch

import marmish_stream_runtime as runtime


class CleanupTests(unittest.TestCase):
    def fixture(self, *, initial=None, exits=(0,), daemon=True):
        process = MagicMock()
        process.pid = 12345
        process.returncode = initial
        process.poll.return_value = initial
        process.stdin = io.BytesIO()
        process.stdout = io.BytesIO()
        process.stderr = io.BytesIO()
        outcomes = iter(exits)

        def wait(*, timeout):
            self.assertIn(timeout, (10, 45))
            outcome = next(outcomes)
            if isinstance(outcome, Exception):
                raise outcome
            process.returncode = outcome
            return outcome

        process.wait.side_effect = wait
        log = io.BytesIO(b"private diagnostics") if daemon else None
        qualification = runtime.Qualification.__new__(runtime.Qualification)
        qualification.children = [(process, log, "server" if daemon else None)]
        qualification.report = {}
        return qualification, process, log

    def check_closed(self, process, log):
        self.assertTrue(process.stdin.closed)
        self.assertTrue(process.stdout.closed)
        self.assertTrue(process.stderr.closed)
        if log is not None:
            self.assertTrue(log.closed)

    @patch.object(runtime.os, "killpg")
    def test_daemon_must_remain_alive_until_cleanup(self, killpg):
        for code in (0, 1, -signal.SIGABRT):
            with self.subTest(code=code):
                qualification, process, log = self.fixture(initial=code)
                self.assertFalse(qualification.cleanup())
                killpg.assert_not_called()
                process.wait.assert_not_called()
                self.assertEqual(qualification.report["daemon_shutdown"],
                                 [{"role": "server", "exit": code}])
                self.check_closed(process, log)

    @patch.object(runtime.os, "killpg")
    def test_live_daemon_requires_graceful_zero_exit(self, killpg):
        for code in (0, 1, -signal.SIGTERM, -signal.SIGABRT):
            with self.subTest(code=code):
                killpg.reset_mock()
                qualification, process, log = self.fixture(exits=(code,))
                self.assertEqual(qualification.cleanup(), code == 0)
                killpg.assert_called_once_with(process.pid, signal.SIGTERM)
                process.wait.assert_called_once_with(timeout=45)
                self.assertEqual(qualification.report["daemon_shutdown"],
                                 [{"role": "server", "exit": code}])
                self.check_closed(process, log)

    @patch.object(runtime.os, "killpg")
    def test_forced_shutdown_fails_even_with_zero_final_exit(self, killpg):
        timeout = subprocess.TimeoutExpired("private argv", 45)
        qualification, process, log = self.fixture(exits=(timeout, 0))
        self.assertFalse(qualification.cleanup())
        self.assertEqual([call.args[1] for call in killpg.call_args_list],
                         [signal.SIGTERM, signal.SIGKILL])
        self.assertEqual(qualification.report["daemon_shutdown"],
                         [{"role": "server", "exit": 0}])
        self.check_closed(process, log)

    @patch.object(runtime.os, "killpg")
    def test_kill_wait_timeout_is_unconfirmed_and_closes_private_logs(self, _killpg):
        qualification, process, log = self.fixture(exits=(
            subprocess.TimeoutExpired("private argv", 45),
            subprocess.TimeoutExpired("private argv", 10),
        ))
        self.assertFalse(qualification.cleanup())
        self.assertEqual(qualification.report["daemon_shutdown"],
                         [{"role": "server", "exit": None}])
        self.check_closed(process, log)

    @patch.object(runtime.os, "killpg", side_effect=ProcessLookupError)
    def test_exit_between_poll_and_signal_is_reaped_and_checked(self, killpg):
        qualification, process, log = self.fixture(exits=(0,))
        self.assertTrue(qualification.cleanup())
        killpg.assert_called_once_with(process.pid, signal.SIGTERM)
        process.wait.assert_called_once_with(timeout=45)
        self.check_closed(process, log)

    @patch.object(runtime.os, "killpg", side_effect=PermissionError)
    def test_group_permission_failure_attempts_owned_child_kill_and_reap(self, _killpg):
        qualification, process, log = self.fixture(exits=(-signal.SIGKILL,))
        self.assertFalse(qualification.cleanup())
        process.kill.assert_called_once_with()
        process.wait.assert_called_once_with(timeout=45)
        self.check_closed(process, log)

    @patch.object(runtime.os, "killpg")
    def test_expected_client_command_and_cancel_exits_are_not_daemon_crashes(self, killpg):
        for code in (0, 7, 130):
            with self.subTest(code=code):
                qualification, process, log = self.fixture(initial=code, daemon=False)
                self.assertTrue(qualification.cleanup())
                self.assertEqual(qualification.report, {})
                killpg.assert_not_called()
                self.check_closed(process, log)


class DaemonReadinessTests(unittest.TestCase):
    @staticmethod
    def status(state="active"):
        result = {"state": state}
        if state != "activating":
            result.update(identity="public-identity", conversations=0, received=0, pending=0)
        return SimpleNamespace(returncode=0, stdout=json.dumps(
            {"ok": True, "result": result, "error": None}).encode())

    def test_zero_exit_activating_and_degraded_are_not_ready(self):
        for state in ("activating", "transport_degraded", "active"):
            with self.subTest(state=state):
                self.assertEqual(runtime.Qualification.daemon_ready(self.status(state), "server"),
                                 state == "active")
        self.assertFalse(runtime.Qualification.daemon_ready(
            SimpleNamespace(returncode=1, stdout=b"not JSON"), "server"))

    def test_start_waits_for_active_snapshot_for_each_owned_daemon(self):
        with tempfile.TemporaryDirectory() as directory:
            qualification = runtime.Qualification.__new__(runtime.Qualification)
            qualification.root = Path(directory)
            process = MagicMock()
            process.poll.return_value = None
            qualification.spawn = MagicMock(return_value=process)
            qualification.run = MagicMock(side_effect=[self.status("activating"), self.status(),
                                                       self.status("transport_degraded"), self.status()])
            with patch.object(runtime.time, "sleep"):
                qualification.start_daemons()
            self.assertEqual([call.args[1] for call in qualification.run.call_args_list],
                             ["server", "server", "client", "client"])
            for call in qualification.spawn.call_args_list:
                call.kwargs["log"].close()

    def test_unknown_or_malformed_success_status_fails_closed(self):
        for stdout in (b"{}", b"null", b"garbage", b"x" * 65537,
                       b'{"ok":true,"result":{"state":"unknown"},"error":null}',
                       b'{"ok":true,"result":{"state":"active"},"error":null}'):
            with self.subTest(stdout=stdout[:70]):
                with self.assertRaises(runtime.QualificationError) as failure:
                    runtime.Qualification.daemon_ready(SimpleNamespace(returncode=0, stdout=stdout), "client")
                self.assertEqual((failure.exception.stage, failure.exception.category),
                                 ("daemon_client", "status_schema"))
        for counter in (True, -1, 2**64, "0"):
            envelope = json.loads(self.status().stdout)
            envelope["result"]["pending"] = counter
            with self.subTest(counter=counter), self.assertRaises(runtime.QualificationError):
                runtime.Qualification.daemon_ready(SimpleNamespace(
                    returncode=0, stdout=json.dumps(envelope).encode()), "client")

    def test_activating_forever_retains_bounded_readiness_deadline(self):
        with tempfile.TemporaryDirectory() as directory:
            qualification = runtime.Qualification.__new__(runtime.Qualification)
            qualification.root = Path(directory)
            process = MagicMock()
            process.poll.return_value = None
            qualification.spawn = MagicMock(return_value=process)
            qualification.run = MagicMock(return_value=self.status("activating"))
            with patch.object(runtime.time, "monotonic", side_effect=[0, 0, 1, 91]), \
                 patch.object(runtime.time, "sleep"), self.assertRaises(runtime.QualificationError) as failure:
                qualification.start_daemons()
            self.assertEqual(failure.exception.category, "readiness_timeout")
            qualification.run.assert_called_once()
            qualification.spawn.call_args.kwargs["log"].close()


class EarlyStdinClosureTests(unittest.TestCase):
    def fixture(self):
        qualification = runtime.Qualification.__new__(runtime.Qualification)
        qualification.options = SimpleNamespace(bytes=1)
        qualification.report = {"results": []}
        return qualification

    def test_natural_exits_preserve_empty_and_nul_output_with_both_codecs(self):
        qualification = self.fixture()

        def run(stage, identity, *arguments, data):
            self.assertEqual(identity, "client")
            self.assertEqual(arguments[:2], ("stream", "public-remote"))
            self.assertGreater(len(data), 262144)
            self.assertEqual(data[:256], bytes(range(256)))
            self.assertIn("--", arguments)
            expected = b"\0" if "/usr/bin/head" in arguments else b""
            return SimpleNamespace(returncode=0, stdout=expected)

        qualification.run = MagicMock(side_effect=run)
        qualification.early_stdin_closure("public-remote")
        self.assertEqual(qualification.run.call_count, 4)
        self.assertEqual([result["compression"] for result in qualification.report["results"]],
                         ["none", "none", "zlib", "zlib"])
        for invocation in qualification.run.call_args_list:
            args = invocation.args
            self.assertEqual("--compress" in args, args[0].endswith("_zlib"))
        self.assertEqual([result["stdout_bytes"] for result in qualification.report["results"]],
                         [0, 1, 0, 1])

    def test_remote_closed_failure_or_wrong_output_cannot_qualify(self):
        for code, true_output, head_output in (
            (255, b"", b"\0"), (0, b"unexpected", b"\0"), (0, b"", b""),
        ):
            with self.subTest(code=code, true_output=true_output, head_output=head_output):
                qualification = self.fixture()
                qualification.run = MagicMock(side_effect=lambda *args, **kwargs: SimpleNamespace(
                    returncode=code, stdout=head_output if "/usr/bin/head" in args else true_output))
                with self.assertRaises(runtime.QualificationError) as failure:
                    qualification.early_stdin_closure("public-remote")
                self.assertEqual(failure.exception.category, "natural_exit_or_output_mismatch")


class HarnessBoundaryTests(unittest.TestCase):
    def test_cleanup_failure_controls_final_report_and_preserves_private_state(self):
        qualification = MagicMock()
        qualification.report = {"schema": 1, "passed": False}
        qualification.root = Path("/private/qualification-fixture")
        qualification.cleanup.return_value = False
        options = SimpleNamespace(output=Path("/private/report-fixture"))
        with patch.object(runtime, "arguments", return_value=options), \
             patch.object(runtime, "Qualification", return_value=qualification), \
             patch.object(runtime.signal, "signal"), \
             patch.object(runtime, "write_report") as write_report, \
             patch.object(runtime.shutil, "rmtree") as remove, \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(runtime.main(), 1)
        remove.assert_not_called()
        qualification.early_stdin_closure.assert_called_once_with(qualification.initialize.return_value)
        self.assertFalse(qualification.report["passed"])
        self.assertEqual(qualification.report["cleanup"], "unconfirmed")
        self.assertTrue(qualification.report["private_state_retained"])
        write_report.assert_called_once_with(options.output, qualification.report)

    def test_report_write_failure_preserves_successful_run_private_state(self):
        qualification = MagicMock()
        qualification.report = {"schema": 1, "passed": False}
        qualification.root = Path("/private/qualification-fixture")
        qualification.cleanup.return_value = True
        with patch.object(runtime, "arguments", return_value=SimpleNamespace(output=Path("report"))), \
             patch.object(runtime, "Qualification", return_value=qualification), \
             patch.object(runtime.signal, "signal"), \
             patch.object(runtime, "write_report", side_effect=OSError), \
             patch.object(runtime.shutil, "rmtree") as remove, \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(runtime.main(), 1)
        remove.assert_not_called()
        self.assertTrue(qualification.report["private_state_retained"])
        self.assertEqual(qualification.report["error"]["category"], "report_write_failed")

    def test_failed_cleanup_report_rewrite_removes_new_passing_artifact(self):
        qualification = MagicMock()
        qualification.report = {"schema": 1, "passed": False}
        qualification.root = Path("/private/qualification-fixture")
        qualification.cleanup.return_value = True
        output = Path("/private/report-fixture")
        with patch.object(runtime, "arguments", return_value=SimpleNamespace(output=output)), \
             patch.object(runtime, "Qualification", return_value=qualification), \
             patch.object(runtime.signal, "signal"), \
             patch.object(runtime, "write_report", side_effect=[None, OSError]) as write_report, \
             patch.object(runtime.shutil, "rmtree", side_effect=OSError), \
             patch.object(runtime.Path, "unlink") as unlink, \
             contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(runtime.main(), 1)
        self.assertEqual(write_report.call_count, 2)
        unlink.assert_called_once_with()
        self.assertFalse(qualification.report["passed"])
        self.assertTrue(qualification.report["private_state_retained"])
        self.assertEqual(qualification.report["error"]["category"], "state_removal_failed")

    def test_relay_token_and_ambient_secrets_are_excluded(self):
        with tempfile.TemporaryDirectory() as parent, \
             patch.object(runtime.Path, "home", return_value=Path(parent)), \
             patch.dict(os.environ, {"PATH": "/usr/bin:/bin", "LANG": "C",
                                    "PRIVATE_TOKEN": "never-forward", "LD_PRELOAD": "private"}, clear=True):
            options = SimpleNamespace(relay="wss://relay.example:443/private-token", bytes=16, samples=1)
            qualification = runtime.Qualification(options)
            try:
                self.assertEqual(qualification.report["relay"], "wss://relay.example:443")
                self.assertTrue(qualification.report["relay_path_redacted"])
                self.assertFalse(qualification.report["passed"])
                self.assertNotIn("private-token", str(qualification.report))
                self.assertNotIn("PRIVATE_TOKEN", qualification.environment)
                self.assertNotIn("LD_PRELOAD", qualification.environment)
                self.assertEqual(qualification.environment["PATH"], "/usr/bin:/bin")
                self.assertTrue(qualification.environment["HOME"].startswith(str(qualification.root)))
            finally:
                shutil.rmtree(qualification.root)

    def test_spawn_defers_interrupt_until_owned_child_is_registered(self):
        qualification = runtime.Qualification.__new__(runtime.Qualification)
        qualification.root = Path("/private/spawn-fixture")
        qualification.environment = {}
        qualification.children = []
        qualification.command = MagicMock(return_value=["not-executed"])
        process = MagicMock()

        def interrupted(number, _frame):
            self.assertEqual(number, signal.SIGTERM)
            self.assertEqual(qualification.children, [(process, None, None)])
            raise runtime.QualificationError("interrupted", "signal_received")

        handlers = {signal.SIGINT: interrupted, signal.SIGTERM: interrupted}

        def register(number, handler):
            previous = handlers[number]
            handlers[number] = handler
            return previous

        def spawning(*_arguments, **_options):
            handlers[signal.SIGTERM](signal.SIGTERM, None)
            self.assertEqual(qualification.children, [])
            return process

        with patch.object(runtime.signal, "signal", side_effect=register), \
             patch.object(runtime.subprocess, "Popen", side_effect=spawning):
            with self.assertRaises(runtime.QualificationError):
                qualification.spawn("client", "status")
        self.assertIs(handlers[signal.SIGINT], interrupted)
        self.assertIs(handlers[signal.SIGTERM], interrupted)


if __name__ == "__main__":
    unittest.main()
