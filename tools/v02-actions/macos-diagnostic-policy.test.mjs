import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const workflow = readFileSync(new URL('../../.github/workflows/streaming-macos-diagnostic.yml', import.meta.url), 'utf8');
function step(yaml, name) {
  const prefix = `      - name: ${name}\n`;
  const start = yaml.indexOf(prefix);
  assert.notEqual(start, -1, `missing step: ${name}`);
  const next = yaml.indexOf('\n      - ', start + prefix.length);
  return yaml.slice(start, next < 0 ? undefined : next);
}
function runtimeBoundary(yaml, name = 'Diagnose installed old product') {
  const run = step(yaml, name);
  assert.match(run, /timeout-minutes: 19\n/);
  assert.doesNotMatch(run, /GH_TOKEN|github\.token|GITHUB_TOKEN|secrets\.|continue-on-error/);
  for (const [flag, suffix] of [
    ['assets', 'assets'], ['bootstrap', 'bootstrap/install.sh'],
    ['fixture', 'tools/marmish-installed-fixture'], ['pins', 'pins.json'],
    ['workspace', 'work'], ['output', 'output'],
  ]) assert.ok(run.includes(`--${flag} "$DIAGNOSTIC_ROOT/${suffix}"`), flag);
  assert.match(run, /target\/debug\/marmish-installed-check "\$\{args\[@\]\}"/);
  assert.doesNotMatch(run, /old.*marmish-installed-check|\/tools\/marmish-installed-check/);
}
function uploadBoundary(yaml) {
  const upload = step(yaml, 'Preserve sanitized diagnostic records only');
  assert.match(upload, /if: always\(\) && steps\.prepare\.outcome == 'success'/);
  const paths = upload.split('          path: |\n')[1]?.split('\n          if-no-files-found:')[0].trim().split('\n').map(s => s.trim());
  assert.deepEqual(paths, [
    '${{ env.DIAGNOSTIC_ROOT }}/provenance.json',
    '${{ env.DIAGNOSTIC_ROOT }}/output/report.json',
    '${{ env.DIAGNOSTIC_ROOT }}/output/complete.json',
    '${{ env.DIAGNOSTIC_ROOT }}/output/failure.json',
  ]);
  assert.match(upload, /if-no-files-found: error\n/);
  assert.match(upload, /retention-days: 7\n/);
}

test('manual diagnostic has one Mac ARM64 job, read-only authority and exclusive installed lane', () => {
  assert.match(workflow, /on:\n  workflow_dispatch:\n/);
  assert.doesNotMatch(workflow, /\n  (push|pull_request|schedule|workflow_run):/);
  assert.match(workflow, /permissions:\n  contents: read\n  actions: read\nconcurrency:/);
  assert.match(workflow, /group: marmish-streaming-installed\n  cancel-in-progress: false/);
  assert.deepEqual([...workflow.matchAll(/^  ([a-z][\w-]*):\n(?=    if:)/gm)].map(m => m[1]), ['diagnostic']);
  assert.match(workflow, /runs-on: macos-15\n    timeout-minutes: 48/);
  assert.match(workflow, /test "\$\(uname -s\):\$\(uname -m\)" = Darwin:arm64/);
  assert.doesNotMatch(workflow, /secrets\.|: write|ubuntu-|windows-|strategy:|matrix:/);
});

test('harness uses full history, exact workflow SHA and checked default branch', () => {
  assert.match(workflow, /ref: \$\{\{ inputs\.harness_commit \}\}\n          persist-credentials: false\n          fetch-depth: 0/);
  const verify = step(workflow, 'Verify reviewed harness and Mac ARM64 host');
  assert.match(verify, /\[\[ "\$HARNESS_COMMIT" =~ \^\[0-9a-f\]\{40\}\$ \]\]/);
  assert.ok(verify.includes('test "$(git rev-parse HEAD)" = "$HARNESS_COMMIT"'));
  assert.ok(verify.includes('test "$HARNESS_COMMIT" = "$GITHUB_SHA"'));
  assert.ok(verify.includes('git merge-base --is-ancestor "$HARNESS_COMMIT" refs/remotes/origin/master'));
  assert.match(workflow, /concurrent:[\s\S]*?default: false\n        type: boolean/);
  assert.doesNotMatch(workflow, /candidate_run:|candidate_source:|candidate_attempt:/);
});

