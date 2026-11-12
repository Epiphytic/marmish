import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import {ORIGINAL, ORIGINAL_JOBS, checkCoordinator, checkOriginal, checkContext, checkArtifacts, originalEnvironment, guard, render} from './report-replay.mjs';
import {expectedArtifacts} from './release-candidate.mjs';
import {ROLES, artifactName, binding} from './transport.mjs';
const contextBytes=Buffer.from("{\"bootstrap_sha256\":\"c3a3e1b8d776d225df375994b301fff231324050605d9a26477abd0e75151e0e\",\"candidate_run_attempt\":1,\"candidate_run_id\":42747078282,\"cohort\":\"stream-matrix\",\"created_at\":1794338795,\"expires_at\":1794342095,\"harness_sha256\":\"0735e97b1dba8921336cb33ef93f7632f445c225bfce3b3f22db80b3cdd01a2c\",\"manifests\":{\"linux-amd64\":\"af18f739b5c0ec8db1a74b5e12b24a8c1923d90974defe4e9c970fa50577a9f3\",\"linux-arm64\":\"809078729173d0d8fa79eb271036c61345a2676d1fa464784612d3d276a25670\",\"macos-arm64\":\"c03f3570c2f88d9536cc836126f05c27a37a84624782d50c6bdee8cc907120b9\"},\"nonce\":\"6bbffdb21f46868fbc781f768618348813047823af80d07828d2ac20f0a6226e\",\"relays\":[\"wss://relay.us.whitenoise.chat/\"],\"repository\":\"Epiphytic/marmish\",\"roles\":[\"linux-amd64\",\"linux-arm64\",\"macos-arm64\"],\"run_attempt\":1,\"run_id\":42885236657,\"schema\":1,\"source_commit\":\"c1d0c0376ca0662ea10e4b6d02acc777f6303328\",\"tooling\":[{\"files\":{\"marmish-installed-check\":{\"bytes\":23367816,\"sha256\":\"edb0b4fbd9748ddb55e7ff4a93c8fac08a62205404721a3638899c66c45b525c\"},\"marmish-installed-fixture\":{\"bytes\":12975072,\"sha256\":\"a893f4ca33d402e4585d51ebd3d64d575384769d7a715c721d26b5698d268aaf\"}},\"harness_sha256\":\"0735e97b1dba8921336cb33ef93f7632f445c225bfce3b3f22db80b3cdd01a2c\",\"schema\":1,\"source_commit\":\"c1d0c0376ca0662ea10e4b6d02acc777f6303328\",\"target\":\"x86_64-unknown-linux-gnu\"},{\"files\":{\"marmish-installed-check\":{\"bytes\":18201864,\"sha256\":\"63cc9620ca6520aa5b73242993bcc885c6a1f01423fd3cd164f9681bcc421bcd\"},\"marmish-installed-fixture\":{\"bytes\":8432616,\"sha256\":\"1957091be0db8d54487289a6059a9804d098de262eba867a215d84303ff701c9\"}},\"harness_sha256\":\"0735e97b1dba8921336cb33ef93f7632f445c225bfce3b3f22db80b3cdd01a2c\",\"schema\":1,\"source_commit\":\"c1d0c0376ca0662ea10e4b6d02acc777f6303328\",\"target\":\"aarch64-unknown-linux-gnu\"},{\"files\":{\"marmish-installed-check\":{\"bytes\":15543472,\"sha256\":\"6899625eb46d024749dd2b475754e77fe023bc0a1fa2edf01be185e2d01244fa\"},\"marmish-installed-fixture\":{\"bytes\":6546528,\"sha256\":\"f683968d7d13d2749ef968f849b28c03e987a55295cd91a5a036fead0170c153\"}},\"harness_sha256\":\"0735e97b1dba8921336cb33ef93f7632f445c225bfce3b3f22db80b3cdd01a2c\",\"schema\":1,\"source_commit\":\"c1d0c0376ca0662ea10e4b6d02acc777f6303328\",\"target\":\"aarch64-apple-darwin\"}],\"tooling_manifests\":{\"linux-amd64\":\"f352de9cc3ba117466499e0900fd95f47a40fe0d87c771ac5a98d69493625be9\",\"linux-arm64\":\"41c7c2db45dedbc40672be92fd2c6872717f6feb4a29587fea1fee2aaf51d1fc\",\"macos-arm64\":\"693f97be24ec1408befda3c742a44668da9c12755b2f017fa66401c733461579\"},\"version\":\"0.3.1\"}\n");
const context=JSON.parse(contextBytes);
const fingerprint=()=>context.harness_sha256;
const coordinator='a'.repeat(40);
const base={GITHUB_REPOSITORY:ORIGINAL.repository,GITHUB_REF:'refs/heads/master',GITHUB_EVENT_NAME:'workflow_dispatch',GITHUB_SHA:coordinator,GITHUB_RUN_ID:'99',GITHUB_RUN_ATTEMPT:'1'};
function run(id,conclusion,workflow) {return {id:Number(id),run_attempt:1,status:'completed',conclusion,head_sha:ORIGINAL.source,
  head_branch:'master',event:'workflow_dispatch',path:workflow,repository:{full_name:ORIGINAL.repository},head_repository:{full_name:ORIGINAL.repository}};}
