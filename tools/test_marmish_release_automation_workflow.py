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

    def test_split_candidate_dependencies_and_bounded_runners(self):
        text = source('v02-release.yml')
        jobs = block(text, 'jobs:')
        self.assertEqual(set(fields(jobs, 2)), {'release-eligibility', 'package',
                         'installed-tools', 'render', 'install-quickstart', 'provider-qualification'})
        self.assertEqual(block(text, 'env:').count('  MARMISH_CANDIDATE_LAYOUT: split-v1\n'), 1)
        for name in ('package', 'installed-tools'):
            job = block(jobs, '  ' + name + ':')
            options = fields(job, 4)
            self.assertEqual(options['needs'], 'release-eligibility')
            expected = "needs.release-eligibility.outputs.decision == 'build'"
            if name == 'installed-tools':
                expected += ' && !inputs.caveated_release'
            self.assertEqual(options['if'], expected)
            strategy = fields(block(job, '    strategy:'), 6)
            self.assertEqual(strategy['max-parallel'], '3')
            self.assertEqual(strategy['fail-fast'], 'false')
            self.assertEqual(fields(block(job, '    env:'), 6)['MARMISH_BUILD_COMMIT'],
                             '${{ inputs.source_commit || github.sha }}')
            self.assertNotIn('CARGO_TARGET_DIR', job)
        tools = block(jobs, '  installed-tools:')
        matrix = block(block(tools, '    strategy:'), '      matrix:')
        entries = re.findall(r'^          - runner: ([^\n]+)\n            target: ([^\n]+)$', matrix, re.M)
        self.assertEqual(entries, [('ubuntu-24.04', 'x86_64-unknown-linux-gnu'),
                                  ('ubuntu-24.04-arm', 'aarch64-unknown-linux-gnu'),
                                  ('macos-15', 'aarch64-apple-darwin')])
        self.assertEqual(len(matrix.splitlines()), 8)
        render = fields(block(jobs, '  render:'), 4)
        self.assertEqual(render['needs'], '[package, installed-tools]')
        self.assertEqual(render['if'], "${{ !cancelled() && needs.package.result == 'success' && (needs.installed-tools.result == 'success' || (inputs.caveated_release && needs.installed-tools.result == 'skipped')) }}")
        self.assertEqual(fields(block(text, 'concurrency:'), 2)['cancel-in-progress'], 'false')

    def test_candidate_cache_is_dependency_only_and_precedes_all_cargo(self):
        text = source('v02-release.yml')
        jobs = block(text, 'jobs:')
        for name in fields(jobs, 2):
            job = block(jobs, '  ' + name + ':')
            selected = steps(job)
            cache = [s for s in selected if 'uses: Swatinem/rust-cache@' in s]
            if name not in ('package', 'installed-tools'):
                self.assertEqual(cache, [])
                continue
            self.assertEqual(len(cache), 1)
            self.assertIn('uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0', cache[0])
            options = fields(block(cache[0], '        with:'), 10)
            self.assertEqual(options['prefix-key'], 'marmish-v02-release-split-v2')
            for option in ('cache-all-crates', 'cache-workspace-crates', 'cache-bin', 'cache-on-failure'):
                self.assertEqual(options[option], 'false')
            self.assertEqual(options['cache-targets'], 'true' if name == 'package' else 'false')
            self.assertEqual(options['save-if'], "${{ github.ref == 'refs/heads/master' && !inputs.caveated_release }}")
            self.assertEqual(set(options['env-vars'].split()),
                             {'MACOSX_DEPLOYMENT_TARGET', 'SDKROOT', 'ImageOS', 'ImageVersion'})
            self.assertIn(name, options['key'])
            self.assertIn('${{ matrix.runner }}', options['key'])
            self.assertIn('${{ matrix.target }}', options['key'])
            self.assertNotIn('shared-key', options)
            self.assertNotIn('cache-directories', options)
            self.assertNotIn('MARMISH_BUILD_COMMIT', str(options))
            before = selected[:selected.index(cache[0])]
            self.assertTrue(any('dtolnay/rust-toolchain@59a558e8c538bf4a23bbffa19e5a278b703f2920' in s
                                and 'toolchain: 1.97.1' in s for s in before))
            self.assertTrue(any("MACOSX_DEPLOYMENT_TARGET=15.0" in s
                                and 'RUST_TEST_THREADS=1' in s for s in before))
            self.assertFalse(any(re.search(r'\bcargo (test|clippy|build)\b', run_text(s)) for s in before))

    def test_candidate_cache_namespace_and_existing_writers_are_pinned(self):
        prefix = 'marmish-v02-release-split-v2'
        paths = {p.name for p in (ROOT / '.github/workflows').glob('*.yml')} | set(SOURCES)
        for name in paths:
            text = source(name)
            if name != 'v02-release.yml':
                self.assertNotIn(prefix, text)
            if name in ('v02-native.yml', 'v02-provider.yml'):
                for action in re.findall(r'uses: ((?:Swatinem/rust-cache|dtolnay/rust-toolchain)@[^\s]+)', text):
                    self.assertRegex(action, r'@[0-9a-f]{40}$')
                for step in steps(text):
                    if 'uses: dtolnay/rust-toolchain@' in step:
                        self.assertIn('toolchain: 1.97.1', step)

    def test_candidate_artifacts_have_a_single_owned_uploader(self):
        jobs = block(source('v02-release.yml'), 'jobs:')
        owners = {}
        for name in fields(jobs, 2):
            for step in steps(block(jobs, '  ' + name + ':')):
                if 'uses: actions/upload-artifact@' not in step:
                    continue
                artifact = fields(block(step, '        with:'), 10)['name']
                self.assertNotIn(artifact, owners)
                owners[artifact] = name
        expected = {'v02-eligibility': 'release-eligibility', 'v02-candidate': 'package',
                    'v02-render-tool': 'package', 'v02-installed-tools': 'installed-tools',
                    'v02-pinned-bootstrap': 'render', 'v02-quickstart': 'install-quickstart',
                    'v02-provider': 'provider-qualification'}
        self.assertEqual(len(owners), len(expected))
        for prefix, owner in expected.items():
            self.assertEqual([v for k, v in owners.items() if k.startswith(prefix + '-')], [owner])

    def test_both_feature_lanes_keep_full_workloads_and_physical_isolation(self):
        self.assertNotRegex(source('v02-release.yml'), r'(?m)^\s*(?:-\s+)?continue-on-error\s*:')
        jobs = block(source('v02-release.yml'), 'jobs:')
        package = block(jobs, '  package:')
        tools = block(jobs, '  installed-tools:')
        for job, commands in ((package, (
                'default-tests cargo test --locked --no-fail-fast -p marmish-engine -p marmish-native -p marmish-vault',
                'default-clippy cargo clippy --locked -p marmish-native -p marmish-engine --all-targets -- -D warnings',
                'product-release cargo build --locked --release -p marmish-native --bins')),
                (tools, (
                'installed-tests cargo test --locked -p marmish-native --features installed-checks',
                'installed-clippy cargo clippy --locked -p marmish-native --features installed-checks --all-targets -- -D warnings',
                'installed-release cargo build --locked --release -p marmish-native --features installed-checks --bin marmish-installed-check --bin marmish-installed-fixture --bin marmish-release-tool'))):
            self.assertNotIn('continue-on-error', fields(job, 4))
            lines = []
            for step in steps(job):
                self.assertNotIn('continue-on-error', fields(step, 8))
                body = re.sub(r'[ \t]*\\\n\s*', ' ', run_text(step))
                lines.extend(line.strip() for line in body.splitlines())
            for command in commands:
                self.assertEqual(lines.count('sh scripts/v02-ci-timed.sh ' + command), 1)
        for command in ('sh scripts/v02-inspect-native.sh',
                        'python3 tools/marmish_native_licenses.py'):
            self.assertEqual(package.count(command), 1)
        self.assertEqual(tools.count('target/release/marmish-release-tool package-tooling'), 1)
        source_checks = [[run_text(step) for step in steps(job)
                          if step.startswith('name: Verify immutable source and native target\n')]
                         for job in (package, tools)]
        self.assertEqual(len(source_checks[0]), 1)
        self.assertEqual(source_checks[0], source_checks[1])
        self.assertIn('git merge-base --is-ancestor "$MARMISH_BUILD_COMMIT" refs/remotes/origin/master',
                      [line.strip() for line in source_checks[0][0].splitlines()])
        self.assertNotIn('native-assets', tools)
        self.assertNotIn('actions/download-artifact@', tools)
        for job in (package, tools):
            self.assertIn('codesign --verify --strict', job)
            self.assertIn('git diff --quiet HEAD --', job)
            self.assertIn('test "$(git rev-parse HEAD)" = "$MARMISH_BUILD_COMMIT"', job)


