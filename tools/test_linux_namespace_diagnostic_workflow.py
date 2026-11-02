"""Offline contracts for the keyless ARM namespace diagnostic."""
from pathlib import Path
import subprocess
import unittest

import yaml


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.text = (Path(__file__).resolve().parent.parent / '.github/workflows/linux-namespace-diagnostic.yml').read_text()
        self.workflow = yaml.safe_load(self.text)
        self.job = self.workflow['jobs']['diagnostic']

    def test_manual_guard_exact_runner_permissions_and_concurrency(self):
        self.assertEqual(self.workflow.get('on', self.workflow.get(True)), {'workflow_dispatch': None})
        self.assertEqual(self.job['if'], "github.repository == 'Epiphytic/marmish' && github.ref == 'refs/heads/master' && github.event_name == 'workflow_dispatch'")
        self.assertEqual(self.job['runs-on'], {'group': 'epiphytic-ci', 'labels': ['self-hosted', 'Linux', 'ARM64', 'laptop-ci']})
        for value in (self.workflow, self.job):
            self.assertEqual(value['permissions'], {'contents': 'read'})
        self.assertEqual(self.workflow['concurrency'], {'group': 'marmish-linux-namespace-diagnostic', 'cancel-in-progress': False})
        self.assertEqual(self.job['timeout-minutes'], 18)

    def test_checkout_order_timeouts_and_owned_cleanup(self):
        steps = self.job['steps']
        self.assertEqual(steps[0], {'uses': 'actions/checkout@11d5960a326750d5838078e36cf38b85af677262',
            'id': 'checkout', 'timeout-minutes': 2, 'with': {'persist-credentials': False}})
        self.assertEqual([step['timeout-minutes'] for step in steps], [2, 5, 6, 2, 3])
        self.assertEqual(steps[1]['run'], 'sh tools/bootstrap_ci_python.sh')
        self.assertEqual(steps[2]['run'], 'python3 tools/marmish_ci_bwrap.py bootstrap --state "$RUNNER_TEMP/namespace-diagnostic-bwrap-state"')
        self.assertEqual(steps[3]['run'], 'python3 tools/marmish_ci_bwrap.py diagnostic-probe')
        cleanup = steps[4]
        self.assertEqual(cleanup['if'], 'always()')
        for token in ("steps.checkout.outcome", 'cleanup_retained_unverified', 'cleanup_not_initialized',
                '[ -L "$RUNNER_TEMP/namespace-diagnostic-bwrap-state" ]',
                'python3 tools/marmish_ci_bwrap.py cleanup --state "$RUNNER_TEMP/namespace-diagnostic-bwrap-state"'):
            self.assertIn(token, cleanup['run'])

    def test_no_tokens_privileged_changes_or_artifacts_and_shell_syntax(self):
        for token in ('GITHUB_TOKEN', 'github.token', 'secrets.', 'sudo ', 'sysctl ', 'continue-on-error', 'upload-artifact', 'actions: read'):
            self.assertNotIn(token, self.text)
        for step in self.job['steps']:
            if 'run' in step:
                result = subprocess.run(['sh', '-n'], input=step['run'], text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
