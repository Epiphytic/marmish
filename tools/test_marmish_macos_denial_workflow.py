"""Static workflow contracts; stdlib only, no CI dispatch or log access."""
import re
import unittest
from unittest.mock import patch

from test_marmish_release_automation_workflow import SOURCES, block, fields, run_text, source, steps


class WorkflowTests(unittest.TestCase):
    def setUp(self):
        self.text = source('macos-probe-diagnostic.yml')
        fields(self.text, 0)
        self.job = block(block(self.text, 'jobs:'), '  diagnostic:')
        fields(self.job, 4)
        self.steps = steps(block(self.job, '    steps:'))

    def test_manual_host_permissions_pins_and_preserved_concurrency(self):
        self.assertEqual(block(self.text, 'on:'), 'on:\n  workflow_dispatch:\n')
        self.assertEqual(fields(block(self.text, 'permissions:'), 2), {'contents': 'read'})
        self.assertEqual(fields(block(self.text, 'concurrency:'), 2),
                         {'group': 'marmish-macos-probe-diagnostic', 'cancel-in-progress': 'false'})
        self.assertEqual(fields(self.job, 4)['runs-on'], 'macos-15')
        self.assertEqual(fields(self.job, 4)['timeout-minutes'], '10')
        identities = [
            'uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262',
            'name: Prepare private report directory', 'name: Run disposable native probe diagnostic',
            'name: Preserve bounded diagnostic report', 'name: Prepare private owned-crash report directory',
            'name: Inspect owned synthetic probe crash', 'name: Preserve bounded owned-crash context',
            'name: Prepare private owned-denial report directory',
            'name: Inspect owned synthetic probe denial candidate',
            'name: Preserve bounded owned-denial candidate report',
        ]
        self.assertEqual([step.splitlines()[0] for step in self.steps], identities)
        for step in self.steps:
            for action in re.findall(r'^(?:uses:|        uses:) (.*)$', step, re.M):
                self.assertRegex(action, r'^(actions/checkout|actions/upload-artifact)@[0-9a-f]{40}$')
        self.assertEqual(fields(block(self.steps[0], '        with:'), 10), {'persist-credentials': 'false'})

    def test_denial_steps_are_independent_always_and_exact_report_only(self):
        preparation, invocation, upload = self.steps[7:10]
        self.assertEqual(fields(preparation, 8)['id'], 'denial_report')
        for step in (preparation, invocation, upload):
            self.assertEqual(fields(step, 8)['if'], 'always()')
        self.assertIn('umask 077', run_text(preparation))
        self.assertIn('mktemp -d "$RUNNER_TEMP/marmish-denial-diagnostic.XXXXXX"', run_text(preparation))
        self.assertIn('path=%s/report.json', run_text(preparation))
        self.assertEqual(fields(block(invocation, '        env:'), 10),
                         {'REPORT_PATH': '${{ steps.denial_report.outputs.path }}'})
        self.assertIn('python3 tools/marmish_macos_denial_diagnostic.py --output "$REPORT_PATH"', run_text(invocation))
        self.assertIn('set -o noclobber', run_text(invocation))
        self.assertEqual(fields(block(upload, '        with:'), 10), {
            'name': 'macos-owned-probe-denial-candidate', 'path': '${{ steps.denial_report.outputs.path }}',
            'if-no-files-found': 'error', 'retention-days': '7'})

    def test_existing_artifacts_and_no_raw_or_privileged_actions(self):
        artifacts = [fields(block(step, '        with:'), 10) for step in self.steps
                     if re.search(r'^(?:uses:|        uses:) actions/upload-artifact@', step, re.M)]
        self.assertEqual(len(artifacts), 3)
        self.assertEqual({item['name'] for item in artifacts}, {'macos-confined-probe-diagnostic',
            'macos-owned-probe-crash-context', 'macos-owned-probe-denial-candidate'})
        for item in artifacts:
            self.assertNotIn('*', item['path'])
            self.assertTrue(item['path'].endswith('.outputs.path }}'))
        for forbidden in ('sudo ', 'log stream', 'log config', 'secrets.', 'actions/cache', '*.ips',
                          'enable-private-data', 'entitlements', '/usr/bin/log'):
            self.assertNotIn(forbidden, self.text)
        self.assertLess(115 + 120 + 90 + 2 + 3 * 7 + 20, 10 * 60)


class MutationTests(unittest.TestCase):
    def check_source(self, before, after, passes=False):
        name = 'macos-probe-diagnostic.yml'
        original = source(name)
        self.assertIn(before, original)
        with patch.dict(SOURCES, {name: original.replace(before, after, 1)}):
            result = unittest.TestResult()
            unittest.defaultTestLoader.loadTestsFromTestCase(WorkflowTests).run(result)
        self.assertEqual(result.testsRun, 3)
        self.assertEqual(result.skipped, [])
        self.assertEqual(result.wasSuccessful(), passes, (result.errors, result.failures))

    def test_scoped_workflow_mutations(self):
        for before, after in [
            ('  contents: read', '    contents: read'),
            ('  cancel-in-progress: false', '  cancel-in-progress: true'),
            ('          persist-credentials: false', '          persist-credentials: true'),
            ('        id: denial_report\n        if: always()', '        id: denial_report\n        # if: always()'),
            ('name: Inspect owned synthetic probe denial candidate\n        if: always()',
             'name: Inspect owned synthetic probe denial candidate\n          if: always()'),
            ('REPORT_PATH: ${{ steps.denial_report.outputs.path }}', 'REPORT_PATH: ${{ steps.report.outputs.path }}'),
            ('name: macos-owned-probe-denial-candidate', 'name: unrelated-report'),
            ('actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02', 'actions/upload-artifact@main'),
            ('name: Prepare private owned-denial report directory', 'name: Inspect owned synthetic probe denial candidate'),
        ]:
            with self.subTest(before=before):
                self.check_source(before, after)
        extra = ('      - name: Unexpected artifact\n'
                 '        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02\n'
                 '        with:\n          name: unexpected\n          path: unexpected.outputs.path }}\n')
        self.check_source('  loader-matrix:', extra + '  loader-matrix:')
        self.check_source('name: macOS confined probe diagnostic',
                          '# benign comment\nname: macOS confined probe diagnostic', passes=True)

    def test_always_scalar_continuation_refuses(self):
        self.check_source('        id: denial_report\n        if: always()',
                          '        id: denial_report\n        if: always()\n          && false')


if __name__ == '__main__':
    unittest.main()
