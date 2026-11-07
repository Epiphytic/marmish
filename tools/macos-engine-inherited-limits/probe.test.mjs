import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync, mkdtempSync, symlinkSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync, spawnSync} from 'node:child_process';
import {PROGRAM, limit, observation, parseChild, compare, main} from './probe.mjs';
const env = () => ({'INPUT_SHELL-SOFT': '256', 'INPUT_SHELL-HARD': 'unlimited',
  'INPUT_MACOS-VERSION': '15.7.2', ImageVersion: '20261101.1', ImageOS: 'macos15-arm64'});
const child = 'soft=65537\nhard=unlimited\n';

test('canonical decimal/infinity parser rejects malformed, imprecise and missing values', () => {
  for (const v of ['0', '65536', '18446744073709551615']) assert.equal(limit(v), v);
  assert.equal(limit('unlimited'), 'infinity');
  for (const v of [undefined, null, 1, true, '', '01', '-1', '+1', '1.0', ' 1',
    '1\n', 'infinity', 'x/private/path', '1'.repeat(21)]) {
    assert.throws(() => limit(v), /limit_invalid/);
  }
  assert.throws(() => observation('other', '1', '2'), /chain_invalid/);
  assert.throws(() => observation('shell', '3', '2'), /limit_order/);
  assert.throws(() => observation('shell', 'unlimited', '2'), /limit_order/);
});
test('child output is a closed, bounded exact two-line schema', () => {
  assert.equal(parseChild(child).soft, '65537');
  for (const v of [undefined, '', 'soft=1\n', child + 'extra\n', child + child,
    'hard=2\nsoft=1\n', 'soft=1\nsoft=2\n', 'soft=1\nhard=2\n\n', 'x'.repeat(4097)]) {
    assert.throws(() => parseChild(v), /child_output/);
  }
});
test('comparison preserves exact boundaries and child inheritance/timeout options', () => {
  const result = compare(env(), 'v24.14.0', (file, args, options) => {
    assert.equal(file, '/bin/sh'); assert.deepEqual(args, ['-c', PROGRAM]);
    assert.deepEqual(options, {env: env(), stdio: ['ignore', 'pipe', 'pipe'],
      encoding: 'utf8', timeout: 5000, maxBuffer: 4096, killSignal: 'SIGKILL'});
    return child;
  });
  assert.equal(result.qualification, false);
  assert.equal(result.shell_exceeds_ceiling, false);
  assert.equal(result.node24_exceeds_ceiling, true);
  assert.equal(result.soft_changed, true);
  const same = compare({...env(), 'INPUT_SHELL-SOFT': '65536'}, 'v24.14.0',
    () => 'soft=65536\nhard=65536\n');
  assert.equal(same.soft_changed, false); assert.equal(same.node24_exceeds_ceiling, false);
  const infinite = compare(env(), 'v24.14.0', () => 'soft=unlimited\nhard=unlimited\n');
  assert.equal(infinite.node24_exceeds_ceiling, true);
});
test('failure never emits a successful summary or arbitrary child diagnostics', () => {
  for (const execute of [() => { throw Object.assign(new Error('/private/secret'), {signal: 'SIGKILL'}); },
    () => { throw Object.assign(new Error('/private/secret'), {status: 1}); },
    () => { throw Object.assign(new Error('/private/secret'), {code: 'ETIMEDOUT'}); },
    () => { throw Object.assign(new Error('/private/secret'), {code: 'ENOBUFS'}); },
    () => { throw Object.assign(new Error('/private/secret'), {code: 'ENOENT'}); }, () => 'bad']) {
    const out = [], err = [];
    assert.equal(main(env(), 'v24.14.0', execute, x => out.push(x), x => err.push(x)), 1);
    assert.deepEqual(out, []); assert.equal(err.length, 1);
    assert.match(err[0], /^::error::macos-inherited-nofile:(child_failed|child_output)$/);
  }
  for (const changes of [{'INPUT_SHELL-SOFT': ''}, {ImageOS: '/private/secret'},
    {ImageVersion: undefined}, {'INPUT_MACOS-VERSION': '15\nsecret'}]) {
    assert.throws(() => compare({...env(), ...changes}, 'v24.14.0', () => child));
  }
  assert.throws(() => compare(env(), 'v22.0.0', () => child), /provenance_invalid/);
});
test('the fixed program actually reads rather than adjusts limits', () => {
  const output = execFileSync('/bin/sh', ['-c', PROGRAM],
    {encoding: 'utf8', timeout: 5000, maxBuffer: 4096});
  assert.equal(parseChild(output).chain, 'node24');
});
test('workflow/action policy is manual, bounded, dependency-free and shares exact shell program', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/macos-engine-inherited-limits.yml', import.meta.url), 'utf8');
  const action = readFileSync(new URL('./action.yml', import.meta.url), 'utf8');
  assert.match(workflow, /on:\n  workflow_dispatch:\npermissions:\n  contents: read/);
  assert.match(workflow, /runs-on: macos-15\n    timeout-minutes: 5/);
  assert.equal((workflow.match(/^  observe:/gm) ?? []).length, 1);
  assert.match(workflow, /actions\/checkout@11d5960a326750d5838078e36cf38b85af677262/);
  assert.match(workflow, /persist-credentials: false/);
  assert.match(action, /using: node24\n  main: run.mjs/);
  assert.match(workflow, /uses: \.\/tools\/macos-engine-inherited-limits/);
  const program = workflow.match(/\/bin\/sh -c '([^']+)' >> "\$GITHUB_OUTPUT"/);
  assert.ok(program); assert.equal(program[1], PROGRAM);
  assert.equal((workflow.match(/^      - /gm) ?? []).length, 3);
  for (const input of ['soft', 'hard', 'macos_version']) {
    assert.ok(workflow.includes('${{ steps.baseline.outputs.' + input + ' }}'));
  }
  assert.doesNotMatch(workflow, /\b(cargo|curl|npm|sudo|marmishd|schedule|push|pull_request)\b|secrets\.|contents: write/);
  assert.doesNotMatch(workflow.split('run: |')[1].split('      - name:')[0], /\$\{\{/);
  assert.doesNotMatch(PROGRAM, /ulimit -[SH]n\s+[0-9]/);
});

test('actual symlink entry executes and fails closed without inputs', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'marmish-nofile-entry-'));
  try {
    const entry = path.join(dir, 'entry.mjs');
    symlinkSync(fileURLToPath(new URL('./run.mjs', import.meta.url)), entry);
    const result = spawnSync(process.execPath, [entry], {
      env: {}, encoding: 'utf8', timeout: 5000, maxBuffer: 4096,
    });
    assert.equal(result.error, undefined);
    assert.equal(result.status, 1);
    assert.equal(result.signal, null);
    assert.equal(result.stdout, '');
    assert.equal(result.stderr, '::error::macos-inherited-nofile:limit_invalid\n');
  } finally {
    rmSync(dir, {recursive: true, force: true});
  }
});
