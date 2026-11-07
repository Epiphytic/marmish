import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import {STREAMING_COHORTS, STREAMING_BOUNDS, streamingServerBudget} from './streaming-policy.mjs';
import {cohortLifetime} from './workflow.mjs';
import {binding} from './transport.mjs';
import {timing} from './supervise.mjs';
const context={schema:1,repository:'Epiphytic/marmish',run_id:12,run_attempt:3,
  source_commit:'a'.repeat(40),cohort:'stream-matrix',created_at:1000,expires_at:4300};
test('eight closed streaming waves have isolated artifacts and reviewed watchdog budgets',()=>{
  assert.equal(STREAMING_COHORTS.length,8);
  for(const cohort of STREAMING_COHORTS) {
    const c={...context,cohort};
    assert.deepEqual(cohortLifetime({TEST_COHORT:cohort}),{name:cohort,seconds:3300});
    assert.equal(binding(c,'linux-amd64'),`v02-installed-12-3-aaaaaaaaaaaa-${cohort}`);
    assert.deepEqual(timing(c,4720,'run',1100_000),{deadline:4300_000,cleanupDeadline:4600_000});
    assert.equal(timing(c,4720,'exchange',1100_000).deadline,1900_000);
    assert.throws(()=>timing(c,4719,'run',1100_000));
  }
  for(const cohort of ['stream','constructor','copy-interrupt','unknown','transfer-edges']) {
    assert.throws(()=>cohortLifetime({TEST_COHORT:cohort}));
    assert.throws(()=>binding({...context,cohort},'linux-amd64'));
  }
});
test('new budgets do not broaden old matrix or stale preparation acceptance',()=>{
  const old={...context,cohort:'matrix',expires_at:3220};
  assert.deepEqual(timing(old,4120,'run',1100_000),{deadline:3220_000,cleanupDeadline:3820_000});
  assert.throws(()=>timing({...context,cohort:'matrix'},5200,'run',1100_000));
  assert.throws(()=>timing(context,4720,'exchange',1900_000));
  assert.throws(()=>timing({...context,created_at:1200},4720,'run',1100_000));
});
test('late server admission fails instead of consuming readiness or cleanup reserve',()=>{
  const budget=streamingServerBudget(context,1720);
  assert.deepEqual(budget,{readyBy:1900,trafficBy:3100,stopBy:3280});
  assert.equal(STREAMING_BOUNDS.uptime-(budget.stopBy-1720),240);
  for(const [c,start,now] of [[context,1721,1721],[context,1720,1900],
    [{...context,expires_at:3699},1720,1720],[{...context,cohort:'matrix'},1720,1720],
    [{...context,expires_at:4301},1720,1720]]) {
    assert.throws(()=>streamingServerBudget(c,start,now),/streaming_start_budget/);
  }
});
test('candidate-only workflows retain three platforms and closed nonpublishing scope',async()=>{
  const read=name=>fs.readFile(new URL(`../../.github/workflows/${name}`,import.meta.url),'utf8');
  const candidate=await read('streaming-candidate.yml'), installed=await read('streaming-installed.yml');
  for(const target of ['x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu','aarch64-apple-darwin']) {
    assert.ok(candidate.includes(`target: ${target}`)); assert.ok(installed.includes(`target: ${target}`));
  }
  assert.ok(candidate.includes('test "$MARMISH_BUILD_COMMIT" = "$GITHUB_SHA"'));
  const nativeStep=candidate.split('      - name: Verify native components\n')[1].split('      - name: Build final native executables\n')[0];
  assert.ok(nativeStep.includes("MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS: '1'"));
  assert.ok(nativeStep.includes('test "$(id -u)" -ne 0'));
  assert.ok(nativeStep.includes('cargo test --locked --no-fail-fast -p marmish-engine -p marmish-stream -p marmish-native -p marmish-vault'));
  for(const forbidden of ['contents: write','gh release','git push','release-eligibility','caveated_release','cargo publish','id-token','gh api']) {
    assert.ok(!candidate.includes(forbidden),forbidden);
  }
  const options=installed.match(/options: \[([^\]]+)\]/)[1].split(',').map(s=>s.trim());
  assert.deepEqual(options,STREAMING_COHORTS);
  assert.ok(installed.includes('timeout-minutes: 65'));
  assert.ok(installed.includes('3720'));
  assert.ok(installed.includes('eight reviewed, separately dispatched installed cohorts'));
  assert.match(installed,/name: Verify maintained rsync prerequisite without Linux privilege escalation\n\s+timeout-minutes: 5\n/);
  assert.ok(installed.includes('MARMISH_JOB_DEADLINE=$(( $(date +%s) + 3720 ))'));
  assert.ok(!installed.includes('contents: write'));
  assert.ok(!installed.includes('gh release'));
  assert.ok(installed.includes('Number(m[2]) < 2 || Number(m[2]) === 2 && Number(m[3]) < 7'));
  assert.ok(installed.includes('MARMISH_TEST_RSYNC=$rsync_image'));
});

