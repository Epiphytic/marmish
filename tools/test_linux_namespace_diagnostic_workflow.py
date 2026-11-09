"""Offline contracts for the fixed host-Python ARM namespace lane."""
import ast
import contextlib
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import marmish_managed_runtime_spike as spike


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(contextlib.chdir(Path(__file__).resolve().parent.parent))
        self.text = (Path(__file__).resolve().parent.parent / '.github/workflows/linux-namespace-diagnostic.yml').read_text()
        self.steps = re.split(r'^      - ', self.text, flags=re.M)[1:]

    def test_fixture_pins_workflow_directory_and_restores_caller(self):
        repository = Path(__file__).resolve().parent.parent
        with tempfile.TemporaryDirectory() as temporary, contextlib.chdir(temporary):
            caller = Path(temporary).resolve()
            for fail in (False, True):
                with self.subTest(fail=fail):
                    case = WorkflowTests('test_manual_guard_exact_runner_permissions_and_concurrency')
                    try:
                        case.setUp()
                        self.assertEqual(Path.cwd(), repository)
                        self.assertEqual(Path('tools').resolve(), repository / 'tools')
                        if fail:
                            raise RuntimeError('simulated_test_body_failure')
                    except RuntimeError as error:
                        self.assertEqual(str(error), 'simulated_test_body_failure')
                    finally:
                        case.doCleanups()
                    self.assertEqual(Path.cwd(), caller)

    def run_text(self, step):
        match = re.search(r'^        run: (.*)\n', step, re.M)
        self.assertIsNotNone(match)
        if match[1] != '|':
            return match[1]
        return '\n'.join(line[10:] for line in step[match.end():].splitlines() if line.startswith('          ')) + '\n'

    def test_manual_guard_exact_runner_permissions_and_concurrency(self):
        self.assertIn('on:\n  workflow_dispatch:\n', self.text)
        self.assertNotIn('  push:', self.text)
        self.assertNotIn('  pull_request:', self.text)
        self.assertIn("    if: github.repository == 'Epiphytic/marmish' && github.ref == 'refs/heads/master' && github.event_name == 'workflow_dispatch'\n", self.text)
        self.assertIn('    runs-on:\n      group: epiphytic-ci\n      labels: [self-hosted, Linux, ARM64, laptop-ci]\n', self.text)
        self.assertEqual(len(re.findall(r'^\s*permissions:\n\s+contents: read$', self.text, re.M)), 2)
        self.assertIn('concurrency:\n  group: marmish-linux-namespace-diagnostic\n  cancel-in-progress: false\n', self.text)
        self.assertIn('    timeout-minutes: 22\n', self.text)

    def test_checkout_order_timeouts_and_owned_cleanup(self):
        self.assertEqual(len(self.steps), 6)
        self.assertTrue(self.steps[0].startswith('uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262\n'))
        self.assertIn('        id: checkout\n', self.steps[0])
        self.assertIn('          persist-credentials: false\n', self.steps[0])
        caps = [int(re.search(r'^        timeout-minutes: (\d+)$', step, re.M)[1]) for step in self.steps]
        self.assertEqual(caps, [2, 5, 6, 2, 2, 3])
        self.assertEqual(sum(caps), 20)
        self.assertEqual(self.run_text(self.steps[1]), 'sh tools/bootstrap_ci_python.sh')
        self.assertEqual(self.run_text(self.steps[2]), 'python3 tools/marmish_ci_bwrap.py bootstrap --state "$RUNNER_TEMP/namespace-diagnostic-bwrap-state"')
        self.assertEqual(self.run_text(self.steps[3]), 'python3 tools/marmish_ci_bwrap.py diagnostic-probe')
        self.assertIn('fixed legacy namespace integration contracts (host Python)', self.steps[4])
        cleanup = self.steps[5]
        self.assertIn('        if: always()\n', cleanup)
        for token in ("steps.checkout.outcome", 'cleanup_retained_unverified', 'cleanup_not_initialized',
                      '[ -L "$RUNNER_TEMP/namespace-diagnostic-bwrap-state" ]',
                      'python3 tools/marmish_ci_bwrap.py cleanup --state "$RUNNER_TEMP/namespace-diagnostic-bwrap-state"'):
            self.assertIn(token, cleanup)

    def test_no_tokens_privileged_changes_or_artifacts_and_shell_syntax(self):
        for token in ('GITHUB_TOKEN', 'github.token', 'secrets.', 'sudo ', 'sysctl ', 'continue-on-error', 'upload-artifact', 'actions: read'):
            self.assertNotIn(token, self.text)
        for step in self.steps[1:]:
            result = subprocess.run(['sh', '-n'], input=self.run_text(step), text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def wrapper(self):
        shell = self.run_text(self.steps[4])
        self.assertTrue(shell.startswith("python3 -I -B - <<'PY_NAMESPACE'\n"))
        return shell.split('\n', 1)[1].rsplit('PY_NAMESPACE\n', 1)[0]

    def test_wrapper_admits_only_exact_authenticated_pair_and_one_success_marker(self):
        source = self.wrapper()
        marker = b'MARMISH_LEGACY_NAMESPACE_RESULT count=2 skipped=0 failures=0 errors=0 ok=1\n'
        pair = {'MARMISH_BWRAP': '/bound/bwrap', 'MARMISH_BWRAP_SHA256': 'a' * 64}
        cases = [(pair, (0, marker), None, True), ({}, (0, marker), None, False),
                 ({'MARMISH_BWRAP': '/bound/bwrap'}, (0, marker), None, False),
                 ({**pair, 'EXTRA': 'private'}, (0, marker), None, False),
                 (pair, (0, marker * 2), None, False), (pair, (0, marker + b'MARMISH_LEGACY_NAMESPACE_RESULT bad\n'), None, False), (pair, (0, b''), None, False),
                 (pair, (0, marker.replace(b'count=2', b'count=1')), None, False),
                 (pair, (1, marker), None, False), (pair, (0, marker), '1', False)]
        for admitted, result, flag, success in cases:
            with self.subTest(pair=sorted(admitted), result=result[0], flag=flag, success=success), \
                    patch.dict(os.environ), patch.object(sys, 'path', list(sys.path)), \
                    patch.object(spike, 'verified_helper_pair', return_value=admitted) as verify, \
                    patch.object(spike, 'run', return_value=result) as run, \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                os.environ.pop('MARMISH_LEGACY_SKIP_NAMESPACE_TESTS', None)
                if flag is not None:
                    os.environ['MARMISH_LEGACY_SKIP_NAMESPACE_TESTS'] = flag
                if success:
                    exec(compile(source, '<fixed namespace wrapper>', 'exec'), {})
                    command, env = run.call_args.args
                    self.assertEqual(command[1:4], ['-I', '-B', '-c'])
                    self.assertTrue(Path(command[0]).is_absolute())
                    self.assertEqual(env['PATH'], '/usr/bin:/bin:/usr/sbin:/sbin')
                    self.assertEqual({key: env[key] for key in pair}, pair)
                    self.assertNotIn('MARMISH_LEGACY_SKIP_NAMESPACE_TESTS', env)
                    self.assertEqual(run.call_args.kwargs, {'timeout': 90, 'capture': True})
                else:
                    with self.assertRaises(SystemExit) as stopped:
                        exec(compile(source, '<fixed namespace wrapper>', 'exec'), {})
                    self.assertEqual(stopped.exception.code, 1)
                self.assertEqual(json.loads(output.getvalue())['ok'], success)
                if flag is not None:
                    verify.assert_not_called()
                elif admitted != pair:
                    run.assert_not_called()
                else:
                    verify.assert_called_once_with()

    def test_child_payload_is_fixed_two_with_no_skip_or_error_success(self):
        tree = ast.parse(self.wrapper())
        payload = next(node.value.value for node in ast.walk(tree) if isinstance(node, ast.Assign)
                       and any(isinstance(target, ast.Name) and target.id == 'code' for target in node.targets))
        names = ['test_marmish_bootstrap.LinuxHandoffTests.test_actual_unshare_bwrap_inner_chain_and_public_configuration',
                 'test_marmish_transport_sandbox.NamespaceTests.test_approved_bridge_decoy_denial_capabilities_and_cleanup']
        for field in (None, 'suite_count', 'testsRun', 'skipped', 'failures', 'errors', 'expectedFailures', 'unexpectedSuccesses', 'unsuccessful'):
            result = Mock(testsRun=2, skipped=[], failures=[], errors=[], expectedFailures=[], unexpectedSuccesses=[])
            result.wasSuccessful.return_value = field != 'unsuccessful'
            if field == 'testsRun': result.testsRun = 1
            elif field not in (None, 'suite_count', 'unsuccessful'): setattr(result, field, ['failure'])
            suite = Mock(); suite.countTestCases.return_value = 1 if field == 'suite_count' else 2
            with self.subTest(field=field), patch.object(sys, 'argv', ['fixed', '/source/tools']), \
                    patch.object(sys, 'path', list(sys.path)), \
                    patch.object(unittest.defaultTestLoader, 'loadTestsFromNames', return_value=suite) as loader, \
                    patch.object(unittest, 'TextTestRunner') as runner, contextlib.redirect_stdout(io.StringIO()) as output:
                runner.return_value.run.return_value = result
                if field == 'suite_count':
                    with self.assertRaises(AssertionError):
                        exec(compile(payload, '<fixed two-test child>', 'exec'), {})
                    runner.assert_not_called()
                else:
                    with self.assertRaises(SystemExit) as stopped:
                        exec(compile(payload, '<fixed two-test child>', 'exec'), {})
                    self.assertEqual(stopped.exception.code, 0 if field is None else 1)
                self.assertEqual(bool(output.getvalue()), field is None)
                loader.assert_called_once_with(names)

    def test_early_wrapper_resolution_and_import_failures_stay_generic(self):
        import builtins
        source = self.wrapper()
        original_import = builtins.__import__
        def import_failure(name, *args, **kwargs):
            if name == 'marmish_managed_runtime_spike':
                raise ImportError('private/path/import')
            return original_import(name, *args, **kwargs)
        for failure in (patch.object(Path, 'resolve', side_effect=OSError('private/path/resolve')),
                        patch.object(builtins, '__import__', side_effect=import_failure)):
            with self.subTest(failure=failure.attribute), failure, \
                    patch.object(sys, 'path', list(sys.path)), \
                    patch.object(spike, 'run') as run, contextlib.redirect_stdout(io.StringIO()) as output:
                with self.assertRaises(SystemExit) as stopped:
                    exec(compile(source, '<fixed namespace wrapper>', 'exec'), {})
                self.assertEqual(stopped.exception.code, 1)
                self.assertEqual(json.loads(output.getvalue())['category'], 'namespace_lane_failed')
                self.assertNotIn('private', output.getvalue())
                run.assert_not_called()

    def test_unknown_wrapper_exceptions_never_publish_exception_text(self):
        for error in (spike.Refused('private/path/secret'), OSError('private/path/secret')):
            with self.subTest(error=type(error).__name__), patch.dict(os.environ), \
                    patch.object(sys, 'path', list(sys.path)), \
                    patch.object(spike, 'verified_helper_pair', side_effect=error), \
                    patch.object(spike, 'run') as run, contextlib.redirect_stdout(io.StringIO()) as output:
                os.environ.pop('MARMISH_LEGACY_SKIP_NAMESPACE_TESTS', None)
                with self.assertRaises(SystemExit):
                    exec(compile(self.wrapper(), '<fixed namespace wrapper>', 'exec'), {})
                self.assertEqual(json.loads(output.getvalue())['category'], 'namespace_lane_failed')
                self.assertNotIn('private', output.getvalue())
                run.assert_not_called()