const original=run(ORIGINAL.run,'failure','.github/workflows/streaming-installed.yml');
const candidate=run(ORIGINAL.candidate,'success','.github/workflows/v02-release.yml');
const originalJobs=ORIGINAL_JOBS.map(([id,name,conclusion])=>({id,name,conclusion,status:'completed',head_sha:ORIGINAL.source,run_id:Number(ORIGINAL.run),run_attempt:1}));
const targets=['x86_64-unknown-linux-gnu','aarch64-unknown-linux-gnu','aarch64-apple-darwin'];
const runners=['ubuntu-24.04','ubuntu-24.04-arm','macos-15'];
const candidateNames=['release-eligibility','render',...targets.flatMap((t,i)=>[
  `package (${runners[i]}, ${t}, ${i===2?'adhoc, macos=15.0':'none, glibc=2.39'})`,
  `install-quickstart (${runners[i]}, ${t})`,`provider-qualification (${runners[i]}, ${t})`])];
const candidateJobs=candidateNames.map((name,i)=>({id:i+1,name,status:'completed',conclusion:'success',head_sha:ORIGINAL.source,run_id:Number(ORIGINAL.candidate),run_attempt:1}));
const names=[`v02-installed-context-${ORIGINAL.run}-1`,...ROLES.flatMap(r=>['identity-ready','server-ready','client-result','server-result'].map(k=>artifactName(binding(context,r),r,k)))];
function artifacts(names,run) {return names.map((name,i)=>({id:i+1,name,digest:'sha256:'+'b'.repeat(64),expired:false,size_in_bytes:100,
 workflow_run:{id:Number(run),head_sha:ORIGINAL.source,head_branch:'master'}}));}
const records=artifacts(names,ORIGINAL.run);
function transport(change={}) {
 const replies={
  [`${ORIGINAL.run}/attempts/1`]:original,[ORIGINAL.run]:original,
  [`${ORIGINAL.run}/attempts/1/jobs?per_page=100`]:{total_count:5,jobs:originalJobs},
  [`${ORIGINAL.candidate}/attempts/1`]:candidate,[ORIGINAL.candidate]:candidate,
  [`${ORIGINAL.candidate}/attempts/1/jobs?per_page=100`]:{total_count:11,jobs:candidateJobs},
  [`${ORIGINAL.candidate}/artifacts?per_page=100`]:{total_count:15,artifacts:artifacts(expectedArtifacts(ORIGINAL.candidate,'1'),ORIGINAL.candidate)},...change};
 return {request:async url=>({bytes:Buffer.from(JSON.stringify(replies[url.split('/actions/runs/')[1]]))}),
  list:async()=>structuredClone(records),download:async()=>contextBytes};
}
async function fixture(t) {const root=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-replay-'));t.after(()=>fs.rm(root,{recursive:true,force:true}));
 const env={...base,RESOLVE_ROOT:root,CONTEXT:path.join(root,'context.json'),REPLAY_OUTPUT:path.join(root,'output'),TOOLS_ROOT:path.join(root,'tools'),RECORDS_ROOT:path.join(root,'records'),BOOTSTRAP_ROOT:path.join(root,'bootstrap')};
 await fs.mkdir(env.REPLAY_OUTPUT);return env;}
