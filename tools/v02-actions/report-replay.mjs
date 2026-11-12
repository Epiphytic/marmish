// One fixed historical report replay, never a product execution or release gate.
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {ArtifactTransport, ROLES, artifactName, binding, boundedFile, childEnvironment, digest, requireThat as need} from './transport.mjs';
import {checkRun, sourceDigest, verifyContext, gather} from './workflow.mjs';
import {checkJobs, expectedArtifacts} from './release-candidate.mjs';
import {workflowDiagnostic} from './workflow-diagnostic.mjs';
export const ORIGINAL = Object.freeze({repository:'Epiphytic/marmish',run:'42885236657',attempt:'1',
  source:'c1d0c0376ca0662ea10e4b6d02acc777f6303328',candidate:'42747078282',candidateAttempt:'1',
  context:'60a26abf406849dfb6223beeacbd87f50dad05d068eb381ecb51396c52ac21aa'});
export const ORIGINAL_JOBS = Object.freeze([
  [130071705822,'resolve','success'],
  [130071963248,'matrix (ubuntu-24.04, linux-amd64, x86_64-unknown-linux-gnu)','success'],
  [130071963269,'matrix (ubuntu-24.04-arm, linux-arm64, aarch64-unknown-linux-gnu)','success'],
  [130071963360,'matrix (macos-15, macos-arm64, aarch64-apple-darwin)','success'],
  [130073932027,'rollup','failure']]);
export const REPLAY_PATHS = Object.freeze([
  ...['workflow','transport','supervisor-diagnostic','workflow-diagnostic','report-replay'].flatMap(n=>
    [`tools/v02-actions/${n}.mjs`,`tools/v02-actions/${n}.test.mjs`]),
  '.github/workflows/streaming-installed.yml','.github/workflows/v02-installed.yml',
  '.github/workflows/installed-report-replay.yml','docs/phases/ci-report-gather.md',
  '.github/workflows/installer-validation.yml','.github/workflows/installed-coordinator.yml',
  'tools/v02-actions/coordinator-policy.test.mjs',
  'docs/reviews/ci-report-gather.md','docs/validation/ci-report-gather.md','docs/agent-spec.md']);