test('only diagnostic probe is built, bounded, locked and signed', () => {
  const build = step(workflow, 'Build diagnostic probe only');
  assert.match(build, /timeout-minutes: 23\n/);
  assert.ok(build.includes('cargo build --locked -p marmish-native --features installed-checks --bin marmish-installed-check'));
  assert.doesNotMatch(build, /--release|--bins|--bin marmish-installed-fixture|GH_TOKEN|github\.token/);
  assert.match(build, /codesign --force --sign - target\/debug\/marmish-installed-check/);
  assert.match(build, /codesign --verify --strict target\/debug\/marmish-installed-check/);
  assert.match(workflow, /toolchain: 1\.97\.1\n/);
  assert.match(workflow, /MACOSX_DEPLOYMENT_TARGET: '15\.0'/);
});

test('private canonical root and frozen old acquisition precede credential-free execution', () => {
  const allocate = step(workflow, 'Allocate private diagnostic root');
  assert.match(allocate, /umask 077/);
  assert.match(allocate, /mktemp -d \/private\/tmp\/mm\.XXXXXX/);
  assert.match(allocate, /chmod 700 "\$root"/);
  assert.ok(allocate.includes('test "$(cd "$root" && pwd -P)" = "$root"'));
  const acquire = step(workflow, 'Acquire and verify immutable old candidate');
  assert.match(acquire, /GH_TOKEN: \$\{\{ github\.token \}\}/);
  assert.match(acquire, /PROBE: \$\{\{ github\.workspace \}\}\/target\/debug\/marmish-installed-check/);
  assert.match(acquire, /run: node tools\/v02-actions\/macos-diagnostic\.mjs$/);
  assert.equal((workflow.match(/GH_TOKEN:/g) ?? []).length, 1);
  assert.ok(workflow.indexOf('Build diagnostic probe only') < workflow.indexOf('Acquire and verify immutable old candidate'));
  assert.ok(workflow.indexOf('Acquire and verify immutable old candidate') < workflow.indexOf('Diagnose installed old product'));
  assert.doesNotMatch(workflow, /actions\/download-artifact|curl|wget/);
  runtimeBoundary(workflow);
});

test('runtime policy rejects credential leakage, wrong fixture and timeout expansion', () => {
  assert.throws(() => runtimeBoundary(workflow.replace('RUN_CONCURRENT:', 'GH_TOKEN:')));
  assert.throws(() => runtimeBoundary(workflow.replace('--fixture "$DIAGNOSTIC_ROOT/tools/marmish-installed-fixture"', '--fixture "$DIAGNOSTIC_ROOT/new-fixture"')));
  assert.throws(() => runtimeBoundary(workflow.replace('timeout-minutes: 19', 'timeout-minutes: 30')));
});

test('probe failure cannot turn green via marker existence or upload success', () => {
  const validate = step(workflow, 'Independently validate closed diagnostic records');
  assert.match(validate, /if: always\(\) && steps\.probe\.outcome != 'skipped'/);
  assert.match(validate, /run: node tools\/v02-actions\/macos-diagnostic\.mjs validate-report/);
  assert.doesNotMatch(workflow, /continue-on-error|\|\| true|set \+e/);
  assert.doesNotMatch(validate, /GH_TOKEN|github\.token|test -[ef]/);
  assert.match(validate, /PROBE: \$\{\{ github\.workspace \}\}\/target\/debug\/marmish-installed-check/);
  assert.match(validate, /RUN_CONCURRENT: \$\{\{ inputs\.concurrent \}\}/);
});

test('uploads name only sanitized closed records on both success and failure', () => {
  uploadBoundary(workflow);
  assert.throws(() => uploadBoundary(workflow.replace('/output/report.json', '/output/*')));
  assert.throws(() => uploadBoundary(workflow.replace('/output/failure.json', '/work/config.json')));
  assert.equal((workflow.match(/uses: actions\/upload-artifact@/g) ?? []).length, 1);
});

test('diagnostic cannot publish source, releases or packages', () => {
  assert.doesNotMatch(workflow, /git (push|tag|commit)|cargo publish|gh release|rad (push|sync)|npm publish|workflow run|repository_dispatch/);
  assert.match(workflow, /not qualification/);
});

const coldWorkflow = readFileSync(new URL('../../.github/workflows/streaming-macos-cold-multiprincipal.yml', import.meta.url), 'utf8');
const coldStep = 'Diagnose cold multiprincipal old product';

