import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';

const workflow = readFileSync(new URL('../../.github/workflows/macos-native-regression.yml', import.meta.url), 'utf8');
const combined = 'cargo test --locked --no-fail-fast -p marmish-engine -p marmish-stream -p marmish-native -p marmish-vault';

test('manual regression is bounded, read-only and nonroot on Mac ARM64', () => {
  assert.match(workflow, /on:\n  workflow_dispatch:\n    inputs:\n      source_commit:\n/);
  assert.match(workflow, /required: true\n        type: string/);
  assert.doesNotMatch(workflow, /\n  (push|pull_request|schedule|workflow_run):/);
  assert.match(workflow, /permissions:\n  contents: read\nconcurrency:/);
  assert.match(workflow, /if: github.ref == 'refs\/heads\/master'/);
  assert.match(workflow, /runs-on: macos-15\n    timeout-minutes: 45/);
  assert.match(workflow, /test "\$\(uname -s\):\$\(uname -m\)" = Darwin:arm64/);
  assert.match(workflow, /RUST_TEST_THREADS: '1'/);
  assert.match(workflow, /MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS: '1'/);
  assert.equal((workflow.match(/test "\$\(id -u\)" -ne 0/g) ?? []).length, 5);
});

test('source is exact workflow and checkout commit with pinned toolchain', () => {
  assert.match(workflow, /ref: \$\{\{ inputs.source_commit \}\}/);
  assert.match(workflow, /persist-credentials: false\n          fetch-depth: 0/);
  assert.ok(workflow.includes('[[ "$MARMISH_BUILD_COMMIT" =~ ^[0-9a-f]{40}$ ]]'));
  assert.ok(workflow.includes('test "$MARMISH_BUILD_COMMIT" = "$GITHUB_SHA"'));
  assert.ok(workflow.includes('test "$(git rev-parse HEAD)" = "$MARMISH_BUILD_COMMIT"'));
  assert.match(workflow, /actions\/checkout@11d5960a326750d5838078e36cf38b85af677262/);
  assert.match(workflow, /dtolnay\/rust-toolchain@59a558e8c538bf4a23bbffa19e5a278b703f2920/);
  assert.match(workflow, /toolchain: 1\.97\.1/);
});

test('combined compilation and structural worker proof precede original combined tests', () => {
  const compile = workflow.indexOf(`${combined} --no-run\n`);
  const metadata = workflow.indexOf('cargo metadata --locked --no-deps --format-version 1');
  const run = workflow.indexOf(`${combined}\n`);
  assert.ok(compile >= 0 && metadata > compile && run > metadata);
  assert.ok(workflow.includes('required_features: ["fault-injection"]'));
  assert.ok(workflow.includes('for binary in marmish-engine-worker marmish-worker; do'));
  for (const check of ['test -f "target/debug/$binary"', 'test -x "target/debug/$binary"', 'shasum -a 256 "target/debug/$binary"']) assert.ok(workflow.includes(check));
});

test('actual workflow jq accepts both declared engine targets and rejects ambiguous workers', () => {
  const filter = workflow.match(/jq -e '([^']+)' "\$metadata"/)[1];
  // Cargo metadata includes feature-gated targets even without enabling the feature.
  // Match the declared Cargo.toml bin contract, including its hyphenated metadata field.
  const worker = {name: 'marmish-engine-worker', kind: ['bin'], crate_types: ['bin']};
  const frontend = {name: 'marmish-test-frontend', kind: ['bin'], crate_types: ['bin'], 'required-features': ['fault-injection']};
  const library = {name: 'marmish_engine', kind: ['lib'], crate_types: ['lib']};
  const metadata = targets => ({packages: [{name: 'marmish-engine', targets}, {name: 'marmish-native', targets: [{name: 'marmish-worker', kind: ['bin']}]}]});
  for (const [targets, expected] of [
    [[library, worker, frontend], 0],
    [[frontend, worker, library], 0],
    [[library, {...worker, name: 'marmish-worker'}, frontend], 1],
    [[library, frontend], 1],
    [[library, worker, {...worker}, frontend], 1],
    [[library, worker, frontend, {name: 'marmish-worker', kind: ['bin']}], 1],
    [[library, worker, {...frontend, 'required-features': []}], 1],
  ]) {
    const result = spawnSync('jq', ['-e', filter], {input: JSON.stringify(metadata(targets)), encoding: 'utf8', timeout: 5000});
    assert.ifError(result.error);
    assert.equal(result.status, expected, result.stderr);
  }
});

test('focused collection survives suite failure, runs twenty times and rejects zero tests', () => {
  assert.match(workflow, /if: always\(\) && steps.compile.outcome == 'success'/);
  assert.match(workflow, /for iteration in \{1\.\.20\}; do/);
  for (const name of [
    'final_handle_drop_releases_full_unacked_output_after_exit',
    'final_handle_drop_releases_full_unacked_output_race',
    'final_handle_drop_cancels_live_full_unacked_output',
    'final_service_handle_drop_finishes_owned_cleanup',
  ]) assert.ok(workflow.includes(`stream_runner::tests::${name}\n`));
  assert.ok(workflow.includes('cargo test --locked -p marmish-native --lib "$test_name" -- --exact --nocapture 2>&1 | tee "$log"'));
  assert.ok(workflow.includes("grep -Eq '^test result: ok\\. 1 passed; 0 failed; 0 ignored;' \"$log\""));
  assert.equal((workflow.match(/set -euo pipefail/g) ?? []).length, 5);
  assert.doesNotMatch(workflow, /continue-on-error|\|\| true/);
});

test('no live traffic, provider, installation or publication authority is added', () => {
  assert.doesNotMatch(workflow, /secrets\.|github\.token|GH_TOKEN|GITHUB_TOKEN|: write|npm |node |curl |wget |wss:|relay|install\.sh|cargo publish|gh release|upload-artifact|cargo build --release/);
  assert.equal((workflow.match(/^      - uses:/gm) ?? []).length, 2);
  assert.doesNotMatch(workflow, /streaming-candidate\.yml|streaming-installed\.yml|v02-release\.yml/);
});