class MutationTests(unittest.TestCase):
    def check_source(self, name, before, after, passes=False):
        original = source(name)
        self.assertIn(before, original)
        with patch.dict(SOURCES, {name: original.replace(before, after, 1)}):
            result = unittest.TestResult()
            unittest.defaultTestLoader.loadTestsFromTestCase(ReleaseAutomation).run(result)
        self.assertEqual(result.testsRun,
                         unittest.defaultTestLoader.loadTestsFromTestCase(ReleaseAutomation).countTestCases())
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

    def test_split_and_cache_security_mutations(self):
        for before, after in [
            ("== 'build' && !inputs.caveated_release", "== 'build'"),
            ('needs: [package, installed-tools]', 'needs: package'),
            ("needs.installed-tools.result == 'success'", "needs.installed-tools.result != 'failure'"),
            ('max-parallel: 3', 'max-parallel: 9'),
            ('MARMISH_CANDIDATE_LAYOUT: split-v1', 'MARMISH_CANDIDATE_LAYOUT: split-v2'),
            ('cache-workspace-crates: false', 'cache-workspace-crates: true'),
            ('cache-on-failure: false', 'cache-on-failure: true'),
            ('cache-targets: true', 'cache-targets: false'),
            ('cache-targets: false', 'cache-targets: true'),
            ('Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0', 'Swatinem/rust-cache@v2'),
            ('MACOSX_DEPLOYMENT_TARGET=15.0', 'MACOSX_DEPLOYMENT_TARGET=14.0'),
            ('RUST_TEST_THREADS=1', 'RUST_TEST_THREADS=2'),
            ("github.ref == 'refs/heads/master' && !inputs.caveated_release", 'true'),
            ('SDKROOT ImageOS ImageVersion', 'SDKROOT ImageOS'),
            ('name: v02-installed-tools-', 'name: v02-candidate-'),
            ('cargo test --locked --no-fail-fast', 'cargo test --locked --no-fail-fast -- --ignored'),
            ('--features installed-checks --all-targets -- -D warnings', '--features installed-checks --all-targets'),
            ('-p marmish-engine -p marmish-native -p marmish-vault', '-p marmish-engine -p marmish-native -p marmish-vault -- --skip mdk'),
            ('installed-tests cargo test --locked -p marmish-native --features installed-checks', 'installed-tests cargo test --locked -p marmish-native --features installed-checks --lib'),
            ('  installed-tools:\n', '  installed-tools:\n    continue-on-error: true\n'),
            ('      - name: Build isolated installed-test tools\n', '      - name: Build isolated installed-test tools\n        continue-on-error: true\n'),
            ('git merge-base --is-ancestor', 'true # git merge-base --is-ancestor'),
        ]:
            with self.subTest(before=before):
                self.check_source('v02-release.yml', before, after)
        self.check_source('v02-native.yml', 'Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0',
                          'Swatinem/rust-cache@v2')
        self.check_source('v02-provider.yml', 'name: v0.2 macOS selected keychain provider',
                          '# marmish-v02-release-split-v2\nname: v0.2 macOS selected keychain provider')

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
