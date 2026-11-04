"""Offline checks for the deliberately single-candidate diagnostic workflow."""
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parents[1]

class DiagnosticWorkflow(unittest.TestCase):
    def setUp(self):
        self.source = (ROOT / '.github/workflows/v02-provider-diagnostic.yml').read_text()

    def test_closed_trigger_privileges_and_platform(self):
        self.assertIn('on:\n  workflow_dispatch:\npermissions:\n  contents: read\n  actions: read', self.source)
        for forbidden in ('inputs:', 'pull_request:', 'push:', 'write', 'secrets:', 'sudo ', 'curl '):
            self.assertNotIn(forbidden, self.source)
        self.assertIn('runs-on: macos-15', self.source)
        self.assertIn('timeout-minutes: 15', self.source)
        self.assertIn('cancel-in-progress: false', self.source)
        self.assertIn("if: github.ref == 'refs/heads/master'", self.source)
        self.assertIn('persist-credentials: false', self.source)
        self.assertIn('fetch-depth: 0', self.source)
        for action in re.findall(r'uses: (\S+)', self.source):
            self.assertRegex(action, r'^actions/[a-z-]+@[0-9a-f]{40}$')

    def test_exact_candidate_and_ordered_preexecution_guards(self):
        for name in ('v02-candidate-aarch64-apple-darwin-41898839617-1',
                     'v02-pinned-bootstrap-41898839617-1'):
            self.assertIn('name: ' + name, self.source)
        self.assertEqual(self.source.count("run-id: '41898839617'"), 2)
        self.assertEqual(self.source.count('repository: Epiphytic/marmish'), 2)
        stages = ['provider-diagnostic-validate', 'actions/download-artifact@',
                  'provider-diagnostic-assets', 'test_marmish_native_keychain_check.py',
                  'python3 tools/marmish_native_keychain_check.py']
        self.assertEqual([self.source.index(stage) for stage in stages],
                         sorted(self.source.index(stage) for stage in stages))
        self.assertIn('MARMISH_BUILD_COMMIT: 61f47606c5e7ba9048f196165a8025e7c2662839', self.source)
        self.assertIn("GH_TOKEN: ''", self.source)
        self.assertIn('--target aarch64-apple-darwin', self.source)
        self.assertIn('DIAGNOSTIC_ROOT=$RUNNER_TEMP/provider-diagnostic', self.source)

    def test_upload_is_separate_finite_and_missing_provider_report_is_allowed(self):
        self.assertIn('if: always()', self.source)
        self.assertIn('name: v02-provider-diagnostic-${{ github.run_id }}-${{ github.run_attempt }}', self.source)
        paths = self.source.split('          path: |\n')[1].split('          if-no-files-found:')[0]
        self.assertEqual(paths.splitlines(), [
            '            ${{ runner.temp }}/provider-diagnostic/provenance.json',
            '            ${{ runner.temp }}/provider-diagnostic/provider.json'])
        self.assertIn('if-no-files-found: warn', self.source)
        self.assertIn('retention-days: 7', self.source)
        self.assertNotIn('**', paths)
