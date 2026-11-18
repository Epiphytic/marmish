import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const workflow = name => readFileSync(new URL(`../../.github/workflows/${name}`, import.meta.url), 'utf8');
const legacy = workflow('installer-validation.yml');
const coordinator = workflow('installed-coordinator.yml');
const nativeCore = workflow('interactive-core.yml');

// Read the reviewed literal workflow shape and fail on syntax drift.
function events(source) {
  const block = source.match(/^on:\n([\s\S]+?)\npermissions:/m);
  assert.ok(block);
  return [...block[1].matchAll(/^  (\w+):$/gm)].map(match => match[1]);
}

function pushPaths(source) {
  const list = source.match(/^  push:\n    branches: \[master\]\n    paths:\n((?:      - '[^']+'\n)+)/m);
  assert.ok(list);
  return [...list[1].matchAll(/      - '([^']+)'/g)].map(match => match[1]);
}

function selected(patterns, changed) {
  return changed.some(file => patterns.some(pattern => pattern.endsWith('/**')
    ? file.startsWith(pattern.slice(0, -2))
    : file === pattern));
}

test('legacy installer remains manually dispatchable without automatic routing', () => {
  assert.deepEqual(events(legacy), ['workflow_dispatch']);
});

test('coordinator routes only trusted master pushes that affect Node tools or their workflow', () => {
  assert.deepEqual(events(coordinator), ['push']);
  const paths = pushPaths(coordinator);
  assert.deepEqual(paths, ['tools/v02-actions/**', '.github/workflows/installed-coordinator.yml']);
  const cases = [
    [['tools/v02-actions/transport.mjs'], true],
    [['tools/v02-actions/package-lock.json'], true],
    [['tools/v02-actions/package.json'], true],
    [['tools/v02-actions/coordinator-policy.test.mjs'], true],
    [['tools/v02-actions/local.mjs', 'tools/test_marmish_install.py'], true],
    [['tools/test_marmish_install.py'], false],
    [['tools/bootstrap_ci_python.sh'], false],
    [['tools/v02-actions-other/fixture.py'], false],
    [['tests/fixtures/legacy/install-v0.1.sh'], false],
    [['tests/fixtures/managed-runtime-assets.json'], false],
    [['.github/workflows/installer-validation.yml'], false],
    [['.github/workflows/installed-coordinator.yml'], true],
    [['README.md'], false],
  ];
  for (const [changed, expected] of cases) {
    assert.equal(selected(paths, changed), expected, changed.join(', '));
  }
  assert.match(nativeCore, /^      - tools\/marmish_interactive_core_case\.sh$/m);
});

test('coordinator uses a trusted self-hosted runner and read-only pinned checkout', () => {
  assert.match(coordinator, /^permissions:\n  contents: read\njobs:/m);
  assert.match(coordinator, /^    if: github\.repository == 'Epiphytic\/marmish' && github\.ref == 'refs\/heads\/master'$/m);
  assert.match(coordinator, /^    runs-on:\n      group: epiphytic-ci\n      labels: \[self-hosted, Linux, X64, bel-ci\]$/m);
  assert.match(coordinator, /^    timeout-minutes: 10$/m);
  assert.match(coordinator, /^    defaults:\n      run:\n        working-directory: tools\/v02-actions$/m);
  assert.deepEqual([...coordinator.matchAll(/uses: (\S+)/g)].map(match => match[1]),
    ['actions/checkout@11d5960a326750d5838078e36cf38b85af677262']);
  assert.match(coordinator, /^          persist-credentials: false$/m);
  assert.deepEqual([...coordinator.matchAll(/^        run: (.+)$/gm)].map(match => match[1]),
    ['npm ci --ignore-scripts', 'npm test']);
  assert.doesNotMatch(coordinator, /pull_request|secrets\.|write-all|workflow_dispatch|cargo |gh /);
});
