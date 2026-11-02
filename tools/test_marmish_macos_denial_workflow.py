"""Static workflow contracts; no CI dispatch or log access."""
from pathlib import Path
import unittest

import yaml


ROOT = Path(__file__).resolve().parent.parent


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.text = (ROOT / '.github/workflows/macos-probe-diagnostic.yml').read_text()
        self.workflow = yaml.safe_load(self.text)
        self.job = self.workflow['jobs']['diagnostic']

    def test_manual_host_permissions_pins_and_preserved_concurrency(self):
        triggers = self.workflow.get('on', self.workflow.get(True))
        self.assertEqual(triggers, {'workflow_dispatch': None})
        self.assertEqual(self.workflow['permissions'], {'contents': 'read'})
        self.assertEqual(self.workflow['concurrency'], {'group': 'marmish-macos-probe-diagnostic', 'cancel-in-progress': False})
        self.assertEqual(self.job['runs-on'], 'macos-15')
        self.assertEqual(self.job['timeout-minutes'], 10)
        for step in self.job['steps']:
            if 'uses' in step:
                self.assertRegex(step['uses'], r'^(actions/checkout|actions/upload-artifact)@[0-9a-f]{40}$')
        checkout = self.job['steps'][0]
        self.assertEqual(checkout['with'], {'persist-credentials': False})

    def test_denial_steps_are_independent_always_and_exact_report_only(self):
        steps = self.job['steps']
        preparation = next(step for step in steps if step.get('id') == 'denial_report')
        invocation = next(step for step in steps if 'marmish_macos_denial_diagnostic.py' in step.get('run', ''))
        upload = next(step for step in steps if step.get('with', {}).get('name') == 'macos-owned-probe-denial-candidate')
        self.assertLess(steps.index(preparation), steps.index(invocation))
        self.assertLess(steps.index(invocation), steps.index(upload))
        for step in (preparation, invocation, upload):
            self.assertEqual(step['if'], 'always()')
        self.assertIn('umask 077', preparation['run'])
        self.assertIn('mktemp -d "$RUNNER_TEMP/marmish-denial-diagnostic.XXXXXX"', preparation['run'])
        self.assertIn('path=%s/report.json', preparation['run'])
        self.assertEqual(invocation['env'], {'REPORT_PATH': '${{ steps.denial_report.outputs.path }}'})
        self.assertIn('python3 tools/marmish_macos_denial_diagnostic.py --output "$REPORT_PATH"', invocation['run'])
        self.assertIn('set -o noclobber', invocation['run'])
        self.assertEqual(upload['with'], {'name': 'macos-owned-probe-denial-candidate',
            'path': '${{ steps.denial_report.outputs.path }}', 'if-no-files-found': 'error', 'retention-days': 7})

    def test_existing_artifacts_and_no_raw_or_privileged_actions(self):
        artifacts = [step['with'] for step in self.job['steps'] if step.get('uses', '').startswith('actions/upload-artifact@')]
        self.assertEqual(len(artifacts), 3)
        self.assertEqual({item['name'] for item in artifacts}, {'macos-confined-probe-diagnostic',
            'macos-owned-probe-crash-context', 'macos-owned-probe-denial-candidate'})
        for item in artifacts:
            self.assertNotIn('*', item['path'])
            self.assertTrue(item['path'].endswith('.outputs.path }}'))
        for forbidden in ('sudo ', 'log stream', 'log config', 'secrets.', 'actions/cache', '*.ips',
                          'enable-private-data', 'entitlements', '/usr/bin/log'):
            self.assertNotIn(forbidden, self.text)
        # Serial worst-case children: old probe <=115s; old crash <=120s;
        # denial <=90s compilation +2s preflight +3*7s child budgets +20s crash.
        self.assertLess(115 + 120 + 90 + 2 + 3 * 7 + 20, 10 * 60)


if __name__ == '__main__':
    unittest.main()
