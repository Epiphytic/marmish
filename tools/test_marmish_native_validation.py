"""Offline validation support tests; no ambient provider operations."""
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

from marmish_keys import encode_nsec
from marmish_native_validation import (NativeLinuxContext, _bounded, native_environment,
                                       scan_at_rest, scan_process, audit_tree, audit_native_process)
from mdk_smoke import SmokeError


class ValidationSupportTests(unittest.TestCase):
    def test_cli_entrypoint_and_import_share_context_module(self):
        script = Path(__file__).with_name('marmish_native_validation.py')
        code = '''import importlib, runpy, sys
sys.argv = [sys.argv[1], '--help']
try:
    runpy.run_path(sys.argv[0], run_name='__main__')
except SystemExit as error:
    assert error.code == 0
module = sys.modules['marmish_native_validation']
assert module.__name__ == '__main__'
assert importlib.import_module('marmish_native_validation') is module
'''
        result = subprocess.run([sys.executable, '-c', code, str(script)],
            env={'PATH': '/usr/bin:/bin', 'PYTHONPATH': str(script.parent)},
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr.decode())

    def test_no_ambient_discovery(self):
        with patch.dict(os.environ, {'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/ambient'}):
            self.assertEqual(native_environment(), {})

    def test_private_process_bounded(self):
        env = {'PATH': '/usr/bin:/bin'}
        self.assertEqual(_bounded([sys.executable, '-c',
                                  'import sys;sys.stdout.buffer.write(sys.stdin.buffer.read())'],
                                 env, b'private'), b'private')
        with self.assertRaisesRegex(SmokeError, 'native_output_limit'):
            _bounded([sys.executable, '-c', 'print("a"*10000)'], env, limit=32)
        with self.assertRaisesRegex(SmokeError, 'native_operation_timeout'):
            _bounded([sys.executable, '-c', 'import time;time.sleep(5)'], env, timeout=.05)
        with self.assertRaisesRegex(SmokeError, 'native_operation_failed'):
            _bounded([sys.executable, '-c', 'raise SystemExit(1)'], env)

    def test_streaming_scan_all_forms_and_boundary(self):
        secret = bytes(range(32))
        import base64
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = root / 'journal'
            for form in (secret, secret.hex().encode(), base64.b64encode(secret),
                         encode_nsec(secret).encode()):
                path.write_bytes(b'x' * 65530 + form)
                with self.assertRaisesRegex(SmokeError, 'native_secret_at_rest'):
                    scan_at_rest([root], [secret])
            path.write_bytes(b'public data')
            self.assertEqual(scan_at_rest([root], [secret]), {'secret_at_rest': False})
            with self.assertRaisesRegex(SmokeError, 'native_scan_limit'):
                scan_at_rest([root], [secret], byte_limit=1)

    def test_provider_exclusion_and_symlink_refusal(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            provider = root / 'provider'
            provider.mkdir()
            secret = b'a' * 32
            (provider / 'data').write_bytes(secret)
            self.assertEqual(scan_at_rest([root], [secret], exclude=[provider]),
                             {'secret_at_rest': False})
            (root / 'link').symlink_to(provider / 'data')
            with self.assertRaisesRegex(SmokeError, 'native_scan_unsafe_file'):
                scan_at_rest([root], [secret], exclude=[provider])

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux proc inspection')
    def test_process_scan(self):
        secret = bytes(range(32))
        child = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(10)'],
                                 env={'PATH': '/usr/bin:/bin', 'TEST_LEAK': secret.hex()})
        try:
            with self.assertRaisesRegex(SmokeError, 'native_secret_in_process'):
                scan_process(child.pid, [secret])
            with self.assertRaisesRegex(SmokeError, 'native_environment_leak'):
                scan_process(child.pid, [], expected_env_absent=['TEST_LEAK'])
        finally:
            child.kill()
            child.wait()

    def test_root_mode_and_bus_address_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            ctx = NativeLinuxContext(Path(sys.executable).resolve())
            ctx.root = Path(directory)
            for name in ('home', 'config', 'data', 'cache', 'run', 'control'):
                (ctx.root / name).mkdir(mode=0o700)
            ctx.env = {'DBUS_SESSION_BUS_ADDRESS': 'unix:path=/ambient'}
            with self.assertRaisesRegex(SmokeError, 'native_context_invalid'):
                ctx.validate()

    def test_metadata_exact_refs_and_no_duplicates(self):
        ctx = NativeLinuxContext(Path(sys.executable).resolve())
        ref = {'version': 1, 'purpose': 'nostr', 'id': '12' * 32}
        with patch.object(ctx, 'provider', return_value={'items': [{'reference': ref, 'label': 'name'}]}):
            self.assertEqual(ctx.references(), [ref])
        with patch.object(ctx, 'provider', return_value={'items': [{'reference': ref, 'label': 'a'}] * 2}):
            with self.assertRaisesRegex(SmokeError, 'native_metadata_invalid'):
                ctx.references()

    def test_lock_requires_owned_collection_and_confirmed_refusal(self):
        ctx = NativeLinuxContext(Path(sys.executable).resolve())
        results = [b"(objectpath '/org/freedesktop/secrets/collection/login',)\n",
                   b"([objectpath '/org/freedesktop/secrets/collection/login'], objectpath '/')\n"]
        with patch.object(ctx, '_bus', side_effect=results), \
                patch.object(ctx, 'provider', side_effect=SmokeError('locked_or_denied')):
            ctx.lock()
        with patch.object(ctx, '_bus', return_value=b"(objectpath '/ambient',)\n"):
            with self.assertRaisesRegex(SmokeError, 'native_collection_invalid'):
                ctx.lock()
        with patch.object(ctx, '_bus', side_effect=results), \
                patch.object(ctx, 'provider', return_value={}):
            with self.assertRaisesRegex(SmokeError, 'native_lock_failed'):
                ctx.lock()

    def test_unlock_refuses_unlocked_or_uncertain_provider(self):
        ctx = NativeLinuxContext(Path(sys.executable).resolve())
        with patch.object(ctx, 'validate'), patch.object(ctx, 'provider', return_value={}):
            with self.assertRaisesRegex(SmokeError, 'native_unlock_state_invalid'):
                ctx.unlock()
        with patch.object(ctx, 'validate'), \
                patch.object(ctx, 'provider', side_effect=SmokeError('unavailable')):
            with self.assertRaisesRegex(SmokeError, 'native_unlock_state_invalid'):
                ctx.unlock()

    def test_uncertain_peer_cleanup_preserves_provider_state(self):
        ctx = NativeLinuxContext(Path(sys.executable).resolve())
        with tempfile.TemporaryDirectory() as directory:
            ctx.root = Path(directory)
            (ctx.root / 'owned-keyring').write_bytes(b'private state')
            ctx.retain_private_state()
            ctx.close()
            self.assertTrue((ctx.root / 'owned-keyring').exists())
            self.assertFalse(ctx.cleanup_complete)
            self.assertTrue(ctx.private_state_retained)

    def test_cleanup_refuses_changed_process_and_retains_root(self):
        ctx = NativeLinuxContext(Path(sys.executable).resolve())
        with tempfile.TemporaryDirectory() as directory:
            ctx.root = Path(directory)
            child = unittest.mock.Mock(pid=123)
            ctx.children = [(child, {'pid': 123})]
            with patch('marmish_native_validation.process_identity', return_value={'pid': 124}), \
                    patch('marmish_native_validation.os.killpg') as kill:
                with self.assertRaisesRegex(SmokeError, 'native_cleanup_incomplete'):
                    ctx.close()
                kill.assert_not_called()
                self.assertTrue(ctx.root.exists())



