import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';

const workflow = readFileSync(new URL('../../.github/workflows/macos-native-regression.yml', import.meta.url), 'utf8');
const combined = 'cargo test --locked --no-fail-fast -p marmish-engine -p marmish-stream -p marmish-native -p marmish-vault';
const followupCondition = "        if: always() && !cancelled() && steps.compile.outcome == 'success' && (!inputs.authority_diagnostics || steps.authority.outcome == 'success')\n";

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
  assert.equal((workflow.match(/test "\$\(id -u\)" -ne 0/g) ?? []).length, 6);
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

test('optional authority observation checks the SDK before exactly matched installed-feature tests', () => {
  assert.match(workflow, /authority_diagnostics:\n        description: [^\n]+\n        required: false\n        type: boolean\n        default: false/);
  const observation = workflow.split('      - name: Observe exact installed-checks authority cleanup one hundred times\n')[1].split('      - name: Run original combined native suites\n')[0];
  assert.ok(observation.startsWith("        id: authority\n        if: always() && !cancelled() && inputs.authority_diagnostics && steps.compile.outcome == 'success'\n"));
  assert.ok(observation.includes('set -euo pipefail'));
  assert.ok(observation.includes('test "$(id -u)" -ne 0'));
  const sdk = observation.indexOf("echo 'authority diagnostics stage=sdk_check'\n");
  const assertion = observation.indexOf("printf '%s\\n' '#include <libproc.h>' '_Static_assert(PROC_FLAG_INEXIT == 4, \"marmish_inexit_abi\");' | xcrun clang -x c -fsyntax-only -");
  const sdkPassed = observation.indexOf("echo 'authority diagnostics stage=sdk_check_passed'\n");
  const tests = observation.indexOf("echo 'authority diagnostics stage=tests'\n");
  const loop = observation.indexOf('for iteration in {1..100}; do\n');
  assert.ok(sdk >= 0 && assertion > sdk && sdkPassed > assertion && tests > sdkPassed && loop > tests);
  assert.ok(observation.includes('log=$(mktemp "$RUNNER_TEMP/marmish-authority-diagnostics.XXXXXX")'));
  assert.ok(observation.includes("trap 'rm -f \"$log\"' EXIT"));
  assert.ok(observation.includes('test_name=stream_runner::tests::authority_withdrawal_halts_queued_input_irreversibly_without_cancel\n'));
  assert.ok(observation.includes('echo "authority installed-checks iteration=$iteration test=$test_name"'));
  assert.ok(observation.includes('cargo test --locked -p marmish-native --features installed-checks --lib "$test_name" -- --exact --nocapture 2>&1 | tee "$log"'));
  assert.ok(observation.includes("grep -Eq '^test result: ok\\. 1 passed; 0 failed; 0 ignored;' \"$log\""));
  assert.ok(observation.includes('test "$(grep -c \'^test result:\' "$log")" -eq 1'));
  const proofDebug = 'cargo test --locked -p marmish-engine --lib platform::macos::tests::zombie_proof_restart_ -- --nocapture 2>&1 | tee "$log"';
  const proofRelease = 'cargo test --locked --release -p marmish-engine --lib platform::macos::tests::zombie_proof_restart_ -- --nocapture 2>&1 | tee "$log"';
  const proofOrder = [
    sdkPassed,
    observation.indexOf("echo 'authority diagnostics stage=deterministic_proof_tests'\n"),
    observation.indexOf(proofDebug),
    observation.indexOf("echo 'authority diagnostics stage=deterministic_proof_tests_debug_passed'\n"),
    observation.indexOf("echo 'authority diagnostics stage=deterministic_proof_tests_release'\n"),
    observation.indexOf(proofRelease),
    observation.indexOf("echo 'authority diagnostics stage=deterministic_proof_tests_release_passed'\n"),
    tests, loop,
  ];
  assert.ok(proofOrder.every((position, index) => position >= 0 && (index === 0 || position > proofOrder[index - 1])));
  const proofChecks = /grep -Eq '\^test result: ok\\\. 7 passed; 0 failed; 0 ignored;' "\$log"\n          test "\$\(grep -c '\^test result:' "\$log"\)" -eq 1\n/g;
  assert.equal((observation.match(proofChecks) ?? []).length, 2);
  assert.equal((observation.match(/cargo test /g) ?? []).length, 3);
  assert.doesNotMatch(observation, /--no-fail-fast|--ignored|--include-ignored|--skip|continue-on-error|\|\| true/);
});

