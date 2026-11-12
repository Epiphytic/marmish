import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const workflow = name => readFileSync(new URL(`../../.github/workflows/${name}`, import.meta.url), 'utf8');
const legacy = workflow('installer-validation.yml');
const coordinator = workflow('installed-coordinator.yml');
// Read only this deliberately simple, literal path-list shape; fail on syntax drift.
function paths(source, event) {
  const section = source.match(new RegExp(`^  ${event}:\\n((?:    .*\\n|      .*\\n)+)`, 'm'));
  assert.ok(section, event);
  const list = section[1].match(/    paths:\n((?:      - '[^']+'\n)+)/);
  assert.ok(list, event);
  return [...list[1].matchAll(/      - '([^']+)'/g)].map(match => match[1]);
}
// Evaluate the reviewed literal glob subset in order (last matching pattern wins).
function selected(patterns, changed) {
  return changed.some(file => {
    let included = false;
    for (const pattern of patterns) {
      const positive = !pattern.startsWith('!');
      const literal = positive ? pattern : pattern.slice(1);
      const matches = literal.endsWith('/**')
        ? file.startsWith(literal.slice(0, -2)) : file === literal;
      if (matches) included = positive;
    }
    return included;
  });
}

test('legacy excludes only coordinator subtree and preserves identical push/PR coverage', () => {
  const expected = ['tests/fixtures/legacy/**', 'tools/**', '!tools/v02-actions/**',
    'tests/fixtures/managed-runtime-assets.json', '.github/workflows/installer-validation.yml'];
  assert.deepEqual(paths(legacy, 'push'), expected);
  assert.deepEqual(paths(legacy, 'pull_request'), expected);
  assert.match(legacy, /^    branches: \[master\]$/m);
  assert.match(legacy, /^  workflow_dispatch:$/m);
});

test('coordinator and legacy route Node, lockfile, mixed, fixture and workflow changes', () => {
  const expected = ['tools/v02-actions/**', '.github/workflows/installed-coordinator.yml'];
  assert.deepEqual(paths(coordinator, 'push'), expected);
  assert.deepEqual(paths(coordinator, 'pull_request'), expected);
  assert.match(coordinator, /^    branches: \[master\]$/m);
  const cases = [
    [['tools/v02-actions/transport.mjs'], false, true],
    [['tools/v02-actions/package-lock.json'], false, true],
    [['tools/v02-actions/package.json'], false, true],
    [['tools/v02-actions/coordinator-policy.test.mjs'], false, true],
    [['tools/v02-actions/local.mjs', 'tools/test_marmish_install.py'], true, true],
    [['tools/test_marmish_install.py'], true, false],
    [['tools/bootstrap_ci_python.sh'], true, false],
    [['tools/v02-actions-other/fixture.py'], true, false],
    [['tests/fixtures/legacy/install-v0.1.sh'], true, false],
    [['tests/fixtures/managed-runtime-assets.json'], true, false],
    [['.github/workflows/installer-validation.yml'], true, false],
    [['.github/workflows/installed-coordinator.yml'], false, true],
    [['README.md'], false, false],
  ];
  for (const event of ['push', 'pull_request']) {
    for (const [changed, oldExpected, newExpected] of cases) {
      assert.equal(selected(paths(legacy, event), changed), oldExpected, `${event}: legacy ${changed}`);
      assert.equal(selected(paths(coordinator, event), changed), newExpected, `${event}: coordinator ${changed}`);
    }
  }
});

test('coordinator uses read-only pinned checkout and locked dependencies without installation scripts', () => {
  assert.match(coordinator, /^permissions:\n  contents: read\njobs:/m);
  assert.match(coordinator, /^    runs-on: ubuntu-24\.04$/m);
  assert.match(coordinator, /^    timeout-minutes: 10$/m);
  assert.match(coordinator, /^    defaults:\n      run:\n        working-directory: tools\/v02-actions$/m);
  assert.deepEqual([...coordinator.matchAll(/uses: (\S+)/g)].map(match => match[1]),
    ['actions/checkout@11d5960a326750d5838078e36cf38b85af677262']);
  assert.match(coordinator, /^          persist-credentials: false$/m);
  assert.deepEqual([...coordinator.matchAll(/^        run: (.+)$/gm)].map(match => match[1]),
    ['npm ci --ignore-scripts', 'npm test']);
  assert.doesNotMatch(coordinator, /pull_request_target|secrets\.|write-all|workflow_dispatch|cargo |gh /);
});
