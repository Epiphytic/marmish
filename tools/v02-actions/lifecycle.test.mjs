import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {execFileSync} from 'node:child_process';
import {lifecycleBudget, runtimeExpiry, lifecycleSequence, collectLifecycle, lifecycleRollup} from './lifecycle.mjs';
import {sourceDigest, validateCandidate, TARGETS} from './workflow.mjs';
import {digest, ROLES} from './transport.mjs';

test('fixed original build/runtime budgets do not renew after work or queue delay', () => {
  const start = 10_000, end = start + 85 * 60_000;
  assert.equal(lifecycleBudget(start, end, start).buildEnd, start + 45 * 60_000);
  assert.equal(lifecycleBudget(start, end, start + 44 * 60_000).buildEnd, start + 45 * 60_000);
  assert.throws(() => lifecycleBudget(start, end + 1, start));
  assert.throws(() => lifecycleBudget(start, end, start - 1));
  assert.throws(() => lifecycleBudget(start, end, start + 45 * 60_000));
  assert.equal(runtimeExpiry(end, end - 2100_000), Math.floor((end - 2100_000) / 1000) + 1800);
  assert.throws(() => runtimeExpiry(end, end - 2100_000 + 1));
});
test('lifecycle workflow keeps finite hosted bounds and credentials outside runtime step', async () => {
  const yaml=await fs.readFile(new URL('../../.github/workflows/v02-version-lifecycle.yml',import.meta.url),'utf8');
  assert.match(yaml,/cancel-in-progress: false/);
  assert.match(yaml,/timeout-minutes: 90/);
  const retention=[...yaml.matchAll(/retention-days: (\d+)/g)].map(m=>Number(m[1]));
  assert.equal(retention.length,4);
  assert.ok(retention.every(days=>days>0&&days<=7));
  const step=yaml.match(/- name: Build genuine variants[\s\S]*?(?=\n      - name:)/)?.[0];
  assert.ok(step);assert.doesNotMatch(step,/GH_TOKEN|github.token/);
});
async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-lifecycle-'));
  t.after(() => fs.rm(root, {recursive:true, force:true}));
  const sourceRoot = execFileSync('git', ['rev-parse','--show-toplevel'], {encoding:'utf8'}).trim();
  const source = execFileSync('git', ['rev-parse','HEAD'], {encoding:'utf8'}).trim();
  const env = {GITHUB_REPOSITORY:'Epiphytic/marmish', SOURCE_COMMIT:source, GITHUB_SHA:source,
    GITHUB_RUN_ID:'100', GITHUB_RUN_ATTEMPT:'1', CANDIDATE_RUN_ID:'90', CANDIDATE_RUN_ATTEMPT:'2',
    TEST_COHORT:'version-lifecycle', TEST_RELAY:'wss://relay.example'};
  const pins = path.join(root,'pins'), toolRoot = path.join(root,'tools');
  await fs.mkdir(path.join(pins,'tooling'), {recursive:true}); await fs.mkdir(toolRoot);
  const reply = {schema:1, version:'0.2.0', target:TARGETS[0], commit:source, formats:[2,6,1,1,1]};
  const script = Buffer.from(`#!/bin/sh\nprintf '%s' '${JSON.stringify(reply)}'\n`);
  const record = {schema:1, source_commit:source, harness_sha256:sourceDigest(source), target:TARGETS[0], files:{}};
  for (const name of ['marmish-installed-check','marmish-installed-fixture']) {
    await fs.writeFile(path.join(toolRoot,name),script, {mode:0o600});
    record.files[name] = {sha256:digest(script),bytes:script.length};
  }
  const bytes = JSON.stringify(record);
  await fs.writeFile(path.join(toolRoot,'linux-amd64-tooling.json'),bytes);
  await fs.writeFile(path.join(pins,'tooling/linux-amd64-tooling.json'),bytes);
  await fs.mkdir(path.join(pins,'manifests'));
  await fs.mkdir(path.join(root,'assets'));
  await fs.writeFile(path.join(root,'assets/candidate.manifest'),'manifest');
  await fs.writeFile(path.join(pins,'manifests/linux-amd64.manifest'),'manifest');
  await fs.writeFile(path.join(root,'install.sh'),'bootstrap');
  await fs.writeFile(path.join(pins,'bootstrap.sha256'),digest(Buffer.from('bootstrap')));
  const started = Date.now(), internal = started + 85 * 60_000;
  return {env,sourceRoot,pins,toolRoot,assets:path.join(root,'assets'),bootstrap:path.join(root,'install.sh'),
    variants:path.join(root,'variants'),workspace:path.join(root,'work'),output:path.join(root,'output'),
    role:'linux-amd64',started,internal};
}
test('build precedes fresh context; exact pinned tools and all operations share original cutoff', async t => {
  const o = await fixture(t), calls = [];
  let time = o.started;
  await lifecycleSequence(o, undefined, async (file,args,options) => {
    calls.push({file,args,options});
    if (calls.length === 1) time += 40 * 60_000;
  }, () => time);
  assert.equal(calls.length,4);
  assert.equal(calls[0].file,'/usr/bin/python3');
  assert.equal(calls[0].options.deadline,o.started + 45 * 60_000);
  assert.equal(calls[0].args[calls[0].args.indexOf('--deadline') + 1], String(Math.floor(o.internal / 1000)));
  assert.equal(calls[0].args[calls[0].args.indexOf('--build-deadline') + 1], String(Math.floor(calls[0].options.deadline / 1000)));
  assert.equal(calls[1].args[0],'context'); assert.equal(calls[2].args[0],'lifecycle-context');
  assert.equal(calls[3].args[0],'lifecycle-run');
  const expires = Number(calls[1].args[calls[1].args.indexOf('--expires-at') + 1]);
  assert.equal(expires, Math.floor(time / 1000) + 1800);
  assert.equal(calls[2].args[calls[2].args.indexOf('--internal-deadline') + 1], String(Math.floor(o.internal / 1000)));
  assert.ok(calls.slice(1).every(c => c.options.deadline === expires * 1000));
  assert.ok(calls[3].args.includes(o.workspace));
  assert.equal(calls[3].args[calls[3].args.indexOf('--bootstrap') + 1],o.bootstrap);
  assert.equal(calls[3].args[calls[3].args.indexOf('--source-root') + 1],o.sourceRoot);
});
test('failed build or insufficient postbuild reserve cannot mint context or launch product', async t => {
  const o = await fixture(t); let count = 0;
  await assert.rejects(lifecycleSequence(o,undefined,async()=>{count++;throw Error('failed');},()=>o.started));
  assert.equal(count,1);
  count = 0; let time = o.started;
  await assert.rejects(lifecycleSequence(o,undefined,async()=>{count++;time=o.internal-2099_000;},()=>time),/reserve/);
  assert.equal(count,1);
});
test('bootstrap or candidate manifest differs from resolve pins: no builder or runtime starts', async t => {
  const o = await fixture(t); let count = 0;
  await fs.writeFile(o.bootstrap,'tampered');
  await assert.rejects(lifecycleSequence(o,undefined,async()=>count++,()=>o.started),/bootstrap_pin/);
  assert.equal(count,0);
  await fs.writeFile(o.bootstrap,'bootstrap');
  await fs.writeFile(path.join(o.assets,'candidate.manifest'),'tampered');
  await assert.rejects(lifecycleSequence(o,undefined,async()=>count++,()=>o.started),/manifest_pin/);
  assert.equal(count,0);
});
test('abort during build does not mint a runtime context', async t => {
  const o = await fixture(t), abort = new AbortController(); let count = 0;
  await assert.rejects(lifecycleSequence(o,abort.signal,async()=>{count++;abort.abort();},()=>o.started),/cancelled/);
  assert.equal(count,1);
});
test('rollup expected source and both run attempts come from workflow controls', async t => {
  const o=await fixture(t); let call;
  await lifecycleRollup({...o.env,TOOLS_ROOT:o.toolRoot,LIFECYCLE_RECORDS_FLAT:o.pins,
    LIFECYCLE_ROLLUP:o.output,GITHUB_WORKSPACE:o.sourceRoot,LIFECYCLE_JOB_RESULT:'failure'},
  async(file,args)=>{call={file,args};});
  const flag=name=>call.args[call.args.indexOf(name)+1];
  assert.equal(flag('--source'),o.env.SOURCE_COMMIT);
  assert.equal(flag('--run-id'),'100');assert.equal(flag('--attempt'),'1');
  assert.equal(flag('--candidate-run-id'),'90');assert.equal(flag('--candidate-run-attempt'),'2');
  assert.equal(flag('--job-result'),'failure');assert.equal(flag('--harness-sha256'),sourceDigest(o.env.SOURCE_COMMIT));
});
test('artifact validation requires exact candidate attempt and enough retention', async () => {
  const source='a'.repeat(40), until=Date.now()+90*60_000;
  const env={GITHUB_REPOSITORY:'Epiphytic/marmish',SOURCE_COMMIT:source,GITHUB_SHA:source,
    CANDIDATE_RUN_ID:'12',CANDIDATE_RUN_ATTEMPT:'3',TEST_COHORT:'version-lifecycle',TEST_RELAY:'wss://relay.example',RESOLVE_ROOT:'/tmp'};
  const run={id:12,run_attempt:3,status:'completed',conclusion:'success',path:'.github/workflows/v02-release.yml',
    event:'workflow_dispatch',head_branch:'master',head_repository:{full_name:env.GITHUB_REPOSITORY},
    repository:{full_name:env.GITHUB_REPOSITORY},head_sha:source};
  const entries=[...TARGETS.flatMap(target=>[`v02-candidate-${target}-12-3`,`v02-installed-tools-${target}-12-3`]),
    'v02-pinned-bootstrap-12-3'].map(name=>({name,expired:false,expires_at:new Date(until+1).toISOString()}));
  const transport={request:async()=>({bytes:Buffer.from(JSON.stringify(run))}),list:async()=>entries};
  await validateCandidate(env,until,transport);
  entries[0].expires_at=new Date(until-1).toISOString();
  await assert.rejects(validateCandidate(env,until,transport),/artifacts_missing/);
});
test('collect preserves genuine failed results and rejects missing/extra or symlink inputs', async t => {
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-lifecycle-records-'));
  t.after(()=>fs.rm(root,{recursive:true,force:true}));
  const input=path.join(root,'input');await fs.mkdir(input);
  for(const role of ROLES) {
    await fs.mkdir(path.join(input,role));
    await fs.writeFile(path.join(input,role,'lifecycle-context.json'),'{}');
    await fs.writeFile(path.join(input,role,'lifecycle-result.json'),'{"complete":false}');
  }
  const output=path.join(root,'flat');await collectLifecycle(input,output);
  assert.equal((await fs.readdir(output)).length,6);
  assert.equal(await fs.readFile(path.join(output,'linux-amd64-result.json'),'utf8'),'{"complete":false}');
  const extra=path.join(input,ROLES[0],'private.log');await fs.writeFile(extra,'private');
  await assert.rejects(collectLifecycle(input,path.join(root,'extra')),/record_set/);await fs.unlink(extra);
  const file=path.join(input,ROLES[0],'lifecycle-result.json');await fs.unlink(file);
  await assert.rejects(collectLifecycle(input,path.join(root,'missing')),/record_set/);
  await fs.symlink(path.join(input,ROLES[0],'lifecycle-context.json'),file);
  await assert.rejects(collectLifecycle(input,path.join(root,'symlink')));
});