test('authority observation preserves original runtime checks and strict compilation', () => {
  const compile = workflow.indexOf(`${combined} --no-run\n`);
  const workers = workflow.indexOf('for binary in marmish-engine-worker marmish-worker; do');
  const authority = workflow.indexOf('      - name: Observe exact installed-checks authority cleanup one hundred times\n');
  const run = workflow.indexOf('      - name: Run original combined native suites\n');
  const repeats = workflow.indexOf('      - name: Repeat exact cleanup regressions twenty times\n');
  const clippy = workflow.indexOf('      - name: Verify strict ordinary-release diagnostics compilation\n');
  assert.ok(compile >= 0 && workers > compile && authority > workers && run > authority && repeats > run && clippy > repeats);
  const original = workflow.slice(run, repeats);
  assert.ok(original.includes(followupCondition));
  assert.ok(original.includes(`${combined}\n`));
  assert.doesNotMatch(original, /--features|--skip|--ignored|--include-ignored/);
  const focused = workflow.slice(repeats, clippy);
  assert.ok(focused.includes(followupCondition));
  assert.match(focused, /for iteration in \{1\.\.20\}; do/);
  assert.equal((focused.match(/stream_runner::tests::/g) ?? []).length, 4);
  assert.equal((focused.match(/authority_diagnostics/g) ?? []).length, 1);
  assert.doesNotMatch(focused.replace(followupCondition, ''), /--features|authority_diagnostics/);
  const strict = workflow.slice(clippy);
  assert.ok(strict.includes(followupCondition));
  assert.ok(strict.includes('if [[ "${{ inputs.authority_diagnostics }}" == true ]]; then'));
  const releaseClippy = strict.indexOf('cargo clippy --locked --release -p marmish-native -p marmish-engine --all-targets -- -D warnings\n');
  const debugBegin = strict.indexOf("echo 'authority diagnostics stage=debug_clippy'\n");
  const debugClippy = strict.indexOf('cargo clippy --locked -p marmish-native -p marmish-engine --features marmish-native/installed-checks --all-targets -- -D warnings\n');
  const debugPassed = strict.indexOf("echo 'authority diagnostics stage=debug_clippy_passed'\n");
  assert.ok(releaseClippy >= 0 && debugBegin > releaseClippy && debugClippy > debugBegin && debugPassed > debugClippy);
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

test('focused collection retains twenty repetitions and rejects zero tests when admitted', () => {
  assert.equal(workflow.split(followupCondition).length - 1, 3);
  assert.match(workflow, /for iteration in \{1\.\.20\}; do/);
  for (const name of [
    'final_handle_drop_releases_full_unacked_output_after_exit',
    'final_handle_drop_releases_full_unacked_output_race',
    'final_handle_drop_cancels_live_full_unacked_output',
    'final_service_handle_drop_finishes_owned_cleanup',
  ]) assert.ok(workflow.includes(`stream_runner::tests::${name}\n`));
  assert.ok(workflow.includes('cargo test --locked -p marmish-native --lib "$test_name" -- --exact --nocapture 2>&1 | tee "$log"'));
  assert.ok(workflow.includes("grep -Eq '^test result: ok\\. 1 passed; 0 failed; 0 ignored;' \"$log\""));
  assert.equal((workflow.match(/set -euo pipefail/g) ?? []).length, 6);
  assert.doesNotMatch(workflow, /continue-on-error|\|\| true/);
});

test('authority failure stops follow-ups while default and successful diagnostics retain them', () => {
  assert.equal((workflow.match(/always\(\)/g) ?? []).length, 4);
  assert.equal((workflow.match(/!cancelled\(\)/g) ?? []).length, 4);
  assert.equal((workflow.match(/^        id: authority$/gm) ?? []).length, 1);
  const cases = [
    [false, 'skipped', 'success', false, true],
    [true, 'success', 'success', false, true],
    [true, 'failure', 'success', false, false],
    [true, 'skipped', 'success', false, false],
    [true, 'cancelled', 'success', false, false],
    [false, 'skipped', 'success', true, false],
    [true, 'success', 'success', true, false],
    [false, 'skipped', 'failure', false, false],
    [true, 'success', 'skipped', false, false],
    [true, 'success', 'cancelled', false, false],
  ];
  for (const [diagnostics, authority, compile, cancelled, expected] of cases) {
    assert.equal(!cancelled && compile === 'success' && (!diagnostics || authority === 'success'), expected);
  }
});

test('no live traffic, provider, installation or publication authority is added', () => {
  assert.doesNotMatch(workflow, /secrets\.|github\.token|GH_TOKEN|GITHUB_TOKEN|: write|npm |node |curl |wget |wss:|relay|install\.sh|cargo publish|gh release|upload-artifact|cargo build --release/);
  assert.equal((workflow.match(/^      - uses:/gm) ?? []).length, 2);
  assert.doesNotMatch(workflow, /streaming-candidate\.yml|streaming-installed\.yml|v02-release\.yml/);
});
