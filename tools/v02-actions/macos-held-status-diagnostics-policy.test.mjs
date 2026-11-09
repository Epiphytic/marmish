import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync, readFileSync, writeFileSync, chmodSync, mkdirSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawnSync} from 'node:child_process';

const workflow = readFileSync(new URL('../../.github/workflows/macos-held-status-diagnostics.yml', import.meta.url), 'utf8');
const race = 'stream_runner::tests::final_handle_drop_releases_full_unacked_output_race';
const fullCommand = 'cargo test --locked -p marmish-native --features installed-checks';
const blocks = [...workflow.matchAll(/        run: \|\n((?:          .*\n|\n)+)/g)]
  .map(match => match[1].replace(/^          /gm, ''));
const [guard, metadata, engine, full, repeats] = blocks;

test('manual diagnostic preserves exact immutable nonroot source guards and fixed budget', () => {
  assert.equal(blocks.length, 5);
  assert.match(workflow, /^name: .*not qualification/m);
  assert.match(workflow, /on:\n  workflow_dispatch:\n    inputs:\n      source_commit:/);
  assert.match(workflow, /required: true\n        type: string/);
  assert.match(workflow, /permissions:\n  contents: read\nconcurrency:\n  group: marmish-macos-held-status-diagnostics\n  cancel-in-progress: false/);
  assert.match(workflow, /if: github.ref == 'refs\/heads\/master'/);
  assert.match(workflow, /runs-on: macos-15\n    timeout-minutes: 45/);
  for (const line of ["MARMISH_NATIVE_TOOLCHAIN: '1'", "MACOSX_DEPLOYMENT_TARGET: '15.0'", "RUST_TEST_THREADS: '1'", "MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS: '1'", 'MARMISH_BUILD_COMMIT: ${{ inputs.source_commit }}']) {
    assert.ok(workflow.includes(line));
  }
  for (const line of [
    '[[ "$MARMISH_BUILD_COMMIT" =~ ^[0-9a-f]{40}$ ]]',
    'test "$MARMISH_BUILD_COMMIT" = "$GITHUB_SHA"',
    'test "$(git rev-parse HEAD)" = "$MARMISH_BUILD_COMMIT"',
    'test "$GITHUB_REPOSITORY" = Epiphytic/marmish',
    'test "$GITHUB_REF" = refs/heads/master',
    'test "$(uname -s):$(uname -m)" = Darwin:arm64',
    'test "$(id -u)" -ne 0',
    'git merge-base --is-ancestor "$MARMISH_BUILD_COMMIT" refs/remotes/origin/master',
    'git diff --quiet HEAD --',
  ]) assert.ok(guard.includes(line));
  assert.match(workflow, /actions\/checkout@11d5960a326750d5838078e36cf38b85af677262/);
  assert.match(workflow, /persist-credentials: false\n          fetch-depth: 0/);
  assert.match(workflow, /dtolnay\/rust-toolchain@59a558e8c538bf4a23bbffa19e5a278b703f2920/);
  assert.match(workflow, /toolchain: 1\.97\.1\n          components: clippy/);
  assert.doesNotMatch(workflow, /always\(\)|continue-on-error|--no-fail-fast|--ignored|--include-ignored|--skip|\|\| true|secrets\.|upload-artifact|sudo|workflow_run:|pull_request:|schedule:|\n  push:/);
  assert.equal((workflow.match(/^        if:/gm) ?? []).length, 0);
});

test('metadata guards identify actual manifest workers without native builds', () => {
  assert.ok(metadata.includes('cargo metadata --locked --no-deps --format-version 1 | tee "$metadata"'));
  assert.ok(metadata.includes('{name: "marmish-engine-worker", required_features: []}, {name: "marmish-test-frontend", required_features: ["fault-injection"]}'));
  assert.ok(metadata.includes('{name: "marmish", required_features: []}, {name: "marmish-installed-check", required_features: ["installed-checks"]}, {name: "marmish-installed-fixture", required_features: ["installed-checks"]}, {name: "marmish-release-tool", required_features: []}, {name: "marmish-worker", required_features: []}, {name: "marmishd", required_features: []}'));
  assert.equal((metadata.match(/if length == 1/g) ?? []).length, 2);
  assert.doesNotMatch(workflow, /cargo build|--no-run|shasum|authority_withdrawal|pending_cleanup_budget_/);
});

