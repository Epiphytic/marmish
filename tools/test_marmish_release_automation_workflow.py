"""Static trust boundaries of the fixed native workflows; stdlib only."""
import pathlib
import re
import unittest
from unittest.mock import patch

from test_marmish_managed_workflow import step_blocks as steps

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCES = {}  # Test-owned overrides; mutations never write workflow files.


def source(name):
    return SOURCES[name] if name in SOURCES else (ROOT / '.github/workflows' / name).read_text()


def block(text, header):
    """Select one literal, indentation-bounded section of the checked-in layout."""
    matches = list(re.finditer(r'^' + re.escape(header) + r'$', text, re.M))
    if len(matches) != 1:
        raise AssertionError('required unique workflow section: ' + header)
    start = matches[0].end()
    indent = len(header) - len(header.lstrip())
    end = re.search(r'^ {0,' + str(indent) + r'}\S', text[start:], re.M)
    return text[matches[0].start():start + end.start() if end else len(text)].rstrip() + '\n'


def fields(text, indent):
    pairs = []
    for match in re.finditer(r'^' + ' ' * indent + r'([A-Za-z_][A-Za-z0-9_-]*):(?: (.*))?$', text, re.M):
        key, value = match[1], match[2] or ''
        # Admit only the checked-in single-line scalars and literal run bodies;
        # refuse unsupported YAML forms instead of interpreting them partially.
        if value and value != '|':
            if value.startswith(('"', "'", '|', '>')):
                raise AssertionError('unsupported workflow scalar')
            following = next((line for line in text[match.end():].splitlines()
                              if line.strip() and not line.lstrip().startswith('#')), '')
            if following and len(following) - len(following.lstrip()) > indent:
                raise AssertionError('workflow scalar continuation')
        pairs.append((key, value))
    if len(dict(pairs)) != len(pairs):
        raise AssertionError('duplicate workflow field')
    return dict(pairs)


def run_text(step):
    value = fields(step, 8).get('run', '')
    if value == '|':
        body = block(step, '        run: |').splitlines()[1:]
        if any(line.strip() and not line.startswith(' ' * 10) for line in body):
            raise AssertionError('unsupported workflow scalar')
        return '\n'.join(line[10:] for line in body)
    return value


class ReleaseAutomation(unittest.TestCase):
    def setUp(self):
        self.text = source('v02-publish.yml')
        fields(self.text, 0)
        jobs = block(self.text, 'jobs:')
        self.jobs = {name: block(jobs, '  ' + name + ':')
                     for name in fields(jobs, 2)}
        for job in self.jobs.values():
            fields(job, 4)
            block(job, '    steps:')

    def test_four_job_permissions_and_triggers(self):
        self.assertEqual(set(self.jobs), {'inspect', 'reproduce-bootstrap', 'verify', 'publish'})
        self.assertEqual(block(self.text, 'permissions: {}'), 'permissions: {}\n')
        self.assertEqual(block(self.jobs['reproduce-bootstrap'], '    permissions: {}'), '    permissions: {}\n')
        for job in ('inspect', 'verify', 'publish'):
            self.assertEqual(fields(block(self.jobs[job], '    permissions:'), 6),
                             {'contents': 'write' if job == 'publish' else 'read', 'actions': 'read'})
        events = block(self.text, 'on:')
        self.assertEqual(set(fields(events, 2)), {'workflow_run', 'workflow_dispatch'})
        self.assertEqual(fields(block(events, '  workflow_run:'), 4)['workflows'], '[v0.2 native release candidates]')
        self.assertEqual(fields(block(self.text, 'concurrency:'), 2)['cancel-in-progress'], 'false')

    def test_pinned_actions_no_write_job_dependency_or_candidate_execution(self):
        identities = {
            'inspect': ['uses: actions/checkout@', 'name: Install locked read-only archive tooling',
                        'name: Inspect completed candidate without executing artifacts', 'uses: actions/upload-artifact@'],
            'reproduce-bootstrap': ['name: Fetch trusted publisher orchestration without credentials',
                'uses: actions/download-artifact@', 'name: Reproduce bootstrap with credential-free child environment',
                'uses: actions/upload-artifact@'],
            'verify': ['uses: actions/checkout@', 'uses: actions/download-artifact@', 'uses: actions/download-artifact@',
                'working-directory: tools/v02-actions', 'name: Independently revalidate candidate and reproduction',
                'uses: actions/upload-artifact@'],
            'publish': ['uses: actions/checkout@', 'uses: actions/download-artifact@', 'uses: actions/download-artifact@',
                'name: Publish immutable GitHub assets and fast-forward installer', 'uses: actions/upload-artifact@'],
        }
        for name, job in self.jobs.items():
            selected = steps(job)
            self.assertEqual(len(selected), len(identities[name]))
            for step, expected in zip(selected, identities[name]):
                first = step.splitlines()[0]
                if expected.endswith('@'):
                    self.assertRegex(first, '^' + re.escape(expected) + r'[0-9a-f]{40}$')
                else:
                    self.assertEqual(first, expected)
                actions = re.findall(r'^(?:uses:|        uses:) (.*)$', step, re.M)
                for action in actions:
                    self.assertRegex(action, r'@[0-9a-f]{40}$')
                self.assertNotIn('${{', run_text(step))
        commands = '\n'.join(run_text(step) for step in steps(self.jobs['publish']))
        self.assertNotRegex(commands, r'\b(npm|cargo|python|curl|rad)\b')
        self.assertIn('node tools/v02-actions/publish.mjs', commands)
        self.assertNotIn('radicle', self.text.lower())

    def test_exact_artifact_handoffs_and_renderer_has_no_repository_token(self):
        for name, job in self.jobs.items():
            for step in steps(job):
                if re.search(r'^(?:uses:|        uses:) actions/download-artifact@', step, re.M):
                    options = fields(block(step, '        with:'), 10)
                    self.assertIn('artifact-ids', options)
                    self.assertNotIn('pattern', options)
                if name == 'reproduce-bootstrap':
                    self.assertNotIn('GH_TOKEN', step)
                    self.assertNotIn('github.token', step)
                    self.assertNotRegex(step, re.compile(r'^(?:uses:|        uses:) actions/checkout@', re.M))

    def test_candidate_source_fallback_and_explicit_version_trigger(self):
        text = source('v02-release.yml')
        self.assertEqual(fields(block(block(text, 'on:'), '  push:'), 4),
                         {'branches': '[master]', 'paths': '[crates/marmish-native/Cargo.toml]'})
        self.assertNotRegex(text, r'\$\{\{ inputs.source_commit \}\}')
        package = block(block(text, 'jobs:'), '  package:')
        self.assertEqual(fields(package, 4)['needs'], 'release-eligibility')
        self.assertIn("== 'build'", fields(package, 4)['if'])
        self.assertIn('release-eligibility', fields(block(text, 'jobs:'), 2))


