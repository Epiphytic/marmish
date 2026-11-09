import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {candidateInputs, checkRun, checkTooling, verifyTools, verifyContext, gather, sourceDigest, SOURCE_PATHS, TARGETS, cohortLifetime, PROVIDER_DIAGNOSTIC, DIAGNOSTIC_PATHS, checkDiagnosticJobs, checkDiagnosticChanges, validateCandidate, providerDiagnostic} from './workflow.mjs';
import {inspectCandidate} from './release-candidate.mjs';
import {INSTALLED_CANDIDATE_PATHS} from './workflow.mjs';
import {LOCAL_COHORTS} from './local.mjs';
import {ArtifactTransport, digest, ROLES} from './transport.mjs';
import {execFileSync} from 'node:child_process';
const source = 'a'.repeat(40), harness = 'b'.repeat(64);
const env = {CANDIDATE_RUN_ID:'12', CANDIDATE_RUN_ATTEMPT:'3', GITHUB_REPOSITORY:'Epiphytic/marmish',
  SOURCE_COMMIT:source, GITHUB_SHA:source, TEST_RELAY:'wss://relay.example'};
const run = {id:12,run_attempt:3,status:'completed',conclusion:'success',path:'.github/workflows/v02-release.yml',
  event:'workflow_dispatch',head_branch:'master',head_repository:{full_name:'Epiphytic/marmish'},
  repository:{full_name:'Epiphytic/marmish'},head_sha:'c'.repeat(40)};