const hex = (s,n)=>typeof s==='string' && new RegExp(`^[0-9a-f]{${n}}$`).test(s);
const gitCommand = args=>execFileSync('git',args,{encoding:'utf8',env:childEnvironment(process.env),maxBuffer:2*1024*1024});
export function checkCoordinator(env, git=gitCommand) {
  need(env.GITHUB_REPOSITORY===ORIGINAL.repository && env.GITHUB_REF==='refs/heads/master'
    && env.GITHUB_EVENT_NAME==='workflow_dispatch' && hex(env.GITHUB_SHA,40)
    && /^[1-9][0-9]{0,15}$/.test(env.GITHUB_RUN_ID??'')
    && /^[1-9][0-9]{0,15}$/.test(env.GITHUB_RUN_ATTEMPT??''),'replay_coordinator');
  need(git(['rev-parse','HEAD']).trim()===env.GITHUB_SHA,'replay_head');
  git(['merge-base','--is-ancestor',ORIGINAL.source,'HEAD']);
  git(['merge-base','--is-ancestor','HEAD','refs/remotes/origin/master']);
  const fields=git(['diff','--no-renames','--name-status','-z',ORIGINAL.source,'HEAD']).split('\0');
  need(fields.pop()==='' && fields.length>0 && fields.length%2===0,'replay_delta');
  for(let i=0;i<fields.length;i+=2) need(['A','M'].includes(fields[i]) && REPLAY_PATHS.includes(fields[i+1])
    && git(['ls-tree','HEAD','--',fields[i+1]]).startsWith('100644 blob '),'replay_delta');
}
function runIdentity(run,id,attempt,workflow,conclusion) {
  need(String(run.id)===id && String(run.run_attempt)===attempt && run.status==='completed'
    && run.conclusion===conclusion && run.head_sha===ORIGINAL.source && run.head_branch==='master'
    && run.event==='workflow_dispatch' && run.path===workflow
    && run.repository?.full_name===ORIGINAL.repository && run.head_repository?.full_name===ORIGINAL.repository,'replay_run');
}
export function checkOriginal(run,latest,jobs) {
  for(const r of [run,latest]) runIdentity(r,ORIGINAL.run,ORIGINAL.attempt,'.github/workflows/streaming-installed.yml','failure');
  need(jobs.length===5 && new Set(jobs.map(j=>j.id)).size===5,'replay_jobs');
  for(const [id,name,conclusion] of ORIGINAL_JOBS) need(jobs.some(j=>j.id===id && j.name===name
    && j.status==='completed' && j.conclusion===conclusion && String(j.run_id)===ORIGINAL.run
    && String(j.run_attempt)===ORIGINAL.attempt && j.head_sha===ORIGINAL.source),'replay_jobs');
}
export function checkArtifacts(entries,names,run) {
  need(Array.isArray(entries) && entries.length===names.length && new Set(entries.map(a=>a.id)).size===names.length,'replay_artifacts');
  for(const name of names) {
    const selected=entries.filter(a=>a.name===name);
    need(selected.length===1,'replay_artifacts');
    const a=selected[0];
    need(!a.expired && Number.isSafeInteger(a.id) && a.id>0 && /^sha256:[0-9a-f]{64}$/.test(a.digest)
      && Number.isSafeInteger(a.size_in_bytes) && a.size_in_bytes>0
      && String(a.workflow_run?.id)===run && a.workflow_run?.head_sha===ORIGINAL.source
      && a.workflow_run?.head_branch==='master','replay_artifacts');
  }
}
export function originalEnvironment(env) {
  return {...env,GITHUB_SHA:ORIGINAL.source,GITHUB_RUN_ID:ORIGINAL.run,GITHUB_RUN_ATTEMPT:ORIGINAL.attempt,
    SOURCE_COMMIT:ORIGINAL.source,CANDIDATE_RUN_ID:ORIGINAL.candidate,CANDIDATE_RUN_ATTEMPT:ORIGINAL.candidateAttempt,
    TEST_COHORT:'stream-matrix',ROLE:'linux-amd64'};
}
export function checkContext(bytes, fingerprint=sourceDigest) {
  need(digest(bytes)===ORIGINAL.context,'replay_context_digest');
  const c=JSON.parse(bytes);
  need(c.repository===ORIGINAL.repository && c.source_commit===ORIGINAL.source && String(c.run_id)===ORIGINAL.run
    && String(c.run_attempt)===ORIGINAL.attempt && String(c.candidate_run_id)===ORIGINAL.candidate
    && String(c.candidate_run_attempt)===ORIGINAL.candidateAttempt && c.cohort==='stream-matrix'
    && c.harness_sha256===fingerprint(ORIGINAL.source),'replay_context');
  return c;
}
function recordNames(context) {
  return [`v02-installed-context-${ORIGINAL.run}-${ORIGINAL.attempt}`, ...ROLES.flatMap(role=>
    ['identity-ready','server-ready','client-result','server-result'].map(kind=>artifactName(binding(context,role),role,kind)))];
}
const artifactProof = entries=>entries.map(a=>({id:a.id,name:a.name,digest:a.digest})).sort((a,b)=>a.id-b.id);
async function api(transport,suffix,deadline) {
  const r=await transport.request(`https://api.github.com/repos/${ORIGINAL.repository}/actions/runs/${suffix}`,true,2*1024*1024,deadline);
  need(r.bytes,'replay_api');return JSON.parse(r.bytes);
}
async function jobs(transport,id,deadline) {
  const r=await api(transport,`${id}/attempts/1/jobs?per_page=100`,deadline);
  need(Array.isArray(r.jobs) && r.total_count===r.jobs.length && r.total_count<=100,'replay_jobs');return r.jobs;
}
export async function guard(env, deps={}) {
  (deps.coordinator??checkCoordinator)(env);
  const t=deps.transport??new ArtifactTransport({context:{repository:ORIGINAL.repository,run_id:ORIGINAL.run},token:env.GH_TOKEN,
    budgetFile:path.join(env.RESOLVE_ROOT,'api-budget.json')});
  const deadline=Date.now()+120000;
  checkOriginal(await api(t,`${ORIGINAL.run}/attempts/1`,deadline),await api(t,ORIGINAL.run,deadline),await jobs(t,ORIGINAL.run,deadline));
  for(const suffix of [`${ORIGINAL.candidate}/attempts/1`,ORIGINAL.candidate]) {
    const r=await api(t,suffix,deadline);
    checkRun(r,{repository:ORIGINAL.repository,run_id:ORIGINAL.candidate,run_attempt:'1',source:ORIGINAL.source});
    runIdentity(r,ORIGINAL.candidate,'1','.github/workflows/v02-release.yml','success');
  }
  const candidateJobs=await jobs(t,ORIGINAL.candidate,deadline);checkJobs(candidateJobs);
  need(new Set(candidateJobs.map(j=>j.id)).size===11 && candidateJobs.every(j=>Number.isSafeInteger(j.id)
    && String(j.run_id)===ORIGINAL.candidate && String(j.run_attempt)==='1' && j.head_sha===ORIGINAL.source),'replay_candidate_jobs');
  const candidates=await api(t,`${ORIGINAL.candidate}/artifacts?per_page=100`,deadline);
  need(candidates.total_count===15,'replay_artifacts');checkArtifacts(candidates.artifacts,expectedArtifacts(ORIGINAL.candidate,'1'),ORIGINAL.candidate);
  const artifacts=await t.list(deadline);
  const name=`v02-installed-context-${ORIGINAL.run}-1`;
  // Context is the only record downloaded before its fixed digest is authenticated.
  const bytes=await t.download(artifacts,name,'context.json',deadline);
  need(bytes,'replay_context_missing');const context=checkContext(bytes,deps.fingerprint);
  checkArtifacts(artifacts,recordNames(context),ORIGINAL.run);
  await fs.writeFile(env.CONTEXT,bytes,{flag:'wx',mode:0o600});
  const proof={schema:1,status:'guard_pass',scope:'original-report-replay-only',original_source:ORIGINAL.source,
    original_run:ORIGINAL.run,original_attempt:'1',candidate:ORIGINAL.candidate,candidate_attempt:'1',
    context_sha256:ORIGINAL.context,bootstrap_sha256:context.bootstrap_sha256,tooling_manifests:context.tooling_manifests,coordinator_source:env.GITHUB_SHA,replay_run:env.GITHUB_RUN_ID,replay_attempt:env.GITHUB_RUN_ATTEMPT,
    original_jobs:ORIGINAL_JOBS,artifacts:artifactProof(artifacts),candidate_artifacts:artifactProof(candidates.artifacts)};
  await fs.writeFile(path.join(env.REPLAY_OUTPUT,'provenance.json'),JSON.stringify(proof,null,2)+'\n',{flag:'wx',mode:0o600});
  return proof;
}
export async function render(env,deps={}) {
  (deps.coordinator??checkCoordinator)(env);
  const file=path.join(env.REPLAY_OUTPUT,'provenance.json');
  const proof=JSON.parse(await boundedFile(file,65536));
  need(proof.status==='guard_pass' && proof.original_source===ORIGINAL.source && proof.original_run===ORIGINAL.run
    && proof.original_attempt==='1' && proof.context_sha256===ORIGINAL.context && proof.candidate===ORIGINAL.candidate
    && proof.candidate_attempt==='1' && proof.coordinator_source===env.GITHUB_SHA && proof.replay_run===env.GITHUB_RUN_ID
    && proof.replay_attempt===env.GITHUB_RUN_ATTEMPT,'replay_guard');
  checkContext(await boundedFile(env.CONTEXT,65536),deps.fingerprint);
  const original=originalEnvironment(env);
  try {
    await (deps.verify??verifyContext)(original);
    const t=deps.transport??new ArtifactTransport({context:JSON.parse(await boundedFile(env.CONTEXT,65536)),token:env.GH_TOKEN,
      budgetFile:path.join(env.RESOLVE_ROOT,'api-budget.json')});
    const list=t.list.bind(t);
    t.list=async (...args)=>{
      const entries=await list(...args);checkArtifacts(entries,recordNames(JSON.parse(await boundedFile(env.CONTEXT,65536))),ORIGINAL.run);
      need(JSON.stringify(artifactProof(entries))===JSON.stringify(proof.artifacts),'replay_artifact_changed');return entries;
    };
    await (deps.gather??gather)(original,t);
    const report=path.join(env.REPLAY_OUTPUT,'report');await fs.mkdir(report,{mode:0o700});
    (deps.execute??execFileSync)(path.join(env.TOOLS_ROOT,'marmish-installed-check'),['rollup','--context',env.CONTEXT,
      '--records',env.RECORDS_ROOT,'--matrix-result','success','--output',report],
    {env:childEnvironment(env),timeout:120000,killSignal:'SIGKILL',maxBuffer:65536,stdio:['ignore','pipe','pipe']});
    proof.status='PASS';
  } catch(error) {proof.status='FAIL';throw error;}
  finally {await fs.writeFile(file,JSON.stringify(proof,null,2)+'\n',{mode:0o600});}
}
if(process.argv[1] && import.meta.url===pathToFileURL(process.argv[1]).href) {
  const mode=process.argv[2];
  Promise.resolve().then(()=>{need(['guard','render'].includes(mode),'replay_mode');return mode==='guard'?guard(process.env):render(process.env);})
    .catch(async error=>{
      workflowDiagnostic('report-replay',error);process.exitCode=1;
      try {await fs.writeFile(path.join(process.env.REPLAY_OUTPUT,'provenance.json'),JSON.stringify({schema:1,status:'FAIL',
        scope:'original-report-replay-only',original_source:ORIGINAL.source,original_run:ORIGINAL.run,original_attempt:'1',
        coordinator_source:hex(process.env.GITHUB_SHA,40)?process.env.GITHUB_SHA:null})+'\n',{flag:'wx',mode:0o600});}
      catch { /* Preserve the original refusal and any existing detailed provenance. */ }
    });
}