class MutationTests(unittest.TestCase):
    def check_source(self, name, before, after, passes=False):
        original = source(name)
        self.assertIn(before, original)
        with patch.dict(SOURCES, {name: original.replace(before, after, 1)}):
            result = unittest.TestResult()
            unittest.defaultTestLoader.loadTestsFromTestCase(ReleaseAutomation).run(result)
        self.assertEqual(result.testsRun, 4)
        self.assertEqual(result.skipped, [])
        self.assertEqual(result.wasSuccessful(), passes, (result.errors, result.failures))

    def test_publication_security_mutations(self):
        changes = [
            ('permissions: {}', '# permissions: {}'),
            ('      contents: write', '      contents: read'),
            ('      actions: read', '        actions: read'),
            ('  cancel-in-progress: false', '  cancel-in-progress: true'),
            ('actions/checkout@11d5960a326750d5838078e36cf38b85af677262', 'actions/checkout@main'),
            ('        run: npm ci --ignore-scripts', '        run: echo ${{ github.token }}'),
            ('          artifact-ids:', '          pattern: ignored\n          artifact-ids:'),
            ('          PUBLISHER_SHA:', '          GH_TOKEN: private\n          PUBLISHER_SHA:'),
            ('  workflow_run:', '  push:'),
        ]
        for before, after in changes:
            with self.subTest(before=before):
                self.check_source('v02-publish.yml', before, after)
        self.check_source('v02-publish.yml', 'name: Publish qualified native release',
                          '# benign comment\nname: Publish qualified native release', passes=True)

    def test_candidate_security_mutations(self):
        for before, after in [('branches: [master]', 'branches: [other]'),
                              ('needs: release-eligibility', 'needs: unrelated'),
                              ("== 'build'", "== 'skip'"),
                              ('${{ inputs.source_commit || github.sha }}', '${{ inputs.source_commit }}')]:
            with self.subTest(before=before):
                self.check_source('v02-release.yml', before, after)

    def test_scalar_continuations_and_quoted_tokens_refuse(self):
        for before, after in [
            ('        run: npm ci --ignore-scripts', '        run: >-\n          echo ${{ github.token }}'),
            ('        run: npm ci --ignore-scripts', '        run: |-\n          echo ${{ github.token }}'),
            ('        run: npm ci --ignore-scripts', '        run: npm ci --ignore-scripts\n          ${{ github.token }}'),
            ('        run: npm ci --ignore-scripts', '        run: "echo\n          ${{ github.token }}"'),
            ('        run: |\n          node tools/v02-actions/release-handoff.mjs',
             '        run: node tools/v02-actions/publish.mjs\n          curl private'),
            ('        run: |\n          node tools/v02-actions/release-handoff.mjs',
             '        run: |\n         curl private\n          node tools/v02-actions/release-handoff.mjs'),
            ('        run: npm ci --ignore-scripts',
             '        run: |\n         ${{ github.token }}\n          npm ci --ignore-scripts'),
            ('          PUBLISHER_SHA:', '            GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}\n          PUBLISHER_SHA:'),
            ('          PUBLISHER_SHA:', '          "GH_TOKEN": ${{ secrets.GITHUB_TOKEN }}\n          PUBLISHER_SHA:'),
        ]:
            with self.subTest(before=before, after=after):
                self.check_source('v02-publish.yml', before, after)


if __name__ == '__main__':
    unittest.main()
