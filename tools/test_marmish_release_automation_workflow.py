"""Static trust boundaries of the fixed native workflows; stdlib only."""
import pathlib
import re
import unittest
from unittest.mock import patch

from test_marmish_managed_workflow import step_blocks as steps

ROOT = pathlib.Path(__file__).resolve().parents[1]
SOURCES = {}  # Test-owned overrides; mutations never write workflow files.
ONTOLOGY_ACTION = './.github/actions/code-ontology'
QUALIFICATION_COHORTS = ('stream-matrix', 'rsync-matrix', 'copy-matrix', 'edge-stream', 'edge-rsync',
                         'edge-copy', 'copy-interrupt-put', 'copy-interrupt-get')
READ_ONLY = {'contents': 'read', 'actions': 'read'}
ARTIFACT_SCOPE = "${{ inputs.release_gate && format('-{0}', inputs.cohort) || '' }}"


def source(name):
    if name in SOURCES:
        return SOURCES[name]
    path = '.github/actions/code-ontology/action.yml' if name == 'code-ontology-action.yml' else '.github/workflows/' + name
    return (ROOT / path).read_text()


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
            options = fields(job, 4)
            if 'uses' not in options:
                block(job, '    steps:')

    def test_job_permissions_and_triggers(self):
        self.assertEqual(set(self.jobs), {'inspect', 'reproduce-bootstrap', 'qualification', 'benchmark', 'verify', 'publish'})
        self.assertEqual(block(self.text, 'permissions: {}'), 'permissions: {}\n')
        self.assertEqual(block(self.jobs['reproduce-bootstrap'], '    permissions: {}'), '    permissions: {}\n')
        for job in ('inspect', 'qualification', 'benchmark', 'verify', 'publish'):
            self.assertEqual(fields(block(self.jobs[job], '    permissions:'), 6),
                             {'contents': 'write' if job == 'publish' else 'read', 'actions': 'read'})
        events = block(self.text, 'on:')
        self.assertEqual(set(fields(events, 2)), {'workflow_run', 'workflow_dispatch'})
        self.assertEqual(fields(block(events, '  workflow_run:'), 4), {
            'workflows': '[v0.2 native release candidates]', 'types': '[completed]', 'branches': '[master]',
        })
        self.assertEqual(fields(self.jobs['inspect'], 4)['if'],
                         "github.ref == 'refs/heads/master' && (github.event_name == 'workflow_dispatch' || github.event.workflow_run.conclusion == 'success')")
        self.assertEqual(fields(block(self.text, 'concurrency:'), 2)['cancel-in-progress'], 'false')
        self.assertNotRegex(self.text, r'(?m)^\s*(?:-\s+)?continue-on-error\s*:')

    def test_publication_requires_success_from_every_gate(self):
        dependencies = {
            'reproduce-bootstrap': ['inspect'],
            'qualification': ['inspect'],
            'benchmark': ['inspect', 'qualification'],
            'verify': ['inspect', 'reproduce-bootstrap', 'qualification', 'benchmark'],
            'publish': ['inspect', 'reproduce-bootstrap', 'qualification', 'benchmark', 'verify'],
        }
        for job, needs in dependencies.items():
            with self.subTest(job=job):
                options = fields(self.jobs[job], 4)
                self.assertEqual(options['needs'], needs[0] if len(needs) == 1 else '[' + ', '.join(needs) + ']')
                condition = [f"needs.{name}.result == 'success'" for name in needs]
                if job in ('reproduce-bootstrap', 'qualification', 'benchmark'):
                    condition.append("needs.inspect.outputs.skip == 'false'")
                self.assertEqual(options['if'], ' && '.join(condition))

        uploads = fields(block(self.jobs['inspect'], '    outputs:'), 6)
        for key, output in (('source', 'candidate_source'), ('run_id', 'candidate_run_id'),
                            ('run_attempt', 'candidate_run_attempt')):
            self.assertEqual(uploads[output], '${{ steps.candidate-source.outputs.' + key + ' }}')

    def test_reusable_calls_bind_verified_attempt_and_serial_cohorts(self):
        inputs = {
            'candidate_run_id': '${{ needs.inspect.outputs.candidate_run_id }}',
            'candidate_run_attempt': '${{ needs.inspect.outputs.candidate_run_attempt }}',
            'source_commit': '${{ needs.inspect.outputs.candidate_source }}',
            'relay': "${{ vars.MARMISH_RELEASE_TEST_RELAY || 'wss://relay.us.whitenoise.chat/' }}",
            'release_gate': 'true',
        }
        for job, file, cohort in (('qualification', 'streaming-installed.yml', '${{ matrix.cohort }}'),
                                  ('benchmark', 'v02-installed.yml', 'benchmark')):
            with self.subTest(job=job):
                options = fields(self.jobs[job], 4)
                allowed = {'if', 'needs', 'permissions', 'uses', 'with'}
                if job == 'qualification':
                    allowed.add('strategy')
                self.assertEqual(set(options), allowed)
                self.assertEqual(options['uses'], './.github/workflows/' + file)
                self.assertEqual(fields(block(self.jobs[job], '    with:'), 6), {**inputs, 'cohort': cohort})
        strategy = block(self.jobs['qualification'], '    strategy:')
        self.assertEqual(fields(strategy, 6), {'fail-fast': 'true', 'max-parallel': '1', 'matrix': ''})
        self.assertEqual(fields(block(strategy, '      matrix:'), 8),
                         {'cohort': '[' + ', '.join(QUALIFICATION_COHORTS) + ']'})

    def check_installed_worker(self, file, gate_group, manual_group, matrix_name, rollup_name):
        text = source(file)
        self.assertEqual(fields(block(text, 'permissions:'), 2), READ_ONLY)
        self.assertNotRegex(text, r'(?m)^\s*(?:-\s+)?continue-on-error\s*:')
        events = block(text, 'on:')
        self.assertEqual(set(fields(events, 2)), {'workflow_dispatch', 'workflow_call'})
        manual = block(block(events, '  workflow_dispatch:'), '    inputs:')
        call = block(block(events, '  workflow_call:'), '    inputs:')
        names = {'cohort', 'candidate_run_id', 'candidate_run_attempt', 'source_commit', 'relay'}
        self.assertEqual(set(fields(manual, 6)), names)
        self.assertEqual(set(fields(call, 6)), names | {'release_gate'})
        for name in names | {'release_gate'}:
            self.assertEqual(fields(block(call, f'      {name}:'), 8),
                             {'type': 'boolean' if name == 'release_gate' else 'string', 'required': 'true'})
        group = "${{ inputs.release_gate && format('marmish-release-" + gate_group + "-{0}-{1}-{2}', github.run_id, github.run_attempt, inputs.cohort) || '" + manual_group + "' }}"
        self.assertEqual(fields(block(text, 'concurrency:'), 2), {'group': group, 'cancel-in-progress': 'false'})
        self.assertEqual(fields(block(text, 'env:'), 2), {
            'RELEASE_GATE': '${{ inputs.release_gate }}',
            'CANDIDATE_RUN_ID': '${{ inputs.candidate_run_id }}',
            'CANDIDATE_RUN_ATTEMPT': '${{ inputs.candidate_run_attempt }}',
            'SOURCE_COMMIT': '${{ inputs.source_commit }}',
            'TEST_RELAY': '${{ inputs.relay }}',
            'TEST_COHORT': '${{ inputs.cohort }}',
        })

        jobs = block(text, 'jobs:')
        context = 'v02-installed-context-${{ github.run_id }}-${{ github.run_attempt }}' + ARTIFACT_SCOPE
        self.assertEqual(text.count('name: ' + context), len(fields(jobs, 2)))
        for job in fields(jobs, 2):
            worker = block(jobs, f'  {job}:')
            self.assertNotIn('permissions', fields(worker, 4))
            self.assertEqual(worker.count('name: ' + context), 1)
            checkout, = (step for step in steps(worker) if step.startswith('uses: actions/checkout@'))
            self.assert_trusted_checkout(checkout, history=True)
            for step in steps(worker):
                if not step.startswith('uses: actions/download-artifact@'):
                    continue
                options = fields(block(step, '        with:'), 10)
                if 'run-id' in options:
                    self.assertEqual(options['run-id'], '${{ inputs.candidate_run_id }}')
                    self.assertTrue((options.get('name') or options.get('pattern')).endswith(
                        '-${{ inputs.candidate_run_id }}-${{ inputs.candidate_run_attempt }}'))

        resolve = block(jobs, '  resolve:')
        self.assertEqual(fields(resolve, 4)['if'], "github.ref == 'refs/heads/master'")
        self.assertEqual([line.strip() for line in resolve.splitlines() if 'workflow.mjs' in line], [
            'node tools/v02-actions/workflow.mjs validate-candidate',
            'run: node tools/v02-actions/workflow.mjs resolve',
        ])
        suffix = '-${{ inputs.candidate_run_id }}-${{ inputs.candidate_run_attempt }}'
        for job in (resolve, block(jobs, f'  {matrix_name}:'), block(jobs, f'  {rollup_name}:')):
            self.assertIn('name: v02-pinned-bootstrap' + suffix + '\n', job)
        matrix = block(jobs, f'  {matrix_name}:')
        self.assertEqual(fields(matrix, 4)['needs'], 'resolve')
        self.assertIn('name: v02-candidate-${{ matrix.target }}' + suffix + '\n', matrix)
        self.assertIn('run: node tools/v02-actions/workflow.mjs verify-context', matrix)
        self.assertEqual(re.findall(r'^          - runner: (.+)\n            role: (.+)\n            target: (.+)$', matrix, re.M), [
            ('ubuntu-24.04', 'linux-amd64', 'x86_64-unknown-linux-gnu'),
            ('ubuntu-24.04-arm', 'linux-arm64', 'aarch64-unknown-linux-gnu'),
            ('macos-15', 'macos-arm64', 'aarch64-apple-darwin'),
        ])
        self.assertEqual(len(block(matrix, '      matrix:').splitlines()), 11)
        return jobs, matrix, block(jobs, f'  {rollup_name}:')

    def test_streaming_call_uses_native_rollup_and_scoped_artifacts(self):
        jobs, matrix, rollup = self.check_installed_worker('streaming-installed.yml', 'streaming',
                                                            'marmish-streaming-installed', 'matrix', 'rollup')
        self.assertEqual(set(fields(jobs, 2)), {'resolve', 'matrix', 'rollup'})
        self.assertNotIn('if', fields(matrix, 4))
        self.assertEqual(fields(rollup, 4)['needs'], '[resolve, matrix]')
        self.assertEqual(fields(rollup, 4)['if'], "always() && needs.resolve.result == 'success'")
        self.assertEqual(fields(block(rollup, '    env:'), 6)['MATRIX_RESULT'], '${{ needs.matrix.result }}')
        self.assertIn('name: v02-installed-diagnostic-${{ github.run_id }}-${{ github.run_attempt }}' + ARTIFACT_SCOPE + '-${{ matrix.role }}', matrix)
        self.assertIn('name: v02-installed-rollup-${{ github.run_id }}-${{ github.run_attempt }}' + ARTIFACT_SCOPE, rollup)
        self.assertIn('run: node tools/v02-actions/workflow.mjs gather', rollup)
        self.assertIn('"$TOOLS_ROOT/marmish-installed-check" rollup --context "$CONTEXT"', rollup)
        self.assertIn('--records "$RECORDS_ROOT" --matrix-result "$MATRIX_RESULT" --output', rollup)

    def test_benchmark_call_uses_native_rollup_and_scoped_artifacts(self):
        jobs, local, rollup = self.check_installed_worker('v02-installed.yml', 'benchmark',
                                                           'marmish-v02-installed', 'local', 'local-rollup')
        self.assertEqual(set(fields(jobs, 2)), {'resolve', 'matrix', 'rollup', 'local', 'local-rollup'})
        self.assertEqual(fields(local, 4)['if'], "inputs.cohort != 'matrix'")
        self.assertEqual(fields(rollup, 4)['needs'], '[resolve, local]')
        self.assertEqual(fields(rollup, 4)['if'], "always() && needs.resolve.result == 'success' && inputs.cohort != 'matrix'")
        self.assertEqual(fields(block(rollup, '    env:'), 6)['LOCAL_RESULT'], '${{ needs.local.result }}')
        self.assertIn('run: node tools/v02-actions/local.mjs', local)
        self.assertIn('name: v02-installed-${{ inputs.cohort }}-${{ matrix.role }}-${{ github.run_id }}-${{ github.run_attempt }}', local)
        self.assertIn('name: v02-installed-${{ inputs.cohort }}-rollup-${{ github.run_id }}-${{ github.run_attempt }}', rollup)
        self.assertIn('"$TOOLS_ROOT/marmish-installed-check" cohort-rollup --cohort "$TEST_COHORT"', rollup)
        self.assertIn('--cohort-result "$LOCAL_RESULT" --output', rollup)

    def test_pinned_actions_no_write_job_dependency_or_candidate_execution(self):
        identities = {
            'inspect': ['uses: actions/checkout@', 'name: Install locked read-only archive tooling',
                        'name: Inspect completed candidate without executing artifacts',
                        'name: Read the verified candidate source commit', "name: Verify the candidate's committed ontology",
                        'uses: actions/upload-artifact@'],
            'reproduce-bootstrap': ['name: Fetch trusted publisher orchestration without credentials',
                'uses: actions/download-artifact@', 'name: Reproduce bootstrap with credential-free child environment',
                'uses: actions/upload-artifact@'],
            'verify': ['uses: actions/checkout@', 'uses: actions/download-artifact@', 'uses: actions/download-artifact@',
                'working-directory: tools/v02-actions', 'name: Independently revalidate candidate and reproduction',
                'uses: actions/upload-artifact@'],
            'publish': ['uses: actions/checkout@', 'uses: actions/download-artifact@', 'uses: actions/download-artifact@',
                'name: Publish immutable GitHub assets and fast-forward installer', 'uses: actions/upload-artifact@'],
        }
        for name, expected_steps in identities.items():
            job = self.jobs[name]
            selected = steps(job)
            self.assertEqual(len(selected), len(expected_steps))
            for step, expected in zip(selected, expected_steps):
                first = step.splitlines()[0]
                if expected.endswith('@'):
                    self.assertRegex(first, '^' + re.escape(expected) + r'[0-9a-f]{40}$')
                else:
                    self.assertEqual(first, expected)
                actions = re.findall(r'^(?:uses:|        uses:) (.*)$', step, re.M)
                for action in actions:
                    if action != ONTOLOGY_ACTION or name != 'inspect':
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
        for job, expected in {
            'verify': ['${{ needs.inspect.outputs.inputs_artifact_id }}',
                       '${{ needs.reproduce-bootstrap.outputs.render_artifact_id }}'],
            'publish': ['${{ needs.inspect.outputs.inputs_artifact_id }}',
                        '${{ needs.verify.outputs.inventory_artifact_id }}'],
        }.items():
            downloads = (step for step in steps(self.jobs[job]) if step.startswith('uses: actions/download-artifact@'))
            self.assertEqual([fields(block(step, '        with:'), 10)['artifact-ids'] for step in downloads], expected)

    def test_candidate_source_fallback_and_explicit_version_trigger(self):
        text = source('v02-release.yml')
        self.assertEqual(fields(block(block(text, 'on:'), '  push:'), 4),
                         {'branches': '[master]', 'paths': '[crates/marmish-native/Cargo.toml]'})
        self.assertNotRegex(text, r'\$\{\{ inputs.source_commit \}\}')
        package = block(block(text, 'jobs:'), '  package:')
        self.assertEqual(fields(package, 4)['needs'], 'release-eligibility')
        self.assertIn("== 'build'", fields(package, 4)['if'])
        self.assertIn('release-eligibility', fields(block(text, 'jobs:'), 2))

    def assert_ontology_action(self, step, commit, condition=None):
        options = fields('        ' + step, 8)
        uses = options.get('uses')
        self.assertEqual(uses, ONTOLOGY_ACTION)
        self.assertEqual(options.get('if'), condition)
        self.assertNotIn('continue-on-error', options)
        self.assertEqual(fields(block(step, '        with:'), 10), {'source-commit': commit})

    def assert_trusted_checkout(self, step, history=False):
        self.assertRegex(step.splitlines()[0], r'^uses: actions/checkout@[0-9a-f]{40}$')
        options = {'ref': '${{ github.sha }}', 'persist-credentials': 'false'}
        if history:
            options['fetch-depth'] = '0'
        self.assertEqual(fields(block(step, '        with:'), 10), options)

    def test_ontology_gates_candidate_and_publication_from_trusted_checkout(self):
        eligibility = block(block(source('v02-release.yml'), 'jobs:'), '  release-eligibility:')
        candidate_steps = steps(eligibility)
        self.assert_trusted_checkout(candidate_steps[0], history=True)
        self.assert_ontology_action(candidate_steps[1], '${{ inputs.source_commit || github.sha }}')
        self.assertIn('node tools/v02-actions/eligibility.mjs', run_text(candidate_steps[3]))
        self.assertNotIn('continue-on-error', fields(eligibility, 4))

        checkout, _, inspect, read_source, ontology, upload = steps(self.jobs['inspect'])
        self.assert_trusted_checkout(checkout, history=True)
        self.assertEqual(fields(inspect, 8)['id'], 'inspect')
        active = "steps.inspect.outputs.skip == 'false'"
        self.assertEqual(fields(read_source, 8)['if'], active)
        self.assertEqual(fields(read_source, 8)['id'], 'candidate-source')
        self.assertNotIn('continue-on-error', fields(read_source, 8))
        self.assertEqual(fields(block(read_source, '        env:'), 10), {
            'INSPECTION_ROOT': '${{ runner.temp }}/publish-inputs',
            'INSPECTION_SHA256': '${{ steps.inspect.outputs.inspection_sha256 }}',
        })
        self.assertIn("import {loadInspection} from './tools/v02-actions/release-render.mjs';", run_text(read_source))
        self.assertIn('const {inventory, bootstrap} = await loadInspection(process.env.INSPECTION_ROOT, process.env.INSPECTION_SHA256);',
                      run_text(read_source))
        self.assertIn("import {releaseQualificationSource} from './tools/v02-actions/workflow.mjs';", run_text(read_source))
        proof = "releaseQualificationSource({...process.env, SOURCE_COMMIT: inventory.source, RELEASE_GATE: 'true'}, {bootstrap});"
        emit = r'await appendFile(process.env.GITHUB_OUTPUT, `source=${inventory.source}\nrun_id=${inventory.candidate.run_id}\nrun_attempt=${inventory.candidate.run_attempt}\n`);'
        self.assertEqual(run_text(read_source).splitlines()[-3:], [proof, emit, 'JS'])
        self.assert_ontology_action(ontology, '${{ steps.candidate-source.outputs.source }}', active)
        self.assertEqual(fields(upload, 8)['if'], active)
        self.assertNotIn('continue-on-error', fields(self.jobs['inspect'], 4))

    def test_ontology_gates_legacy_build_before_any_matrix_and_binds_sha(self):
        text = source('release.yml')
        jobs = block(text, 'jobs:')
        gate = block(jobs, '  ontology:')
        self.assertEqual(fields(block(text, 'permissions:'), 2), {'contents': 'read'})
        self.assertEqual(fields(gate, 4), {
            'if': "github.ref == 'refs/heads/master'", 'runs-on': 'ubuntu-24.04',
            'timeout-minutes': '5', 'steps': '',
        })
        checkout, ontology = steps(gate)
        self.assert_trusted_checkout(checkout, history=True)
        self.assert_ontology_action(ontology, '${{ github.sha }}')
        self.assertEqual(fields(block(jobs, '  build:'), 4)['needs'], 'ontology')
        self.assertEqual(fields(block(jobs, '  publish:'), 4)['needs'], 'build')
        for job in ('build', 'publish'):
            self.assert_trusted_checkout(steps(block(jobs, '  ' + job + ':'))[0])

    def test_ontology_pr_has_read_only_disposable_runner_and_no_paths_filter(self):
        text = source('code-ontology.yml')
        self.assertEqual(fields(block(text, 'permissions:'), 2), {'contents': 'read'})
        event = block(text, 'on:')
        self.assertEqual(fields(event, 2), {'pull_request': '', 'merge_group': '', 'push': ''})
        self.assertEqual(fields(block(event, '  merge_group:'), 4), {'types': '[checks_requested]'})
        self.assertEqual(fields(block(event, '  push:'), 4), {'branches': '[master]', 'tags': "['v*']"})
        self.assertNotIn('paths', event)
        jobs = block(text, 'jobs:')
        self.assertEqual(fields(jobs, 2), {'freshness': ''})
        job = block(jobs, '  freshness:')
        self.assertEqual(fields(job, 4), {'name': 'Ontology freshness', 'runs-on': 'ubuntu-24.04',
                                          'timeout-minutes': '5', 'steps': ''})
        checkout, resolve, ontology, validation = steps(job)
        self.assert_trusted_checkout(checkout, history=True)
        self.assertEqual(fields(validation, 8)['run'], 'sh tools/code-ontology/test.sh')
        self.assertNotIn('continue-on-error', fields(validation, 8))
        self.assertIn('git rev-parse --verify "$GITHUB_SHA^{commit}"', run_text(resolve))
        self.assertIn('test "$source_commit" = "$(git rev-parse HEAD)"', run_text(resolve))
        self.assert_ontology_action(ontology, '${{ steps.source.outputs.source }}')
        self.assertNotIn('github.token', text)
        self.assertNotIn('${{', run_text(resolve))

    def test_ontology_composite_pins_tools_and_checks_exact_sha(self):
        text = source('code-ontology-action.yml')
        self.assertEqual(re.findall(r'(?m)^    - uses: (.+)$', text), [
            'astral-sh/setup-uv@1db35dd8eee11c5894872e0b383ae6c406cf738d # v10.4.0',
        ])
        self.assertIn('version: 0.11.16\n        require-checksums: true\n        enable-cache: false', text)
        self.assertIn('SOURCE_COMMIT: ${{ inputs.source-commit }}', text)
        command = block(text, '      run: |')
        self.assertNotIn('${{', command)
        self.assertIn('[[ "$SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]]', command)
        self.assertIn('test "$(git rev-parse --verify "$SOURCE_COMMIT^{commit}")" = "$SOURCE_COMMIT"', command)
        self.assertIn('uv run --locked --python 3.12 --project tools/code-ontology python tools/code-ontology/index.py check --ref "$SOURCE_COMMIT"', command)
        self.assertNotIn('continue-on-error', text)

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
            environment = block(job, '    env:')
            if name == 'installed-tools':
                environment = environment.replace("      CARGO_INCREMENTAL: '0'\n", '')
            self.assertEqual(fields(environment, 6)['MARMISH_BUILD_COMMIT'],
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
            if name != 'package':
                self.assertEqual(cache, [])
                continue
            self.assertEqual(len(cache), 1)
            self.assertIn('uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0', cache[0])
            options = fields(block(cache[0], '        with:'), 10)
            self.assertEqual(options['prefix-key'], 'marmish-v02-release-split-v2')
            for option in ('cache-all-crates', 'cache-workspace-crates', 'cache-bin', 'cache-on-failure'):
                self.assertEqual(options[option], 'false')
            self.assertEqual(options['cache-targets'], 'true')
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

    def test_installed_tools_disable_caching_and_incremental_compilation(self):
        text = source('v02-release.yml')
        jobs = block(text, 'jobs:')
        self.assertNotIn('CARGO_INCREMENTAL', block(text, 'env:'))
        self.assertNotIn('CARGO_INCREMENTAL', block(block(jobs, '  package:'), '    env:'))
        tools = block(jobs, '  installed-tools:')
        environment = block(tools, '    env:')
        # This single exact quoted value is intentional; keep generic scalar
        # parsing strict for every other workflow value.
        incremental = "      CARGO_INCREMENTAL: '0'\n"
        self.assertEqual(environment.count(incremental), 1)
        self.assertEqual(tools.count('CARGO_INCREMENTAL'), 2)
        self.assertIn('echo "marmish CI tooling: cache=disabled incremental=$CARGO_INCREMENTAL"',
                      run_text(next(step for step in steps(tools)
                                    if step.startswith('name: Build isolated installed-test tools\n'))))
        self.assertEqual(fields(environment.replace(incremental, ''), 6),
                         {'MARMISH_BUILD_COMMIT': '${{ inputs.source_commit || github.sha }}'})
        self.assertNotIn('Swatinem/rust-cache', tools)
        self.assertNotIn('steps.rust-cache', tools)
        self.assertNotIn('before cache pruning', tools)

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

    def test_qualification_security_mutations(self):
        for before, after in [
            ('needs: [inspect, reproduce-bootstrap, qualification, benchmark, verify]',
             'needs: [inspect, reproduce-bootstrap, benchmark, verify]'),
            ("needs.qualification.result == 'success'", "needs.qualification.result != 'failure'"),
            ("needs.benchmark.result == 'success'", "needs.benchmark.result != 'cancelled'"),
            ("if: needs.inspect.result == 'success' && needs.inspect.outputs.skip == 'false'",
             "if: needs.inspect.result == 'success'"),
            ('  qualification:\n', '  qualification:\n    continue-on-error: true\n'),
            ('    uses: ./.github/workflows/streaming-installed.yml',
             '    uses: ./.github/workflows/streaming-installed.yml\n    secrets: inherit'),
            ('      candidate_run_id: ${{ needs.inspect.outputs.candidate_run_id }}',
             '      candidate_run_id: ${{ inputs.candidate_run_id }}'),
            ('      candidate_run_attempt: ${{ needs.inspect.outputs.candidate_run_attempt }}',
             '      candidate_run_attempt: ${{ github.run_attempt }}'),
            ('      source_commit: ${{ needs.inspect.outputs.candidate_source }}',
             '      source_commit: ${{ github.sha }}'),
            ('      contents: read\n      actions: read\n    strategy:',
             '      contents: write\n      actions: read\n    strategy:'),
            ('        cohort: [' + ', '.join(QUALIFICATION_COHORTS) + ']',
             '        cohort: [' + ', '.join(QUALIFICATION_COHORTS[:-1]) + ']'),
            ('      max-parallel: 1', '      max-parallel: 3'),
            ('      release_gate: true', '      release_gate: false'),
            ('          releaseQualificationSource(', '          // releaseQualificationSource('),
        ]:
            with self.subTest(before=before):
                self.check_source('v02-publish.yml', before, after)
        for file, before, after in [
            ('streaming-installed.yml', '        type: boolean', '        type: string'),
            ('streaming-installed.yml', ARTIFACT_SCOPE, ''),
            ('streaming-installed.yml', 'marmish-release-streaming', 'marmish-native-publication'),
            ('streaming-installed.yml', '"$TOOLS_ROOT/marmish-installed-check" rollup', 'echo rollup'),
            ('v02-installed.yml', '          run-id: ${{ inputs.candidate_run_id }}', '          run-id: ${{ github.run_id }}'),
            ('v02-installed.yml', '        run: node tools/v02-actions/local.mjs', '        run: true'),
            ('v02-installed.yml', '        if: always()\n        uses: actions/upload-artifact@',
             '        if: always()\n        continue-on-error: true\n        uses: actions/upload-artifact@'),
        ]:
            with self.subTest(file=file, before=before):
                self.check_source(file, before, after)

    def test_ontology_security_mutations(self):
        for name, before, after in [
            ('code-ontology.yml', 'pull_request:', 'pull_request_target:'),
            ('code-ontology.yml', 'runs-on: ubuntu-24.04', 'runs-on: self-hosted'),
            ('code-ontology.yml', 'contents: read', 'contents: write'),
            ('code-ontology.yml', 'branches: [master]', 'branches: [master]\n    paths: [crates]'),
            ('code-ontology-action.yml', 'enable-cache: false', 'enable-cache: true'),
            ('code-ontology-action.yml', 'check --ref "$SOURCE_COMMIT"', 'check --ref HEAD'),
            ('v02-release.yml', 'ref: ${{ github.sha }}', 'ref: ${{ inputs.source_commit || github.sha }}'),
            ('v02-release.yml', '      - uses: ./.github/actions/code-ontology\n        with:\n',
             '      - uses: ./.github/actions/code-ontology\n        if: false\n        with:\n'),
            ('v02-publish.yml', 'source-commit: ${{ steps.candidate-source.outputs.source }}',
             'source-commit: ${{ github.sha }}'),
            ('v02-publish.yml', "        if: steps.inspect.outputs.skip == 'false'\n        uses: ./.github/actions/code-ontology",
             '        if: false\n        uses: ./.github/actions/code-ontology'),
            ('release.yml', 'needs: ontology', 'needs: []'),
        ]:
            with self.subTest(name=name, before=before):
                self.check_source(name, before, after)

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
            ('  CARGO_TERM_COLOR: never\n',
             '  CARGO_TERM_COLOR: never\n  CARGO_INCREMENTAL: 0\n'),
            ('      MARMISH_BUILD_COMMIT: ${{ inputs.source_commit || github.sha }}\n',
             '      MARMISH_BUILD_COMMIT: ${{ inputs.source_commit || github.sha }}\n'
             '      CARGO_INCREMENTAL: 0\n'),
            ("      CARGO_INCREMENTAL: '0'\n", ''),
            ("      CARGO_INCREMENTAL: '0'", "      CARGO_INCREMENTAL: '1'"),
            ('      - name: Build isolated installed-test tools\n',
             '      - uses: Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0\n'
             '      - name: Build isolated installed-test tools\n'),
            ('          echo \"marmish CI tooling: cache=disabled incremental=$CARGO_INCREMENTAL\"',
             '          echo \"marmish CI tooling: cache=disabled incremental=$CARGO_INCREMENTAL\"\n'
             '          echo \"${{ steps.rust-cache.outputs.cache-hit }}\"'),
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