test('engine checks cover exact pure test family and formatter in isolated profiles', () => {
  assert.ok(engine.includes('export CARGO_TARGET_DIR="$RUNNER_TEMP/held-status-engine-target"\ntest ! -e "$CARGO_TARGET_DIR"'));
  assert.equal((engine.match(/for profile in debug release; do/g) ?? []).length, 2);
  assert.doesNotMatch(engine, /--locked \"\$\{flags\[@\]\}\"/);
  assert.ok(engine.includes('cargo test --locked ${flags[@]+"${flags[@]}"} -p marmish-engine --lib platform::macos::tests::held_status_diagnostics_ -- --nocapture 2>&1 | tee "$log"'));
  assert.ok(engine.includes("grep -Eq '^test result: ok\\. 2 passed; 0 failed; 0 ignored;'"));
  assert.ok(engine.includes('for test_name in acceptance failure_detail; do'));
  assert.ok(engine.includes('cargo test --locked -p marmish-engine --lib process::diagnostic::tests::held_wait_records_are_closed_and_failure_only -- --exact --nocapture 2>&1 | tee "$log"'));
  assert.ok(engine.includes("grep -Eq '^test result: ok\\. 1 passed; 0 failed; 0 ignored;'"));
  assert.equal((engine.match(/grep -c '\^test result:'/g) ?? []).length, 2);
  assert.ok(engine.includes('cargo clippy --locked ${flags[@]+"${flags[@]}"} -p marmish-engine --all-targets -- -D warnings 2>&1 | tee "$log"'));
  assert.doesNotMatch(engine, /marmish-native|installed-checks/);
});

test('full suite remains exact once before one hundred feature-matched repetitions', () => {
  assert.ok(full.includes('export CARGO_TARGET_DIR="$RUNNER_TEMP/installed-tools-target"\ntest ! -e "$CARGO_TARGET_DIR"'));
  assert.ok(full.includes(`${fullCommand} 2>&1 | tee "$log"`));
  assert.equal((full.match(/cargo /g) ?? []).length, 1);
  assert.doesNotMatch(full, /--lib|--exact|--nocapture/);
  assert.ok(full.includes("grep -Ec '^test result: ok\\. 907 passed; 0 failed; 3 ignored;'"));
  assert.ok(full.includes(`grep -c '^test ${race} \\.\\.\\.'`));
  assert.ok(repeats.includes('export CARGO_TARGET_DIR="$RUNNER_TEMP/installed-tools-target"\ntest -d "$CARGO_TARGET_DIR"'));
  assert.ok(repeats.includes(`test_name=${race}\n`));
  assert.ok(repeats.includes('for iteration in {1..100}; do'));
  assert.ok(repeats.includes('echo "held-status installed-checks iteration=$iteration test=$test_name"'));
  assert.ok(repeats.includes(`${fullCommand} --lib "$test_name" -- --exact --nocapture 2>&1 | tee "$log"`));
  assert.ok(repeats.includes("grep -Eq '^test result: ok\\. 1 passed; 0 failed; 0 ignored;'"));
  assert.ok(repeats.includes('test "$(grep -c \'^test result:\' "$log")" -eq 1'));
  assert.ok(repeats.includes('grep -c "^test $test_name \\.\\.\\."'));
  for (const block of blocks) {
    assert.ok(block.startsWith('set -euo pipefail\n'));
    const syntax = spawnSync('bash', ['-n'], {input: block, encoding: 'utf8', timeout: 2000});
    assert.equal(syntax.status, 0, syntax.stderr);
  }
});

// Only these inert local executables run: no Cargo, native process or workflow.
function stubbedSequence(mode) {
  const directory = mkdtempSync(join(tmpdir(), 'marmish-held-policy-'));
  try {
    const bin = join(directory, 'bin');
    mkdirSync(bin);
    const cargo = join(bin, 'cargo');
    writeFileSync(cargo, `#!/bin/bash
set -euo pipefail
printf '%s\\n' "$*" >> "$RUNNER_TEMP/invocations"
mkdir -p "$CARGO_TARGET_DIR"
if [[ " $* " != *' --lib '* ]]; then
  [[ "$STUB_MODE" != full_failure ]] || exit 17
  printf 'test ${race} ... diagnostic-stderr\\nok\\n'
  if [[ "$STUB_MODE" = bad_full_count ]]; then
    echo 'test result: ok. 906 passed; 0 failed; 3 ignored;'
  else
    echo 'test result: ok. 907 passed; 0 failed; 3 ignored;'
    echo 'test result: ok. 0 passed; 0 failed; 0 ignored;'
  fi
else
  count=$(wc -l < "$RUNNER_TEMP/invocations")
  [[ "$STUB_MODE" != repeat_failure || "$count" -ne 2 ]] || exit 19
  printf 'test ${race} ... diagnostic-stderr\\nok\\n'
  echo 'test result: ok. 1 passed; 0 failed; 0 ignored;'
fi
`);
    chmodSync(cargo, 0o700);
    const git = join(bin, 'git');
    writeFileSync(git, '#!/bin/sh\nexit 0\n');
    chmodSync(git, 0o700);
    const first = spawnSync('bash', ['-c', full], {encoding: 'utf8', timeout: 5000,
      env: {...process.env, PATH: `${bin}:${process.env.PATH}`, RUNNER_TEMP: directory, STUB_MODE: mode}});
    let second;
    // Actions' default success() admission: no repeat step after a full failure.
    if (first.status === 0) second = spawnSync('bash', ['-c', repeats], {encoding: 'utf8', timeout: 15000,
      env: {...process.env, PATH: `${bin}:${process.env.PATH}`, RUNNER_TEMP: directory, STUB_MODE: mode}});
    const invocations = readFileSync(join(directory, 'invocations'), 'utf8').trim().split('\n');
    return {first, second, invocations};
  } finally {
    rmSync(directory, {recursive: true, force: true});
  }
}

test('inert full-command failure runs no repetitions', () => {
  const result = stubbedSequence('full_failure');
  assert.equal(result.first.status, 17);
  assert.equal(result.second, undefined);
  assert.equal(result.invocations.length, 1);
});

test('inert full-summary refusal runs no repetitions', () => {
  const result = stubbedSequence('bad_full_count');
  assert.notEqual(result.first.status, 0);
  assert.equal(result.second, undefined);
  assert.equal(result.invocations.length, 1);
});

test('inert first failing repetition stops later invocations', () => {
  const result = stubbedSequence('repeat_failure');
  assert.equal(result.first.status, 0, result.first.stderr);
  assert.equal(result.second.status, 19);
  assert.equal(result.invocations.length, 2);
  assert.equal((result.second.stdout.match(/iteration=/g) ?? []).length, 1);
});

test('inert success runs one full suite and exactly one hundred repetitions', () => {
  const result = stubbedSequence('success');
  assert.equal(result.first.status, 0, result.first.stderr);
  assert.equal(result.second.status, 0, result.second.stderr);
  assert.equal(result.invocations[0], 'test --locked -p marmish-native --features installed-checks');
  assert.equal(result.invocations.length, 101);
  assert.ok(result.invocations.slice(1).every(args => args === `test --locked -p marmish-native --features installed-checks --lib ${race} -- --exact --nocapture`));
  const announcements = [...result.second.stdout.matchAll(/iteration=(\d+) test=/g)].map(match => Number(match[1]));
  assert.deepEqual(announcements, Array.from({length: 100}, (_, index) => index + 1));
});

test('inert engine block uses empty debug flags and exact release arguments', () => {
  const directory = mkdtempSync(join(tmpdir(), 'marmish-held-engine-policy-'));
  try {
    const bin = join(directory, 'bin');
    mkdirSync(bin);
    const cargo = join(bin, 'cargo');
    writeFileSync(cargo, `#!/bin/bash
set -euo pipefail
printf '%s\\n' "$*" >> "$RUNNER_TEMP/invocations"
mkdir -p "$CARGO_TARGET_DIR"
if [[ "$1" = clippy ]]; then exit 0; fi
if [[ "$*" = *held_status_diagnostics_* ]]; then
  echo 'test platform::macos::tests::held_status_diagnostics_acceptance ... ok'
  echo 'test platform::macos::tests::held_status_diagnostics_failure_detail ... ok'
  echo 'test result: ok. 2 passed; 0 failed; 0 ignored;'
else
  echo 'test process::diagnostic::tests::held_wait_records_are_closed_and_failure_only ... ok'
  echo 'test result: ok. 1 passed; 0 failed; 0 ignored;'
fi
`);
    chmodSync(cargo, 0o700);
    const git = join(bin, 'git');
    writeFileSync(git, '#!/bin/sh\nexit 0\n');
    chmodSync(git, 0o700);
    const result = spawnSync('bash', ['-c', engine], {encoding: 'utf8', timeout: 5000,
      env: {...process.env, PATH: `${bin}:${process.env.PATH}`, RUNNER_TEMP: directory}});
    assert.equal(result.status, 0, result.stderr);
    const invocations = readFileSync(join(directory, 'invocations'), 'utf8').trim().split('\n');
    assert.deepEqual(invocations, [
      'test --locked -p marmish-engine --lib platform::macos::tests::held_status_diagnostics_ -- --nocapture',
      'test --locked --release -p marmish-engine --lib platform::macos::tests::held_status_diagnostics_ -- --nocapture',
      'test --locked -p marmish-engine --lib process::diagnostic::tests::held_wait_records_are_closed_and_failure_only -- --exact --nocapture',
      'clippy --locked -p marmish-engine --all-targets -- -D warnings',
      'clippy --locked --release -p marmish-engine --all-targets -- -D warnings',
    ]);
  } finally {
    rmSync(directory, {recursive: true, force: true});
  }
});