@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux process/FD acceptance')
class ProcessAuditTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.executable = Path(self.directory.name) / 'wnd'
        shutil.copyfile('/bin/sleep', self.executable)
        self.executable.chmod(0o700)

    def launch(self, *, env=None, stdin=subprocess.DEVNULL):
        child = subprocess.Popen([str(self.executable), '10'],
                                 env={'PATH': '/usr/bin:/bin'} if env is None else env,
                                 stdin=stdin, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        def cleanup():
            child.kill()
            child.wait()
            if child.stdin is not None:
                child.stdin.close()
        self.addCleanup(cleanup)
        return child

    def test_daemon_null_stdin_and_private_environment(self):
        child = self.launch()
        result = audit_native_process(child.pid, [bytes(range(32))], role='daemon')
        self.assertTrue(result['bootstrap_fd0_closed'])
        result = audit_tree(child.pid, [bytes(range(32))])
        self.assertEqual(result['daemons'], 1)
        self.assertEqual(result['workers'], 0)

    def test_open_bootstrap_pipe_rejected(self):
        child = self.launch(stdin=subprocess.PIPE)
        with self.assertRaisesRegex(SmokeError, 'native_bootstrap_fd_open'):
            audit_native_process(child.pid, [], role='daemon')

    def test_daemon_host_bus_discovery_rejected(self):
        for name in ('DBUS_SESSION_BUS_ADDRESS', 'GNOME_KEYRING_CONTROL', 'XDG_RUNTIME_DIR'):
            child = self.launch(env={name: 'private-but-unwanted'})
            with self.subTest(name=name), self.assertRaisesRegex(SmokeError, 'native_environment_leak'):
                audit_native_process(child.pid, [], role='daemon')

    def test_required_role_is_nonvacuous(self):
        child = self.launch()
        with self.assertRaisesRegex(SmokeError, 'native_audit_missing_worker'):
            audit_tree(child.pid, [], require_worker=True)
        with self.assertRaisesRegex(SmokeError, 'native_audit_wrong_executable'):
            audit_native_process(child.pid, [], role='worker')

    def test_descendant_identity_change_rejected(self):
        child = self.launch()
        import marmish_native_validation as validation
        original = validation.process_identity
        calls = 0
        def changed(pid):
            nonlocal calls
            result = original(pid)
            calls += 1
            if calls > 1:
                result['start'] += 1
            return result
        with patch.object(validation, 'process_identity', side_effect=changed):
            with self.assertRaisesRegex(SmokeError, 'native_scan_process_changed'):
                audit_tree(child.pid, [])


class FinalReportTests(unittest.TestCase):
    def invoke(self, *, teardown=False, retained=False, secret_report=False):
        import contextlib
        import io
        import types
        import marmish_native_validation as validation
        context = unittest.mock.Mock()
        context.env = {'PATH': '/usr/bin:/bin'}
        context.e2e_args.return_value = []
        context.secret_values = [b'x' * 32]
        context.private_state_retained = retained
        context.cleanup_complete = False
        context.__enter__ = unittest.mock.Mock(return_value=context)
        def close(*_):
            if teardown:
                raise SmokeError('native_cleanup_incomplete')
            context.cleanup_complete = not retained
        context.__exit__ = unittest.mock.Mock(side_effect=close)
        def e2e(_argv, *, report_sink):
            report_sink.append({'outcome': 'passed', 'assertions': ['checked'],
                                **({'bad': 'x' * 32} if secret_report else {})})
            return 0
        output = io.StringIO()
        with patch.object(validation, 'NativeLinuxContext', return_value=context), \
                patch.dict(sys.modules, {'marmish_e2e': types.SimpleNamespace(main=e2e)}), \
                contextlib.redirect_stdout(output):
            result = validation.main(['--helper', sys.executable])
        import json
        lines = output.getvalue().splitlines()
        self.assertEqual(len(lines), 1)
        return result, json.loads(lines[0])

    def test_report_emitted_only_after_successful_teardown(self):
        code, report = self.invoke()
        self.assertEqual(code, 0)
        self.assertEqual(report['outcome'], 'passed')
        self.assertEqual(report['provider_cleanup'], 'complete')

    def test_teardown_failure_preserves_assertions_and_fails(self):
        for kwargs in ({'teardown': True}, {'retained': True}):
            code, report = self.invoke(**kwargs)
            self.assertEqual(code, 1)
            self.assertEqual(report['outcome'], 'failed')
            self.assertEqual(report['category'], 'native_cleanup_incomplete')
            self.assertEqual(report['provider_cleanup'], 'incomplete')
            self.assertEqual(report['assertions'], ['checked'])

    def test_report_secret_is_discarded(self):
        code, report = self.invoke(secret_report=True)
        self.assertEqual(code, 1)
        self.assertNotIn('bad', report)

    def test_private_regular_log_scan_has_no_offset_side_effect(self):
        import marmish_native_validation as validation
        with tempfile.TemporaryFile() as log:
            log.write(b'fixed stage diagnostic')
            log.flush()
            before = log.tell()
            real_fstat = os.fstat
            with patch.object(validation.os, 'fstat', side_effect=lambda fd: real_fstat(log.fileno() if fd == 2 else fd)), \
                    patch.object(validation.os, 'open', side_effect=lambda path, flags: os.dup(log.fileno())):
                self.assertTrue(validation.scan_owned_stderr([b'x' * 32])['private_stderr_scanned'])
                self.assertEqual(log.tell(), before)
                log.write(b'x' * 32)
                log.flush()
                with self.assertRaisesRegex(SmokeError, 'native_secret_in_log'):
                    validation.scan_owned_stderr([b'x' * 32])

    def test_setup_signal_restores_handlers_and_emits_one_failure(self):
        import contextlib
        import io
        import signal
        import marmish_native_validation as validation
        before = signal.getsignal(signal.SIGTERM)
        output = io.StringIO()
        with patch.object(validation.NativeLinuxContext, '__enter__',
                          side_effect=lambda: validation._interrupt(signal.SIGTERM, None)), \
                contextlib.redirect_stdout(output):
            self.assertEqual(validation.main(['--helper', str(Path(sys.executable).resolve())]), 1)
        self.assertEqual(signal.getsignal(signal.SIGTERM), before)
        self.assertEqual(len(output.getvalue().splitlines()), 1)


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux owned processes')
class AdditionalProcessTests(unittest.TestCase):
    def test_identity_failure_reaps_unregistered_child(self):
        import marmish_native_validation as validation
        context = NativeLinuxContext(Path(sys.executable).resolve())
        children = []
        original = subprocess.Popen
        def spawn(*args, **kwargs):
            child = original(*args, **kwargs)
            children.append(child)
            return child
        with patch.object(validation.subprocess, 'Popen', side_effect=spawn), \
                patch.object(validation, 'process_identity', side_effect=SmokeError('identity_failed')):
            with self.assertRaisesRegex(SmokeError, 'identity_failed'):
                context._spawn([sys.executable, '-c', 'import time;time.sleep(10)'])
        self.assertEqual(context.children, [])
        self.assertIsNotNone(children[0].returncode)

    def test_missing_live_child_fd_is_not_skipped(self):
        import marmish_native_validation as validation
        root = os.getpid()
        identity = validation.process_identity(root)
        records = {root: {'pid': root, 'parent': 0, 'start': 1},
                   root + 1: {'pid': root + 1, 'parent': root, 'start': 2}}
        entries = [Path('/proc') / str(pid) for pid in records]
        with patch.object(validation.Path, 'iterdir', return_value=entries), \
                patch.object(validation, '_proc_record', side_effect=lambda pid: records[pid]), \
                patch.object(validation, 'process_identity', return_value=identity), \
                patch.object(validation, '_audit_process', side_effect=[None, FileNotFoundError()]), \
                patch.object(validation.Path, 'exists', return_value=True):
            with self.assertRaisesRegex(SmokeError, 'native_scan_process_changed'):
                validation.audit_tree(root, [], require_daemon=False)

    def test_exited_child_preserves_observed_security_findings(self):
        import marmish_native_validation as validation
        root = os.getpid()
        records = {root: {'pid': root, 'parent': 0, 'start': 1},
                   root + 1: {'pid': root + 1, 'parent': root, 'start': 2}}
        entries = [Path('/proc') / str(pid) for pid in records]
        for category in ('native_secret_in_process', 'native_environment_leak',
                         'native_bootstrap_fd_open', 'process_identity_failed',
                         'native_scan_process_changed'):
            with self.subTest(category=category), \
                    patch.object(validation.Path, 'iterdir', return_value=entries), \
                    patch.object(validation, '_proc_record', side_effect=lambda pid: records[pid]), \
                    patch.object(validation, 'process_identity', return_value={'pid': root}), \
                    patch.object(validation, '_audit_process',
                                 side_effect=[None, SmokeError(category)]), \
                    patch.object(validation.Path, 'exists', return_value=False):
                if category in ('process_identity_failed', 'native_scan_process_changed'):
                    result = validation.audit_tree(root, [], require_daemon=False)
                    self.assertEqual(result['processes'], 1)
                else:
                    with self.assertRaisesRegex(SmokeError, category):
                        validation.audit_tree(root, [], require_daemon=False)

    def test_bridge_requires_two_private_pipes(self):
        import marmish_native_validation as validation
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / 'marmish-engine-bridge'
            shutil.copyfile('/bin/sleep', binary)
            binary.chmod(0o700)
            for output in (subprocess.DEVNULL, subprocess.PIPE):
                child = subprocess.Popen([str(binary), '10'], env={}, stdin=subprocess.PIPE,
                                         stdout=output, stderr=subprocess.DEVNULL)
                try:
                    if output == subprocess.PIPE:
                        self.assertEqual(validation.audit_tree(child.pid, [], require_daemon=False)['bridges'], 1)
                    else:
                        with self.assertRaisesRegex(SmokeError, 'native_private_pipe_required'):
                            validation.audit_tree(child.pid, [], require_daemon=False)
                finally:
                    child.kill()
                    child.wait()
                    child.stdin.close()
                    if child.stdout:
                        child.stdout.close()

    def test_helper_inspection_precedes_protocol_request(self):
        import marmish_native_validation as validation
        def bounded(argv, env, payload, *, timeout, inspect):
            inspect(123)
            self.assertEqual(timeout, 5)
            return b'{"version":1,"ok":true,"result":{}}'
        with patch.object(validation, '_bounded', side_effect=bounded), \
                patch.object(validation, '_proc_record', return_value={'pid': 123}), \
                patch.object(validation, '_audit_process') as audit:
            self.assertTrue(validation.audit_helper(Path(sys.executable).resolve(), {}, [])['private_pipes'])
            audit.assert_called_once_with({'pid': 123}, [], 'helper')

if __name__ == '__main__':
    unittest.main()
