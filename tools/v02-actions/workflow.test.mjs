import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {candidateInputs, checkRun, checkTooling, verifyTools, verifyContext, gather, sourceDigest, TARGETS} from './workflow.mjs';
import {digest, ROLES} from './transport.mjs';
import {execFileSync} from 'node:child_process';
const source = 'a'.repeat(40), harness = 'b'.repeat(64);
const env = {CANDIDATE_RUN_ID:'12', CANDIDATE_RUN_ATTEMPT:'3', GITHUB_REPOSITORY:'Epiphytic/marmish',
  SOURCE_COMMIT:source, GITHUB_SHA:source, TEST_RELAY:'wss://relay.example'};
const run = {id:12,run_attempt:3,status:'completed',conclusion:'success',path:'.github/workflows/v02-release.yml',
  event:'workflow_dispatch',head_branch:'master',head_repository:{full_name:'Epiphytic/marmish'},
  repository:{full_name:'Epiphytic/marmish'},head_sha:'c'.repeat(40)};
test('candidate inputs bind control source and require explicit direct relay', () => {
  assert.equal(candidateInputs(env).relay,'wss://relay.example/');
  for (const change of [{CANDIDATE_RUN_ID:'../1'},{CANDIDATE_RUN_ATTEMPT:'0'},{GITHUB_SHA:'c'.repeat(40)},
    {GITHUB_REPOSITORY:'fork/marmish'},{TEST_RELAY:'ws://relay.example'},{TEST_RELAY:'wss://u:p@relay.example'},
    {TEST_RELAY:'wss://hidden.onion'},{TEST_RELAY:'wss://relay.example/#x'}]) {
    assert.throws(()=>candidateInputs({...env,...change}));
  }
});
test('exact candidate attempt and workflow required; dispatch head is not selected source', () => {
  const c=candidateInputs(env); checkRun(run,c);
  for (const change of [{run_attempt:2},{id:13},{conclusion:'failure'},{status:'in_progress'},
    {path:'.github/workflows/other.yml'},{head_branch:'feature'},{event:'pull_request'},
    {head_repository:{full_name:'fork/marmish'}}]) assert.throws(()=>checkRun({...run,...change},c));
});
function record(bytes) {
  return {schema:1,target:TARGETS[0],source_commit:source,harness_sha256:harness,
    files:Object.fromEntries(['marmish-installed-check','marmish-installed-fixture'].map(name=>
      [name,{sha256:digest(bytes),bytes:bytes.length}]))};
}
test('tooling records reject open schemas, mismatched source and malformed size pins', () => {
  const good=record(Buffer.from('x')); checkTooling(good,'linux-amd64',source,harness);
  for(const change of [{target:TARGETS[1]},{source_commit:'c'.repeat(40)},{harness_sha256:'d'.repeat(64)},
    {extra:true},{files:{...good.files,extra:{sha256:harness,bytes:1}}}]) {
    assert.throws(()=>checkTooling({...good,...change},'linux-amd64',source,harness));
  }
  assert.throws(()=>checkTooling(good,'invalid',source,harness));
  assert.throws(()=>checkTooling({...good,files:{...good.files,'marmish-installed-check':{sha256:harness,bytes:0}}},'linux-amd64',source,harness));
});
test('bytes and original record digest authenticated before executable permission', async t => {
  const directory=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-workflow-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  const bytes=Buffer.from('not an executable'), good=record(bytes), serialized=Buffer.from(JSON.stringify(good));
  for(const name of Object.keys(good.files)) await fs.writeFile(path.join(directory,name),bytes,{mode:0o600});
  await fs.writeFile(path.join(directory,'linux-amd64-tooling.json'),serialized);
  await assert.rejects(verifyTools(directory,'linux-amd64',source,harness,'e'.repeat(64)),/record_digest/);
  assert.equal((await fs.stat(path.join(directory,'marmish-installed-check'))).mode&0o777,0o600);
  await fs.writeFile(path.join(directory,'marmish-installed-check'),Buffer.from('wrong'));
  await assert.rejects(verifyTools(directory,'linux-amd64',source,harness,digest(serialized)),/tool_digest/);
  assert.equal((await fs.stat(path.join(directory,'marmish-installed-check'))).mode&0o777,0o600);
  await fs.writeFile(path.join(directory,'marmish-installed-check'),bytes);
  await verifyTools(directory,'linux-amd64',source,harness,digest(serialized));
  assert.equal((await fs.stat(path.join(directory,'marmish-installed-check'))).mode&0o777,0o500);
});

test('source digest matches the candidate immutable git-tree pipeline', () => {
  const sha=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
  const tree=execFileSync('git',['ls-tree','-r',sha,'--','crates/marmish-native','Cargo.lock',
    'tools/v02-actions','.github/workflows/v02-installed.yml']);
  const expected=execFileSync('shasum',['-a','256'],{input:tree,encoding:'utf8'}).split(' ')[0];
  assert.equal(sourceDigest(sha),expected);
});
test('missing tooling pin or different candidate attempt fails before executing a tool', async t => {
  const directory=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-context-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  const sha=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
  const context={repository:'Epiphytic/marmish',source_commit:sha,run_id:12,run_attempt:3,
    candidate_run_id:10,candidate_run_attempt:2,harness_sha256:sourceDigest(sha),bootstrap_sha256:harness};
  const file=path.join(directory,'context.json'); await fs.writeFile(file,JSON.stringify(context));
  const e={GITHUB_REPOSITORY:context.repository,GITHUB_SHA:sha,GITHUB_RUN_ID:'12',GITHUB_RUN_ATTEMPT:'3',
    CANDIDATE_RUN_ID:'10',CANDIDATE_RUN_ATTEMPT:'2',CONTEXT:file,ROLE:'linux-amd64',TOOLS_ROOT:directory};
  await assert.rejects(verifyContext(e),/context_pins/);
  context.tooling_manifests={'linux-amd64':harness}; await fs.writeFile(file,JSON.stringify(context));
  await assert.rejects(verifyContext({...e,CANDIDATE_RUN_ATTEMPT:'1'}),/context_pins/);
});
test('gather writes exact twelve role-prefixed records and refuses a missing artifact', async t => {
  const directory=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-gather-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  const context={schema:1,repository:'Epiphytic/marmish',source_commit:source,run_id:12,run_attempt:3,cohort:'matrix'};
  const file=path.join(directory,'context.json');await fs.writeFile(file,JSON.stringify(context));
  const e={GITHUB_REPOSITORY:context.repository,GITHUB_SHA:source,GITHUB_RUN_ID:'12',GITHUB_RUN_ATTEMPT:'3',
    CONTEXT:file,RECORDS_ROOT:path.join(directory,'records')};
  const requested=[];
  const transport={list:async()=>[],download:async(_entries,name,basename)=>{
    requested.push({name,basename});return Buffer.from('{}');}};
  await gather(e,transport);
  const names=ROLES.flatMap(role=>['identity-ready','server-ready','client-result','server-result'].map(k=>`${role}-${k}.json`));
  assert.deepEqual((await fs.readdir(e.RECORDS_ROOT)).sort(),names.sort());
  assert.equal(requested.length,12);
  assert.ok(requested.every(r=>r.name.startsWith('v02-installed-12-3-aaaaaaaaaaaa-matrix-') && !r.basename.includes('linux')));
  await assert.rejects(gather({...e,RECORDS_ROOT:path.join(directory,'missing')},
    {list:async()=>[],download:async()=>null}),/record_missing/);
});