test('closed cohort lifetimes preserve matrix and original local wave bounds', () => {
  assert.deepEqual(cohortLifetime({}), {name:'matrix',seconds:2220});
  for (const name of ['autoexit','uptime','benchmark','recovery','policy']) assert.deepEqual(cohortLifetime({TEST_COHORT:name}), {name,seconds:3300});
  assert.deepEqual(cohortLifetime({TEST_COHORT:'version-lifecycle'}), {name:'version-lifecycle',seconds:1800});
  for (const name of ['unknown','constructor','__proto__','']) assert.throws(()=>cohortLifetime({TEST_COHORT:name}));
});
test('candidate inputs bind control source and require explicit direct relay', () => {
  assert.equal(candidateInputs(env).relay,'wss://relay.example/');
  for (const change of [{CANDIDATE_RUN_ID:'../1'},{CANDIDATE_RUN_ATTEMPT:'0'},{GITHUB_SHA:'c'.repeat(40)},
    {GITHUB_REPOSITORY:'fork/marmish'},{TEST_RELAY:'ws://relay.example'},{TEST_RELAY:'wss://u:p@relay.example'},
    {TEST_RELAY:'wss://hidden.onion'},{TEST_RELAY:'wss://relay.example/#x'}]) {
    assert.throws(()=>candidateInputs({...env,...change}));
  }
});
test('exact candidate attempt and workflow required; dispatch head is not selected source', () => {
  const c=candidateInputs(env);
  for (const event of ['workflow_dispatch','push']) checkRun({...run,event},c);
  for (const event of ['pull_request','pull_request_target','workflow_run','release',undefined,null,'Push']) {
    assert.throws(()=>checkRun({...run,event},c));
  }
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
  const tree=execFileSync('git',['ls-tree','-r',sha,'--',...SOURCE_PATHS]);
  const expected=execFileSync('shasum',['-a','256'],{input:tree,encoding:'utf8'}).split(' ')[0];
  assert.equal(sourceDigest(sha),expected);
});
test('both candidate producers hash the exact source path set', async () => {
  for (const name of ['v02-release.yml','streaming-candidate.yml']) {
    const yaml=await fs.readFile(new URL(`../../.github/workflows/${name}`,import.meta.url),'utf8');
    const match=yaml.match(/git ls-tree -r "\$MARMISH_BUILD_COMMIT" -- ([\s\S]*?)\| shasum/);
    assert.ok(match,name);
    assert.deepEqual(match[1].replaceAll('\\','').trim().split(/\s+/),SOURCE_PATHS,name);
  }
});
test('source fingerprint closes native local path dependencies and build inputs', async () => {
  const visited=new Set();
  async function visit(directory) {
    if(visited.has(directory)) return;
    visited.add(directory);
    assert.ok(SOURCE_PATHS.includes(directory),directory);
    const manifest=await fs.readFile(new URL(`../../${directory}/Cargo.toml`,import.meta.url),'utf8');
    for(const match of manifest.matchAll(/path\s*=\s*"(\.\.[^"]+)"/g)) {
      await visit(path.posix.normalize(path.posix.join(directory,match[1])));
    }
  }
  await visit('crates/marmish-native');
  for(const name of ['engine','stream','core','vault']) assert.ok(visited.has(`crates/marmish-${name}`));
  for(const name of ['Cargo.toml','Cargo.lock','rust-toolchain.toml','scripts',
    '.github/workflows/streaming-candidate.yml','.github/workflows/streaming-installed.yml','tools/release-version-vectors.json']) {
    assert.ok(SOURCE_PATHS.includes(name),name);
  }
});
test('streaming candidates require exact reviewed workflow head and dispatch', () => {
  const c=candidateInputs(env);
  const streaming={...run,path:'.github/workflows/streaming-candidate.yml',head_sha:source};
  assert.throws(()=>checkRun(streaming,c),/invalid_candidate_provenance/);
  checkRun(streaming,c,undefined,INSTALLED_CANDIDATE_PATHS);
  for(const change of [{event:'push'},{head_sha:'c'.repeat(40)},
    {path:'.github/workflows/streaming-candidate.yml@master'},
    {path:'.github/workflows/streaming-installed.yml'}]) {
    assert.throws(()=>checkRun({...streaming,...change},c,undefined,INSTALLED_CANDIDATE_PATHS));
  }
});
test('release publication rejects streaming candidate before reading artifacts', async()=>{
  let calls=0;
  await assert.rejects(inspectCandidate({run_id:'12',run_attempt:'3',publisher_sha:'b'.repeat(40)}, {
    api:async()=>{calls++;return {...run,path:'.github/workflows/streaming-candidate.yml',head_sha:source};},
    git:()=>{throw new Error('must_not_read_git');},
    transport:{list:async()=>{throw new Error('must_not_read_artifacts');}},
  }),/invalid_candidate_provenance/);
  assert.equal(calls,1);
});
test('missing tooling pin or different candidate attempt fails before executing a tool', async t => {
  const directory=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-context-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  const sha=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
  const context={cohort:'matrix',repository:'Epiphytic/marmish',source_commit:sha,run_id:12,run_attempt:3,
    candidate_run_id:10,candidate_run_attempt:2,harness_sha256:sourceDigest(sha),bootstrap_sha256:harness};
  const file=path.join(directory,'context.json'); await fs.writeFile(file,JSON.stringify(context));
  const e={GITHUB_REPOSITORY:context.repository,GITHUB_SHA:sha,GITHUB_RUN_ID:'12',GITHUB_RUN_ATTEMPT:'3',
    CANDIDATE_RUN_ID:'10',CANDIDATE_RUN_ATTEMPT:'2',CONTEXT:file,ROLE:'linux-amd64',TOOLS_ROOT:directory};
  await assert.rejects(verifyContext(e),/context_pins/);
  await assert.rejects(verifyContext({...e,TEST_COHORT:'benchmark'}),/workflow_cohort/);
  context.tooling_manifests={'linux-amd64':harness}; await fs.writeFile(file,JSON.stringify(context));
  await assert.rejects(verifyContext({...e,CANDIDATE_RUN_ATTEMPT:'1'}),/context_pins/);
});
test('gather writes available exact role records and preserves missing records for FAIL rollup', async t => {
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
  await gather({...e,RECORDS_ROOT:path.join(directory,'missing')},
    {list:async()=>[],download:async()=>null});
  assert.deepEqual(await fs.readdir(path.join(directory,'missing')),[]);
});

// Source parity supplements typed Rust tests: every advertised wave must route.
test('workflow choices, JS routing and Rust routing advertise the same closed cohorts', async () => {
  const root = new URL('../../', import.meta.url);
  const read = name => fs.readFile(new URL(name, root), 'utf8');
  const expected = ['matrix', 'autoexit', 'uptime', 'benchmark', 'recovery', 'policy'];
  const yaml = await read('.github/workflows/v02-installed.yml');
  const options = yaml.match(/options: \[([^\]]+)\]/)[1].split(',').map(s => s.trim());
  assert.deepEqual(options, expected);
  assert.deepEqual(LOCAL_COHORTS, expected.slice(1));
  for (const name of expected) assert.equal(cohortLifetime({TEST_COHORT:name}).seconds,
    name === 'matrix' ? 2220 : 3300);
  const records = await read('crates/marmish-native/src/installed_records.rs');
  const lifetime = records.split('fn cohort_lifetime')[1].split('pub(super) fn validate')[0];
  const local = await read('crates/marmish-native/src/installed_local.rs');
  const dispatch = local.split('match cohort {')[1].split('#[derive(Serialize)]')[0];
  const rollup = local.split('pub(super) fn rollup')[1].split('let expected =')[0];
  for (const name of expected) assert.ok(lifetime.includes(`"${name}"`), name);
  for (const name of expected.slice(1)) {
    assert.ok(dispatch.includes(`"${name}"`), `dispatch ${name}`);
    assert.ok(rollup.includes(`"${name}"`), `rollup ${name}`);
  }
});

