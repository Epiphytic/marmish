import test from 'node:test';
import assert from 'node:assert/strict';
import {evaluateEligibility, verifyEligibility, versionAt, compareVersions} from './eligibility.mjs';
const sha = n => n.toString(16).padStart(40,'0');
const input = {repository:'Epiphytic/marmish',source:sha(3),before:sha(1),event:'push',runId:'12',runAttempt:'1',caveated:false};
function fixtures({versions=['0.2.1','0.2.2','0.2.2'],chain=[sha(3),sha(2),sha(1)],ancestors=chain,
  shallow=false,tags=[],releases=[],verifyPublished}={}) {
  const git = async args => {
    if(args[0]==='show') {
      const [commit,file]=args[1].split(':');
      const version=versions[Number.parseInt(commit,16)-1];
      assert.ok(version, 'unknown fixture commit');
      return `${file==='Cargo.lock'?'[[package]]':'[package]'}\nname = "marmish-native"\nversion = "${version}"\n`;
    }
    if(args[0]==='rev-parse') return String(shallow);
    if(args[0]==='rev-list') return (args.includes('--first-parent')?chain:ancestors).join('\n');
    assert.fail(`unexpected git ${args}`);
  };
  const api=async path=>path.includes('/tags?')?tags:releases;
  return {git,api,verifyPublished};
}
test('numeric patch and rc precedence, bounded existing native grammar',()=>{
  for(const [a,b] of [['0.2.9','0.2.10'],['0.2.10-rc.2','0.2.10-rc.10'],['0.2.10-rc.10','0.2.10']]) {
    assert.equal(compareVersions(a,b),-1);assert.equal(compareVersions(b,a),1);
  }
  assert.equal(compareVersions('0.2.1','0.2.1'),0);
  for(const bad of ['0.1.1','0.2.01','0.2.1-rc.0','0.2.18446744073709551616','0.2.1-rc.18446744073709551616']) {
    assert.throws(()=>compareVersions(bad,'0.2.1'),/invalid_version/);
  }
});
test('manifest and native lock entry must agree',async()=>{
  assert.equal(await versionAt(sha(3),fixtures().git),'0.2.2');
  await assert.rejects(versionAt(sha(3),async args=>args[1].endsWith('Cargo.lock')
    ? '[[package]]\nname = "marmish-native"\nversion = "0.2.3"\n' : '[package]\nname = "marmish-native"\nversion = "0.2.2"\n'),/invalid_version/);
});
test('bump before tip, squash/merge first-parent transition and manual unpublished rebuild',async()=>{
  assert.equal((await evaluateEligibility(input,fixtures())).decision,'build');
  assert.equal((await evaluateEligibility(input,fixtures({chain:[sha(3),sha(1)]}))).decision,'build');
  assert.equal((await evaluateEligibility({...input,event:'workflow_dispatch',before:null},fixtures())).decision,'build');
});
test('unchanged skip and ancestry/transition failure contracts',async()=>{
  const cases=[
    [{},fixtures({versions:['0.2.2','0.2.2','0.2.2']}),'unchanged_version'],
    [{before:'0'.repeat(40)},fixtures(),'invalid_before'],
    [{forced:true},fixtures(),'invalid_before'],
    [{},fixtures({shallow:true}),'incomplete_history'],
    [{},fixtures({ancestors:[sha(3),sha(2)]}),'nonancestor_before'],
    [{},fixtures({chain:[sha(3),sha(2)],ancestors:[sha(3),sha(2),sha(1)]}),'before_not_first_parent'],
    [{},fixtures({versions:['0.2.0','0.2.1','0.2.2']}),'multiple_version_transitions'],
    [{},fixtures({versions:['0.2.3','0.2.2','0.2.2']}),'nonadvancing_version'],
  ];
  for(const [change,adapters,reason] of cases) assert.equal((await evaluateEligibility({...input,...change},adapters)).reason,reason);
});
test('unchanged version skips without treating historical tag as publication proof',async()=>{
  const adapters=fixtures({versions:['0.2.2','0.2.2','0.2.2'],tags:[{name:'v0.2.2',commit:{sha:sha(1)}}]});
  adapters.api=async()=>{assert.fail('unchanged version must not consult release state');};
  const report=await evaluateEligibility(input,adapters);
  assert.equal(report.reason,'unchanged_version');assert.equal(report.decision,'skip-by-design');
  assert.deepEqual(await verifyEligibility(report,adapters),report);
});