// Restrict the budget check to job-level keys; unrelated jobs/steps cannot satisfy it.
function candidateBuildBudget(source) {
  const jobs=[...source.matchAll(/^  (package|render):\n([\s\S]*?)(?=^  [a-z][a-z_-]*:|$(?![\s\S]))/gm)];
  assert.deepEqual(jobs.map(job=>job[1]),['package','render']);
  for(const [index,expected] of [[0,'120'],[1,'5']]) {
    const caps=[...jobs[index][2].matchAll(/^    timeout-minutes: ([0-9]+)\s*$/gm)];
    assert.deepEqual(caps.map(cap=>cap[1]),[expected]);
  }
}
function candidateStep(source,name) {
  const marker=`      - name: ${name}\n`;
  assert.equal(source.split(marker).length,2,`unique step: ${name}`);
  return source.split(marker)[1].split('\n      - ')[0];
}
test('candidate package budget is bounded independently of render and installed traffic',async()=>{
  const candidate=await fs.readFile(new URL('../../.github/workflows/streaming-candidate.yml',import.meta.url),'utf8');
  candidateBuildBudget(candidate);
  const cap='    timeout-minutes: 120\n';
  for(const replacement of ['', '    timeout-minutes: 90\n',cap+cap]) {
    assert.throws(()=>candidateBuildBudget(candidate.replace(cap,replacement)));
  }
  assert.throws(()=>candidateBuildBudget(candidate.replace(cap,'    timeout-minutes: 90\n')+
    '  unrelated:\n    timeout-minutes: 120\n'));
  assert.throws(()=>candidateBuildBudget(candidate.replace('    timeout-minutes: 5\n',cap)));
  const native=candidateStep(candidate,'Verify native components');
  for(const command of [
    'cargo test --locked --no-fail-fast -p marmish-engine -p marmish-stream -p marmish-native -p marmish-vault',
    'cargo clippy --locked -p marmish-native -p marmish-engine --all-targets -- -D warnings',
  ]) assert.ok(native.includes(command),command);
  const isolated=candidateStep(candidate,'Build isolated installed-test tools without changing product assets');
  for(const command of [
    'export CARGO_TARGET_DIR="$RUNNER_TEMP/installed-tools-target"',
    'cargo test --locked -p marmish-native --features installed-checks',
    'cargo clippy --locked -p marmish-native --features installed-checks --all-targets -- -D warnings',
    'cargo build --locked --release -p marmish-native --features installed-checks ' + '\\' +
      '\n            --bin marmish-installed-check --bin marmish-installed-fixture',
    'shasum -a 256 "$RUNNER_TEMP"/native-assets/* > "$RUNNER_TEMP/product-before.sha256"',
    'shasum -a 256 "$RUNNER_TEMP"/native-assets/* > "$RUNNER_TEMP/product-after.sha256"',
    'cmp "$RUNNER_TEMP/product-before.sha256" "$RUNNER_TEMP/product-after.sha256"',
  ]) assert.ok(isolated.includes(command),command);
});

test('native enforcing budget constants remain equal to the Node mirror',async()=>{
  const source=await fs.readFile(new URL('../../crates/marmish-native/src/installed_streaming_records.rs',import.meta.url),'utf8');
  for(const fragment of [
    `start > ctx.created_at.saturating_add(${STREAMING_BOUNDS.latestServerStart})`,
    `ctx.expires_at.saturating_sub(start) < ${STREAMING_BOUNDS.minimumRemaining}`,
    `let ready_by = start.checked_add(${STREAMING_BOUNDS.ready})`,
    `(ready_by.checked_add(${STREAMING_BOUNDS.traffic}).ok_or_else(bad)?).min(ctx.expires_at.saturating_sub(${STREAMING_BOUNDS.cleanup}))`,
    `let stop_by = traffic_by.checked_add(${STREAMING_BOUNDS.cleanup})`,
    `stop_by > start.checked_add(${STREAMING_BOUNDS.uptime})`,
  ]) assert.ok(source.includes(fragment),fragment);
  assert.equal(STREAMING_BOUNDS.latestServerStart,720);
  assert.equal(STREAMING_BOUNDS.minimumRemaining,1980);
  assert.equal(STREAMING_BOUNDS.ready,180);assert.equal(STREAMING_BOUNDS.traffic,1200);
  assert.equal(STREAMING_BOUNDS.cleanup,180);assert.equal(STREAMING_BOUNDS.uptime,1800);
});

test('streaming report failure marker is uploaded and overrides a partial PASS',async()=>{
  const source=await fs.readFile(new URL('../../crates/marmish-native/src/installed_streaming.rs',import.meta.url),'utf8');
  assert.ok(source.includes('const ROLLUP_FAILURE: &str = "matrix-rollup.failure.json"'));
  const workflow=await fs.readFile(new URL('../../.github/workflows/streaming-installed.yml',import.meta.url),'utf8');
  const path=workflow.split('\n').find(line=>line.trimStart().startsWith('path:')&&line.includes('matrix-rollup.*'));
  assert.ok(path,'failure-marker upload must retain report wildcard');
  const basename=path.trim().split('/').at(-1);
  const pattern=basename.split('*').map(part=>part.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')).join('.*');
  assert.match('matrix-rollup.failure.json',new RegExp(`^${pattern}$`));
  const upload=workflow.slice(workflow.lastIndexOf('      - uses: actions/upload-artifact@'));
  assert.ok(upload.includes('if: always()'));
  assert.ok(upload.includes('if-no-files-found: error'));
});