test('cold workflow retains manual read-only Mac authority and fixed administrative bounds', () => {
  assert.match(coldWorkflow, /on:\n  workflow_dispatch:\n/);
  assert.doesNotMatch(coldWorkflow, /\n  (push|pull_request|schedule|workflow_run):/);
  assert.match(coldWorkflow, /permissions:\n  contents: read\n  actions: read\nconcurrency:/);
  assert.match(coldWorkflow, /group: marmish-streaming-installed\n  cancel-in-progress: false/);
  assert.deepEqual([...coldWorkflow.matchAll(/^  ([a-z][\w-]*):\n(?=    if:)/gm)].map(m => m[1]), ['diagnostic']);
  assert.match(coldWorkflow, /runs-on: macos-15\n    timeout-minutes: 48/);
  assert.doesNotMatch(coldWorkflow, /secrets\.|: write|ubuntu-|windows-|strategy:|matrix:|concurrent:|RUN_CONCURRENT|--concurrent/);
  assert.doesNotMatch(coldWorkflow, /candidate_run:|candidate_source:|candidate_attempt:/);
  assert.doesNotMatch(coldWorkflow, /git (push|tag|commit)|cargo publish|gh release|rad (push|sync)|npm publish|workflow run|repository_dispatch/);
  assert.match(coldWorkflow, /not qualification/);
});

test('cold workflow inherits exact reviewed source, private root and probe-only signed build', () => {
  // All unchanged steps must remain literal copies of the reviewed original policy.
  for (const name of ['Verify reviewed harness and Mac ARM64 host', 'Allocate private diagnostic root', 'Build diagnostic probe only', 'Install locked acquisition tooling']) {
    assert.equal(step(coldWorkflow, name), step(workflow, name));
  }
  assert.match(coldWorkflow, /ref: \$\{\{ inputs\.harness_commit \}\}\n          persist-credentials: false\n          fetch-depth: 0/);
  assert.match(coldWorkflow, /toolchain: 1\.97\.1\n/);
  assert.match(coldWorkflow, /MACOSX_DEPLOYMENT_TARGET: '15\.0'/);
  assert.ok(coldWorkflow.indexOf('Build diagnostic probe only') < coldWorkflow.indexOf('Acquire and verify immutable old candidate'));
});

test('cold acquisition and validator use distinct fixed modes with credential-free execution', () => {
  const acquire = step(coldWorkflow, 'Acquire and verify immutable old candidate');
  assert.equal(acquire, step(workflow, 'Acquire and verify immutable old candidate').replace('macos-diagnostic.mjs', 'macos-diagnostic.mjs acquire-cold'));
  assert.equal((coldWorkflow.match(/GH_TOKEN:/g) ?? []).length, 1);
  assert.doesNotMatch(coldWorkflow, /actions\/download-artifact|curl|wget/);
  assert.ok(coldWorkflow.indexOf('Acquire and verify immutable old candidate') < coldWorkflow.indexOf(coldStep));
  const probe = step(coldWorkflow, coldStep);
  runtimeBoundary(coldWorkflow, coldStep);
  assert.match(probe, /args=\(macos-cold-multiprincipal-diagnostic\n/);
  assert.doesNotMatch(probe, /RUN_CONCURRENT|--concurrent|macos-diagnostic\n/);
  const validate = step(coldWorkflow, 'Independently validate closed diagnostic records');
  assert.match(validate, /if: always\(\) && steps\.probe\.outcome != 'skipped'/);
  assert.match(validate, /PROBE: \$\{\{ github\.workspace \}\}\/target\/debug\/marmish-installed-check/);
  assert.match(validate, /run: node tools\/v02-actions\/macos-diagnostic\.mjs validate-cold-report/);
  assert.doesNotMatch(validate, /GH_TOKEN|github\.token|RUN_CONCURRENT/);
  assert.doesNotMatch(coldWorkflow, /continue-on-error|\|\| true|set \+e/);
});

test('cold workflow uploads only closed sanitized evidence with a distinct artifact name', () => {
  uploadBoundary(coldWorkflow);
  assert.match(coldWorkflow, /name: macos-cold-multiprincipal-diagnostic-\$\{\{ github\.run_id \}\}-\$\{\{ github\.run_attempt \}\}/);
  assert.equal((coldWorkflow.match(/uses: actions\/upload-artifact@/g) ?? []).length, 1);
  assert.throws(() => uploadBoundary(coldWorkflow.replace('/output/report.json', '/output/*')));
});

test('cold runtime rejects secret exposure, wrong fixture and expanded deadline', () => {
  assert.throws(() => runtimeBoundary(coldWorkflow.replace('id: probe', 'id: probe\n        env:\n          GH_TOKEN: bad'), coldStep));
  assert.throws(() => runtimeBoundary(coldWorkflow.replace('--fixture "$DIAGNOSTIC_ROOT/tools/marmish-installed-fixture"', '--fixture "$DIAGNOSTIC_ROOT/new-fixture"'), coldStep));
  assert.throws(() => runtimeBoundary(coldWorkflow.replace('timeout-minutes: 19', 'timeout-minutes: 30'), coldStep));
});