const diagnosticEnv={...env,CANDIDATE_RUN_ID:PROVIDER_DIAGNOSTIC.run,
  CANDIDATE_RUN_ATTEMPT:PROVIDER_DIAGNOSTIC.attempt,SOURCE_COMMIT:PROVIDER_DIAGNOSTIC.source};
const diagnosticRun={...run,id:Number(PROVIDER_DIAGNOSTIC.run),run_attempt:1,
  conclusion:'failure',head_sha:PROVIDER_DIAGNOSTIC.source};
const diagnosticJobs=[
  [126816261687,'package (ubuntu-24.04-arm, aarch64-unknown-linux-gnu, none, glibc=2.39)','success'],
  [126816262401,'package (macos-15, aarch64-apple-darwin, adhoc, macos=15.0)','success'],
  [126816262683,'package (ubuntu-24.04, x86_64-unknown-linux-gnu, none, glibc=2.39)','success'],
  [126848469757,'render','success'],
  [126848598035,'provider-qualification (macos-15, aarch64-apple-darwin)','failure']
].map(([id,name,conclusion])=>({id,name,conclusion,status:'completed'}));
function diagnosticTransport(change={}) {
  return {request:async url=>({bytes:Buffer.from(JSON.stringify(url.includes('/jobs?')
    ? (change.jobs??{total_count:5,jobs:diagnosticJobs}) : (change.run??diagnosticRun)))}),
    list:async()=>change.artifacts??[
      {name:`v02-candidate-aarch64-apple-darwin-${PROVIDER_DIAGNOSTIC.run}-1`,expired:false},
      {name:`v02-pinned-bootstrap-${PROVIDER_DIAGNOSTIC.run}-1`,expired:false}]};
}
test('diagnostic exception requires literal policy and exact old product, production stays strict',()=>{
  const c=candidateInputs(diagnosticEnv,PROVIDER_DIAGNOSTIC);
  checkRun(diagnosticRun,c,PROVIDER_DIAGNOSTIC);
  assert.throws(()=>candidateInputs(diagnosticEnv));
  assert.throws(()=>candidateInputs(diagnosticEnv,{...PROVIDER_DIAGNOSTIC}));
  assert.throws(()=>checkRun(diagnosticRun,c));
  for(const change of [{SOURCE_COMMIT:source},{CANDIDATE_RUN_ID:'41898839618'},
    {CANDIDATE_RUN_ATTEMPT:'2'},{GITHUB_REPOSITORY:'fork/marmish'}])
    assert.throws(()=>candidateInputs({...diagnosticEnv,...change},PROVIDER_DIAGNOSTIC));
  for(const change of [{head_sha:source},{conclusion:'success'},{run_attempt:2},
    {path:'.github/workflows/other.yml'},{event:'push'},{head_branch:'other'},
    {repository:{full_name:'fork/marmish'}},{head_repository:{full_name:'fork/marmish'}}])
    assert.throws(()=>checkRun({...diagnosticRun,...change},c,PROVIDER_DIAGNOSTIC));
});
test('diagnostic requires three exact packages, render, and original failed mac job',async()=>{
  assert.equal((await validateCandidate(diagnosticEnv,0,diagnosticTransport(),PROVIDER_DIAGNOSTIC)).length,2);
  for(let i=0;i<diagnosticJobs.length;i++) {
    for(const change of [{id:1},{name:'other'},{conclusion:'cancelled'},{status:'in_progress'}]) {
      const jobs=diagnosticJobs.map((job,n)=>n===i?{...job,...change}:job);
      assert.throws(()=>checkDiagnosticJobs(jobs));
    }
    assert.throws(()=>checkDiagnosticJobs(diagnosticJobs.filter((_,n)=>n!==i)));
  }
  assert.throws(()=>checkDiagnosticJobs([...diagnosticJobs,diagnosticJobs[0]]));
  for(const jobs of [{total_count:6,jobs:diagnosticJobs},{total_count:4,jobs:diagnosticJobs},
    {total_count:0,jobs:[]},{total_count:5,jobs:[...diagnosticJobs.slice(0,4),diagnosticJobs[0]]}])
    await assert.rejects(validateCandidate(diagnosticEnv,0,diagnosticTransport({jobs}),PROVIDER_DIAGNOSTIC));
  const artifacts=await diagnosticTransport().list();
  for(const bad of [[],[...artifacts,artifacts[0]],artifacts.map(a=>({...a,expired:true})),
    artifacts.map(a=>({...a,name:a.name.replace('-1','-2')}))])
    await assert.rejects(validateCandidate(diagnosticEnv,0,diagnosticTransport({artifacts:bad}),PROVIDER_DIAGNOSTIC));
});
test('diagnostic source audit permits only thirteen literal regular diagnostic paths',()=>{
  assert.equal(DIAGNOSTIC_PATHS.length,13);
  for(const name of DIAGNOSTIC_PATHS) for(const status of ['A','M']) checkDiagnosticChanges([[status,name,'100644']]);
  for(const name of ['tools/v02-actions/workflow.mjs','tools/v02-actions/workflow.test.mjs','tools/marmish_native_quickstart_check.py'])
    assert.ok(DIAGNOSTIC_PATHS.includes(name));
  assert.throws(()=>checkDiagnosticChanges([]));
  for(const entry of [['D',DIAGNOSTIC_PATHS[0],'100644'],['R100',DIAGNOSTIC_PATHS[0],'100644'],
    ['M','crates/marmish-native/src/identity.rs','100644'],['M','tools/marmish_native_quickstart_check.pyc','100644'],
    ['M',DIAGNOSTIC_PATHS[0],'120000'],['M',DIAGNOSTIC_PATHS[0],'100755'],
    ['D','tools/marmish_native_quickstart_check.py','100644'],
    ['R100','tools/marmish_native_quickstart_check.py','100644'],
    ['M','tools/marmish_native_quickstart_check.py','100755'],
    ['M','tools/marmish_native_quickstart_check.py','120000'],
    ['M','other/marmish_native_quickstart_check.py','100644'],
    ['M','docs/reviews/unreviewed.md','100644']]) assert.throws(()=>checkDiagnosticChanges([entry]));
});
test('diagnostic guard refusal preserves separate finite product and supervisor envelope',async t=>{
  const directory=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-diagnostic-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  await assert.rejects(providerDiagnostic({...env,DIAGNOSTIC_ROOT:directory,GITHUB_REF:'refs/heads/other',
    GITHUB_RUN_ID:'88',GITHUB_RUN_ATTEMPT:'1'}),/provider_diagnostic_refused/);
  const record=JSON.parse(await fs.readFile(path.join(directory,'provenance.json')));
  assert.equal(record.product_source_commit,PROVIDER_DIAGNOSTIC.source);
  assert.equal(record.supervisor_source_commit,source);
  assert.equal(record.guard,'metadata_refused');
  assert.equal(record.diagnostic,true);
  assert.ok(!JSON.stringify(record).includes('Error:'));
});


test('validateCandidate fails closed on a bounded list mismatch without retry or artifact admission', async t => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-candidate-list-'));
  t.after(() => fs.rm(directory, {recursive: true, force: true}));
  const calls = [];
  const transport = new ArtifactTransport({context: {repository: env.GITHUB_REPOSITORY, run_id: '12'},
    token: 'private-token', budgetFile: path.join(directory, 'budget'), fetcher: async url => {
      calls.push(String(url));
      if (String(url).endsWith('/attempts/3')) return new Response(JSON.stringify({...run, head_sha: source}));
      assert.ok(String(url).endsWith('/artifacts?per_page=100'));
      return new Response(JSON.stringify({total_count: 3, artifacts: [{}, {}]}));
    }});
  transport.download = async () => { assert.fail('incomplete inventory must never be admitted'); };
  await assert.rejects(validateCandidate(env, 0, transport), {message: 'artifact_retry'});
  assert.equal(calls.length, 2);
  assert.equal(calls.filter(url => url.endsWith('/artifacts?per_page=100')).length, 1);
  assert.equal(JSON.parse(await fs.readFile(path.join(directory, 'budget'))).used, 2);
});