const deps={coordinator:()=>{},fingerprint};
test('fixed retained Context digest and original environment preserve current provenance',()=>{
 assert.deepEqual(checkContext(contextBytes,fingerprint),context);
 assert.throws(()=>checkContext(Buffer.from(contextBytes.toString()+' '),fingerprint),/context_digest/);
 assert.throws(()=>checkContext(contextBytes,()=> 'c'.repeat(64)),/replay_context/);
 const scoped=originalEnvironment(base);assert.equal(scoped.GITHUB_SHA,ORIGINAL.source);assert.equal(scoped.GITHUB_RUN_ID,ORIGINAL.run);
 assert.equal(base.GITHUB_SHA,coordinator);assert.equal(base.GITHUB_RUN_ID,'99');
});
test('source delta admits only exact regular allowlist entries',()=>{
 const git=args=>args[0]==='rev-parse'?coordinator:args[0]==='diff'?'M\0tools/v02-actions/workflow.mjs\0':args[0]==='ls-tree'?'100644 blob abc\tfile':'';
 checkCoordinator(base,git);
 for(const delta of ['D\0tools/v02-actions/workflow.mjs\0','M\0Cargo.lock\0','M\0scripts/install.sh\0','A\0elsewhere\0',''])
  assert.throws(()=>checkCoordinator(base,args=>args[0]==='diff'?delta:git(args)));
 assert.throws(()=>checkCoordinator(base,args=>args[0]==='ls-tree'?'120000 blob abc':git(args)));
 for(const change of [{GITHUB_REF:'refs/heads/other'},{GITHUB_REPOSITORY:'fork/marmish'},{GITHUB_EVENT_NAME:'push'},{GITHUB_RUN_ID:'0'}])assert.throws(()=>checkCoordinator({...base,...change},git));
});
test('exact original run and five job gates reject metadata drift',()=>{
 checkOriginal(original,original,originalJobs);
 for(const change of [{head_sha:coordinator},{run_attempt:2},{id:1},{status:'in_progress'},{conclusion:'success'},{path:'other'},{event:'push'},{repository:{full_name:'fork/marmish'}},{head_branch:'other'}])
  assert.throws(()=>checkOriginal(original,{...original,...change},originalJobs));
 for(const change of [{id:1},{name:'matrix'},{conclusion:'failure'},{run_id:1},{run_attempt:2},{head_sha:coordinator}]) {
  const jobs=structuredClone(originalJobs);Object.assign(jobs[1],change);assert.throws(()=>checkOriginal(original,original,jobs));
 }
 for(const jobs of [originalJobs.slice(1),[...originalJobs,originalJobs[0]],originalJobs.map(()=>originalJobs[0])])assert.throws(()=>checkOriginal(original,original,jobs));
});
test('exact original artifact set rejects missing duplicate foreign and malformed entries',()=>{
 checkArtifacts(records,names,ORIGINAL.run);
 for(const change of [{id:0},{expired:true},{digest:'wrong'},{size_in_bytes:0},{name:'foreign'},{workflow_run:{id:1,head_sha:ORIGINAL.source,head_branch:'master'}}]) {
  const entries=structuredClone(records);Object.assign(entries[0],change);assert.throws(()=>checkArtifacts(entries,names,ORIGINAL.run));
 }
 assert.throws(()=>checkArtifacts(records.slice(1),names,ORIGINAL.run));
 assert.throws(()=>checkArtifacts([...records,records[0]],names,ORIGINAL.run));
});
test('guard authenticates exact candidate eleven jobs and stores only pinned original context',async t=>{
 const env=await fixture(t);const proof=await guard(env,{...deps,transport:transport()});
 assert.equal(proof.status,'guard_pass');assert.equal(proof.artifacts.length,13);assert.equal(proof.candidate_artifacts.length,15);
 assert.equal(proof.coordinator_source,coordinator);assert.equal(proof.original_source,ORIGINAL.source);
 assert.deepEqual(await fs.readFile(env.CONTEXT),contextBytes);
});
test('candidate missing/failing/foreign jobs or changed latest attempt fail before context download',async t=>{
 for(const change of [
  {[ORIGINAL.candidate]:{...candidate,run_attempt:2}},
  {[`${ORIGINAL.candidate}/attempts/1/jobs?per_page=100`]:{total_count:10,jobs:candidateJobs.slice(1)}},
  {[`${ORIGINAL.candidate}/attempts/1/jobs?per_page=100`]:{total_count:11,jobs:candidateJobs.map((j,i)=>i===0?{...j,conclusion:'failure'}:j)}},
  {[`${ORIGINAL.candidate}/attempts/1/jobs?per_page=100`]:{total_count:11,jobs:candidateJobs.map((j,i)=>i===0?{...j,head_sha:coordinator}:j)}}]) {
  const env=await fixture(t);const tx=transport(change);let downloads=0;tx.download=()=>{downloads++;throw Error('must not download');};
  await assert.rejects(guard(env,{...deps,transport:tx}));assert.equal(downloads,0);await assert.rejects(fs.stat(env.CONTEXT));
 }
});
test('render verifies before gathering; immutable artifact changes stop native execution',async t=>{
 const env=await fixture(t);await guard(env,{...deps,transport:transport()});const order=[];
 const tx=transport();tx.list=async()=>records.map((r,i)=>i===0?{...r,id:999}:r);
 await assert.rejects(render(env,{...deps,transport:tx,verify:async()=>order.push('verify'),gather:async(e,t)=>{await t.list();},execute:()=>order.push('execute')}),/artifact_changed/);
 assert.deepEqual(order,['verify']);assert.equal(JSON.parse(await fs.readFile(path.join(env.REPLAY_OUTPUT,'provenance.json'))).status,'FAIL');
});
test('successful inert render consumes all twelve original records and exact native rollup argv',async t=>{
 const env=await fixture(t);await guard(env,{...deps,transport:transport()});const order=[];
 await render(env,{...deps,transport:transport(),verify:async e=>{order.push('verify');assert.equal(e.GITHUB_SHA,ORIGINAL.source);},
  gather:async(e,t)=>{order.push('gather');await t.list();await fs.mkdir(e.RECORDS_ROOT);for(const name of names.slice(1))await fs.writeFile(path.join(e.RECORDS_ROOT,name+'.json'),'{}');},
  execute:(file,args,options)=>{order.push('execute');assert.equal(file,path.join(env.TOOLS_ROOT,'marmish-installed-check'));
   assert.deepEqual(args,['rollup','--context',env.CONTEXT,'--records',env.RECORDS_ROOT,'--matrix-result','success','--output',path.join(env.REPLAY_OUTPUT,'report')]);
   assert.equal(options.timeout,120000);assert.equal(options.env.GH_TOKEN,undefined);}});
 assert.deepEqual(order,['verify','gather','execute']);assert.equal((await fs.readdir(env.RECORDS_ROOT)).length,12);
 assert.equal((await fs.readdir(path.join(env.REPLAY_OUTPUT,'report'))).length,0,'native gets empty output, distinct from provenance');
 assert.equal(JSON.parse(await fs.readFile(path.join(env.REPLAY_OUTPUT,'provenance.json'))).status,'PASS');
});
test('invalid or foreign saved guard refuses even probing',async t=>{
 const env=await fixture(t);await guard(env,{...deps,transport:transport()});const file=path.join(env.REPLAY_OUTPUT,'provenance.json');const saved=JSON.parse(await fs.readFile(file));
 for(const change of [{coordinator_source:ORIGINAL.source},{replay_run:'1'},{status:'PASS'},{context_sha256:'a'.repeat(64)}]) {
  await fs.writeFile(file,JSON.stringify({...saved,...change}));let probes=0;await assert.rejects(render(env,{...deps,verify:()=>probes++}));assert.equal(probes,0);
 }
});
test('workflow is manual bounded read-only replay with guarded downloads and full hosted unit suite',async()=>{
 const yaml=await fs.readFile(new URL('../../.github/workflows/installed-report-replay.yml',import.meta.url),'utf8');
 for(const required of ['workflow_dispatch:','timeout-minutes: 15','contents: read','actions: read','persist-credentials: false','fetch-depth: 0','npm ci --ignore-scripts','run: npm test','if: always()','report/matrix-rollup.*'])assert.ok(yaml.includes(required),required);
 assert.ok(yaml.indexOf('report-replay.mjs guard')<yaml.indexOf('actions/download-artifact@'));
 assert.ok(yaml.indexOf('actions/download-artifact@')<yaml.indexOf('report-replay.mjs render'));
 for(const forbidden of ['strategy:','matrix:','cargo ','--cohort','prepare --','mode: run','contents: write','actions: write','pull_request:'])assert.ok(!yaml.includes(forbidden),forbidden);
 assert.equal((yaml.match(/run-id: '42747078282'/g)??[]).length,2);
 assert.doesNotMatch(yaml.split('    steps:')[0],/runner\.temp/,'runner context is unavailable in job-level env');
 for(const name of ['RESOLVE_ROOT','CONTEXT','TOOLS_ROOT','BOOTSTRAP_ROOT','RECORDS_ROOT','REPLAY_OUTPUT'])
  assert.match(yaml,new RegExp(`echo "${name}=\\$RUNNER_TEMP/[^"\\n]+" >> "\\$GITHUB_ENV"`));
});
test('candidate artifact count/pins and original Context substitution refuse guard publication',async t=>{
 const key=`${ORIGINAL.candidate}/artifacts?per_page=100`;
 for(const tx of [transport({[key]:{total_count:14,artifacts:[]}}),{...transport(),download:async()=>Buffer.from('{}')},
  {...transport(),list:async()=>records.filter(a=>a.name!==names[1])}]) {
  const env=await fixture(t);await assert.rejects(guard(env,{...deps,transport:tx}));
  await assert.rejects(fs.stat(env.CONTEXT));await assert.rejects(fs.stat(path.join(env.REPLAY_OUTPUT,'provenance.json')));
 }
});
test('failed byte verification prevents gathering and native execution',async t=>{
 const env=await fixture(t);await guard(env,{...deps,transport:transport()});let gathered=0,executed=0;
 await assert.rejects(render(env,{...deps,verify:async()=>{throw Error('tool_digest');},gather:async()=>gathered++,execute:()=>executed++}),/tool_digest/);
 assert.equal(gathered,0);assert.equal(executed,0);
 assert.equal(JSON.parse(await fs.readFile(path.join(env.REPLAY_OUTPUT,'provenance.json'))).status,'FAIL');
});