test('highest native release includes numeric rc order, ignores historical tags, rejects conflicting selected tag',async()=>{
  const tags=[{name:'v0.1.0-experimental.1'},{name:'v0.2.1',commit:{sha:sha(1)}}];
  assert.equal((await evaluateEligibility(input,fixtures({tags}))).decision,'build');
  const selected={name:'v0.2.2',commit:{sha:sha(3)}};
  assert.equal((await evaluateEligibility(input,fixtures({tags:[selected]}))).reason,'existing_version_conflict');
  const releases=[{tag_name:'v0.2.2',draft:false}];
  assert.equal((await evaluateEligibility(input,fixtures({tags:[selected],releases,verifyPublished:async()=>true}))).reason,'already_published');
  assert.equal((await evaluateEligibility(input,fixtures({tags:[selected],releases,verifyPublished:async()=>false}))).reason,'existing_version_conflict');
  assert.equal((await evaluateEligibility(input,fixtures({tags:[{name:'v0.2.10',commit:{sha:sha(1)}}]}))).reason,'nonadvancing_version');
});
test('complete bounded enumeration and matching native malformed states fail closed',async()=>{
  const adapters=fixtures();adapters.api=async()=>Array.from({length:100},(_,n)=>({name:`ignored-${n}`}));
  assert.equal((await evaluateEligibility(input,adapters)).reason,'incomplete_listing');
  for(const state of [{tags:[{name:'v0.2.01',commit:{sha:sha(1)}}]},
    {releases:[{tag_name:'v0.2.1',draft:false}]},
    {tags:[{name:'v0.2.1',commit:{sha:sha(1)}}],releases:[{tag_name:'v0.2.1',draft:true}]}]) {
    assert.ok(['invalid_version','inconsistent_release_state'].includes((await evaluateEligibility(input,fixtures(state))).reason));
  }
});
test('report recomputation is exact, current same-source selected tag can resume original build only explicitly',async()=>{
  const original=await evaluateEligibility(input,fixtures());
  assert.deepEqual(await verifyEligibility(original,fixtures()),original);
  await assert.rejects(verifyEligibility({...original,extra:true},fixtures()),/invalid_eligibility/);
  await assert.rejects(verifyEligibility({...original,decision:'skip-by-design',reason:'unchanged_version'},fixtures()),/recomputation/);
  const recovery=fixtures({tags:[{name:'v0.2.2',commit:{sha:sha(3)}}]});
  await assert.rejects(verifyEligibility(original,recovery),/recomputation/);
  assert.deepEqual(await verifyEligibility(original,{...recovery,allowSelectedTag:true}),original);
  const wrong=fixtures({tags:[{name:'v0.2.2',commit:{sha:sha(2)}}]});
  await assert.rejects(verifyEligibility(original,{...wrong,allowSelectedTag:true}),/recomputation/);
});
test('transport failures never become clean skip',async()=>{
  const adapters=fixtures();adapters.api=async()=>{throw new Error('network_down');};
  await assert.rejects(evaluateEligibility(input,adapters),/network_down/);
});

