"""Static trust boundaries of native release automation (no network)."""
import pathlib
import re
import unittest
import yaml
ROOT = pathlib.Path(__file__).resolve().parents[1]
class ReleaseAutomation(unittest.TestCase):
    def setUp(self):
        self.text = (ROOT / '.github/workflows/v02-publish.yml').read_text()
        self.flow = yaml.safe_load(self.text)
    def test_four_job_permissions_and_triggers(self):
        self.assertEqual(set(self.flow['jobs']), {'inspect','reproduce-bootstrap','verify','publish'})
        self.assertEqual(self.flow['permissions'], {})
        self.assertEqual(self.flow['jobs']['reproduce-bootstrap']['permissions'], {})
        for job in ('inspect','verify'):
            self.assertEqual(self.flow['jobs'][job]['permissions'], {'contents':'read','actions':'read'})
        self.assertEqual(self.flow['jobs']['publish']['permissions'], {'contents':'write','actions':'read'})
        events = self.flow.get('on', self.flow.get(True))
        self.assertEqual(set(events), {'workflow_run','workflow_dispatch'})
        self.assertEqual(events['workflow_run']['workflows'], ['v0.2 native release candidates'])
        self.assertFalse(self.flow['concurrency']['cancel-in-progress'])
    def test_pinned_actions_no_write_job_dependency_or_candidate_execution(self):
        for job in self.flow['jobs'].values():
            for step in job['steps']:
                if 'uses' in step:
                    self.assertRegex(step['uses'], r'@[0-9a-f]{40}$')
                self.assertNotIn('${{', step.get('run',''))
        write = self.flow['jobs']['publish']
        commands = '\n'.join(step.get('run','') for step in write['steps'])
        self.assertNotRegex(commands, r'\b(npm|cargo|python|curl|rad)\b')
        self.assertIn('node tools/v02-actions/publish.mjs',commands)
        self.assertNotIn('radicle',self.text.lower())
    def test_exact_artifact_handoffs_and_renderer_has_no_repository_token(self):
        for name,job in self.flow['jobs'].items():
            for step in job['steps']:
                if 'actions/download-artifact@' in step.get('uses',''):
                    self.assertIn('artifact-ids',step['with'])
                    self.assertNotIn('pattern',step['with'])
                if name == 'reproduce-bootstrap':
                    self.assertNotIn('GH_TOKEN',step.get('env',{}))
                    self.assertNotIn('github.token',str(step))
                    self.assertNotIn('actions/checkout@',step.get('uses',''))
    def test_candidate_source_fallback_and_explicit_version_trigger(self):
        text = (ROOT / '.github/workflows/v02-release.yml').read_text()
        flow = yaml.safe_load(text)
        events = flow.get('on',flow.get(True))
        self.assertEqual(events['push'], {'branches':['master'],'paths':['crates/marmish-native/Cargo.toml']})
        self.assertNotRegex(text,r'\$\{\{ inputs.source_commit \}\}')
        self.assertEqual(flow['jobs']['package']['needs'],'release-eligibility')
        self.assertIn("== 'build'",flow['jobs']['package']['if'])
        self.assertIn('release-eligibility',flow['jobs'])
if __name__ == '__main__':
    unittest.main()