test('real git handles multi-commit mainline, merge topology and missing/side-branch before',async t=>{
  const {promises:fs}=await import('node:fs');
  const {tmpdir}=await import('node:os');
  const path=await import('node:path');
  const {execFile}=await import('node:child_process');
  const {promisify}=await import('node:util');
  const directory=await fs.mkdtemp(path.join(tmpdir(),'marmish-eligibility-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  const exec=promisify(execFile);
  const git=async argv=>(await exec('git',argv,{cwd:directory,encoding:'utf8',env:{...process.env,
    GIT_AUTHOR_NAME:'Test',GIT_AUTHOR_EMAIL:'test@example.invalid',GIT_COMMITTER_NAME:'Test',GIT_COMMITTER_EMAIL:'test@example.invalid'}})).stdout;
  await git(['init','-b','master']);
  await fs.mkdir(path.join(directory,'crates/marmish-native'),{recursive:true});
  async function commit(version,label) {
    await fs.writeFile(path.join(directory,'crates/marmish-native/Cargo.toml'),`[package]\nname = "marmish-native"\nversion = "${version}"\n`);
    await fs.writeFile(path.join(directory,'Cargo.lock'),`[[package]]\nname = "marmish-native"\nversion = "${version}"\n`);
    await fs.writeFile(path.join(directory,'marker'),label);
    await git(['add','.']);await git(['commit','-m',label]);
    return (await git(['rev-parse','HEAD'])).trim();
  }
  const before=await commit('0.2.1','initial');
  await commit('0.2.2','bump');
  const source=await commit('0.2.2','after bump');
  const adapters={git,api:async()=>[]};
  assert.equal((await evaluateEligibility({...input,before,source},adapters)).decision,'build');
  // The workflow dispatch field is caveated_release, not the report's caveated key.
  for (const caveated_release of [true,'true']) {
    const eventPath=path.join(directory,'dispatch.json'),reportPath=path.join(directory,'eligibility.json');
    await fs.writeFile(eventPath,JSON.stringify({inputs:{caveated_release}}));
    await exec(process.execPath,[new URL('./eligibility.mjs',import.meta.url).pathname],{
      cwd:directory,env:{...process.env,GITHUB_EVENT_PATH:eventPath,GITHUB_EVENT_NAME:'workflow_dispatch',
        GITHUB_REPOSITORY:input.repository,GITHUB_SHA:source,SOURCE_COMMIT:source,
        GITHUB_RUN_ID:'12',GITHUB_RUN_ATTEMPT:'1',ELIGIBILITY_REPORT:reportPath,GITHUB_OUTPUT:''}});
    const caveatedReport=JSON.parse(await fs.readFile(reportPath,'utf8'));
    assert.equal(caveatedReport.decision,'build');assert.equal(caveatedReport.reason,'eligible_version');assert.equal(caveatedReport.caveated,true);
  }
  await git(['checkout','-b','side',before]);
  await fs.writeFile(path.join(directory,'side'),'side');await git(['add','side']);await git(['commit','-m','side']);
  const side=(await git(['rev-parse','HEAD'])).trim();
  await git(['checkout','master']);await git(['merge','--no-ff','side','-m','merge side']);
  const merge=(await git(['rev-parse','HEAD'])).trim();
  assert.equal((await evaluateEligibility({...input,before,source:merge},adapters)).decision,'build');
  assert.equal((await evaluateEligibility({...input,before:side,source:merge},adapters)).reason,'before_not_first_parent');
  assert.equal((await evaluateEligibility({...input,before:sha(77),source:merge},adapters)).reason,'nonancestor_before');
});

test('CLI writes bounded canonical failure report and outputs before exiting nonzero',async t=>{
  const {promises:fs}=await import('node:fs');
  const {tmpdir}=await import('node:os');
  const path=await import('node:path');
  const {execFile}=await import('node:child_process');
  const {promisify}=await import('node:util');
  const directory=await fs.mkdtemp(path.join(tmpdir(),'marmish-eligibility-cli-'));
  t.after(()=>fs.rm(directory,{recursive:true,force:true}));
  const event=path.join(directory,'event.json'), report=path.join(directory,'report.json'), output=path.join(directory,'output');
  await fs.writeFile(event,JSON.stringify({before:sha(1)}));
  await assert.rejects(promisify(execFile)(process.execPath,[new URL('./eligibility.mjs',import.meta.url).pathname],{
    env:{...process.env,GITHUB_EVENT_PATH:event,GITHUB_EVENT_NAME:'push',GITHUB_REPOSITORY:'fork/marmish',
      GITHUB_SHA:sha(3),SOURCE_COMMIT:sha(3),GITHUB_RUN_ID:'12',GITHUB_RUN_ATTEMPT:'1',
      ELIGIBILITY_REPORT:report,GITHUB_OUTPUT:output,GH_TOKEN:'never-print-this-token'}}),error=>{
        assert.equal(error.code,1);assert.equal(error.stderr,'');return true;
      });
  const bytes=await fs.readFile(report,'utf8'), value=JSON.parse(bytes);
  assert.equal(bytes,JSON.stringify(value)+'\n');assert.equal(value.reason,'invalid_context');
  assert.equal(value.decision,'ineligible');
  assert.equal(await fs.readFile(output,'utf8'),`decision=ineligible\nversion=\nsource=${sha(3)}\nreason=invalid_context\n`);
});
test('caveated manual diagnostics build without release authority or release-state inspection',async()=>{
  const adapters=fixtures();adapters.api=async()=>assert.fail('diagnostic build must not inspect releases');
  const value=await evaluateEligibility({...input,event:'workflow_dispatch',before:null,caveated:true},adapters);
  assert.equal(value.decision,'build');assert.equal(value.caveated,true);assert.equal(value.reason,'eligible_version');
  assert.equal((await evaluateEligibility({...input,caveated:true},adapters)).reason,'invalid_context');
});
test('an existing selected version without full published-proof adapter fails closed',async()=>{
  const tags=[{name:'v0.2.2',commit:{sha:sha(3)}}],releases=[{tag_name:'v0.2.2',draft:false}];
  assert.equal((await evaluateEligibility(input,fixtures({tags,releases}))).reason,'existing_version_conflict');
});
test('intermediate version transitions require matching native manifest and lockfile',async()=>{
  const adapters=fixtures(),git=adapters.git;
  adapters.git=async args=>args[0]==='show'&&args[1]===`${sha(2)}:Cargo.lock`
    ? '[[package]]\nname = "marmish-native"\nversion = "0.2.1"\n' : git(args);
  assert.equal((await evaluateEligibility(input,adapters)).reason,'invalid_version');
});

test('cross-minor progression and bounded recovery reject downgrade and malformed native tags',async()=>{
  for(const current of ['0.3.0','0.3.1']) {
    const adapters=fixtures({versions:['0.2.18446744073709551615',current,current],
      tags:[{name:'v0.2.18446744073709551615',commit:{sha:sha(1)}}]});
    assert.equal((await evaluateEligibility(input,adapters)).decision,'build');
    const original=await evaluateEligibility(input,fixtures({versions:['0.2.3',current,current]}));
    const recovery=fixtures({versions:['0.2.3',current,current],tags:[{name:`v${current}`,commit:{sha:sha(3)}}],allowSelectedTag:true});
    assert.deepEqual(await verifyEligibility(original,{...recovery,allowSelectedTag:true}),original);
  }
  const lower=fixtures({tags:[{name:'v0.3.0',commit:{sha:sha(1)}}]});
  assert.equal((await evaluateEligibility(input,lower)).reason,'nonadvancing_version');
  const old=await evaluateEligibility(input,fixtures());
  await assert.rejects(verifyEligibility(old,{...fixtures({tags:[{name:'v0.2.2',commit:{sha:sha(3)}},{name:'v0.3.0',commit:{sha:sha(1)}}]}),allowSelectedTag:true}),/recomputation/);
  for(const bad of ['v0.3.01','v0.3.1-rc.0','v0.3.1+build','v0.3.bogus','v0.2.1-bogus','v0.3.']) {
    assert.equal((await evaluateEligibility(input,fixtures({tags:[{name:bad,commit:{sha:sha(1)}}]}))).reason,'invalid_version');
  }
});
