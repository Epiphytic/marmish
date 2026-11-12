import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {checkJobs,candidateLayout,checkReport,checkManifest,unpack,inspectCandidate,expectedArtifacts,releaseFormats} from './release-candidate.mjs';
import {TARGETS,releaseNames,canonicalInventory,parseVersion} from './release-schema.mjs';
import {digest,ROLES} from './transport.mjs';
const source='a'.repeat(40),publisher='b'.repeat(40),harness='c'.repeat(64),version='0.2.3';
const selection={run_id:'12',run_attempt:'2',publisher_sha:publisher};
function zip(entries) {
  return execFileSync('python3',['-c',
    'import sys,json,io,zipfile,base64,warnings;warnings.simplefilter("ignore");b=io.BytesIO()\nwith zipfile.ZipFile(b,"w") as z:\n for e in json.load(sys.stdin):\n  i=zipfile.ZipInfo(e["name"]);i.external_attr=e.get("mode",33152)<<16;z.writestr(i,base64.b64decode(e["bytes"]))\nsys.stdout.buffer.write(b.getvalue())'],
    {input:JSON.stringify(entries.map(e=>({name:e.name,bytes:Buffer.from(e.bytes).toString('base64'),...(e.mode?{mode:e.mode}:{})}))),maxBuffer:1024*1024});
}
function jobs(skip=false,layout='split-v1') {
  const runners=['ubuntu-24.04','ubuntu-24.04-arm','macos-15'];
  return ['release-eligibility','render',...TARGETS.flatMap((target,i)=>[
    `package (${runners[i]}, ${target}, ${i===2?'adhoc, macos=15.0':'none, glibc=2.39'})`,
    ...(layout==='split-v1'?[`installed-tools (${runners[i]}, ${target})`]:[]),
    `install-quickstart (${runners[i]}, ${target})`,`provider-qualification (${runners[i]}, ${target})`])]
    .map(name=>({name,status:'completed',conclusion:skip&&name!=='release-eligibility'?'skipped':'success'}));
}
function report(kind,target,version='0.2.3',relaySelection='explicit') {
  const common={schema:1,ok:true,python_in_runtime_path:false,network_activation:false,owned_state_removed:true,elapsed_ms:10};
  if(kind==='quickstart') return {...common,kind:'installed_quickstart_pty',key_storage:'file',role:'both',inbound_enabled:false,
    prompts_completed:5,identity_created:true,repeat_same_identity:true,repeat_owner_opened:false,stop_complete:true,
    ...(parseVersion(version).minor===3?{relay_selection:relaySelection}:{})};
  return {...common,kind:'installed_auto_provider',owned_children_reaped:true,auto_keychain:true,distinct_named_identities:true,
    fresh_reload:true,item_count:4,locked_refused:true,unlocked_fresh_reload:true,no_plaintext_fallback:true,...(target.endsWith('darwin')?{}:{absent_provider_files:true}),
    source_commit:source,target,run_id:'12',run_attempt:'2',provider_packages:['test-provider=1'],
    ...(target.endsWith('darwin')?{owned_account_removed:true,runner_keychain_unchanged:true,supervisor_interpreter_unchanged:true}:{})};
}
function product(target,version='0.2.3') {
  const files=new Map(['marmish','marmishd','marmish-worker','THIRD-PARTY-LICENSES'].map(prefix=>[
    `${prefix}-${version}-${target}`,Buffer.from(`${prefix}:${target}`)]));
  const text=['marmish-manifest 1',`version ${version}`,`target ${target}`,`commit ${source}`,
    `signing ${target.endsWith('darwin')?'adhoc':'none'} notarized=no`,
    `minimum ${target.endsWith('darwin')?'macos=15.0':'glibc=2.39'}`,`state profile=2 journal=${releaseFormats(version)[1]} engine=1 config=1 commands=1`,
    ...[...files].map(([name,bytes],i)=>`file ${digest(bytes)} ${bytes.length} ${i===3?'0444':'0555'} ${name.slice(0,-1-version.length-1-target.length)}`),'end',''].join('\n');
  files.set(`marmish-${version}-${target}.manifest`,Buffer.from(text));return files;
}
const bases=new Map();
function fixture(version='0.2.3') {
  let base=bases.get(version);
  if(!base) {
    const artifacts=[],blobs=new Map();
    function add(name,files) {
      const id=artifacts.length+1,bytes=zip([...files].map(([name,bytes])=>({name,bytes})));
      artifacts.push({id,name,expired:false,size_in_bytes:bytes.length,digest:`sha256:${digest(bytes)}`,
        workflow_run:{id:12,head_sha:source,head_branch:'master'}});blobs.set(id,bytes);
    }
    const eligibility={schema:1,repository:'Epiphytic/marmish',workflow_path:'.github/workflows/v02-release.yml',run_id:'12',
      run_attempt:'2',event:'workflow_dispatch',source,before:null,version,caveated:false,decision:'build',reason:'eligible_version'};
    add('v02-eligibility-12-2',new Map([['eligibility.json',Buffer.from(JSON.stringify(eligibility)+'\n')]]));
    for(const [i,target] of TARGETS.entries()) {
      add(`v02-candidate-${target}-12-2`,product(target,version));
      const bin=Buffer.from('not executed fixture'),record={schema:1,target,source_commit:source,harness_sha256:harness,
        files:Object.fromEntries(['marmish-installed-check','marmish-installed-fixture'].map(name=>[name,{sha256:digest(bin),bytes:bin.length}]))};
      add(`v02-installed-tools-${target}-12-2`,new Map([[`${ROLES[i]}-tooling.json`,Buffer.from(JSON.stringify(record))],
        ['marmish-installed-check',bin],['marmish-installed-fixture',bin]]));
      for(const kind of ['quickstart','provider']) {
        const files=new Map([[`${kind}.json`,Buffer.from(JSON.stringify(report(kind,target,version)))]]);
        if(kind==='quickstart'&&parseVersion(version).minor===3) files.set('quickstart-default-relays.json',Buffer.from(JSON.stringify(report(kind,target,version,'default'))));
        add(`v02-${kind}-${target}-12-2`,files);
      }
    }
    add('v02-render-tool-12-2',new Map([['marmish-release-tool',Buffer.from('not executed renderer')]]));
    add('v02-pinned-bootstrap-12-2',new Map([['install.sh',Buffer.from('#!/bin/sh\nexit 0\n')]]));
    base={artifacts,blobs};bases.set(version,base);
  }
  const artifacts=structuredClone(base.artifacts),blobs=new Map(base.blobs),calls=[];
  const run={id:12,run_attempt:2,status:'completed',conclusion:'success',event:'workflow_dispatch',head_branch:'master',head_sha:source,
    path:'.github/workflows/v02-release.yml',repository:{full_name:'Epiphytic/marmish'},head_repository:{full_name:'Epiphytic/marmish'}};
  const state={artifacts,blobs,calls,run,latest:{run_attempt:2},jobs:{total_count:14,jobs:jobs()}};
  const api=async route=>{
    calls.push(route);
    if(route.endsWith('/attempts/2')) return state.run;
    if(route.endsWith('/runs/12')) return state.latest;
    if(route.endsWith('/jobs?per_page=100')) return state.jobs;
    if(route.includes('/tags?')||route.includes('/releases?')) return [];
    assert.fail(`unexpected api ${route}`);
  };
  const transport={list:async()=>state.artifacts,request:async(url,auth)=>{
    const text=String(url);calls.push(text);
    if(auth) {const id=Number(/artifacts\/(\d+)\/zip$/.exec(text)?.[1]);return {redirect:`https://fixture.blob.core.windows.net/${id}`};}
    return {bytes:state.blobs.get(Number(new URL(text).pathname.slice(1)))};
  }};
  const git=async argv=>{
    if(argv[0]==='ls-tree')return `100644 blob ${'e'.repeat(40)}\t.github/workflows/v02-release.yml\n`;
    const workflow='env:\n  MARMISH_CANDIDATE_LAYOUT: split-v1\njobs:\n';
    if(argv[0]==='cat-file')return `${Buffer.byteLength(workflow)}\n`;
    assert.equal(argv[0],'show');
    if(argv[1].endsWith('v02-release.yml'))return workflow;
    return `${argv[1].endsWith('Cargo.lock')?'[[package]]':'[package]'}\nname = "marmish-native"\nversion = "${version}"\n`;};
  return {...state,state,adapters:{api,git,transport,harnessDigest:()=>harness}};
}

test('job gate requires exact completed successful matrix or exact designed skip',()=>{
  checkJobs(jobs(),{layout:'split-v1',skip:false});checkJobs(jobs(true),{layout:'split-v1',skip:true});
  const collapsed=['release-eligibility','render','package','installed-tools','install-quickstart','provider-qualification']
    .map(name=>({name,status:'completed',conclusion:name==='release-eligibility'?'success':'skipped'}));
  checkJobs(collapsed,{layout:'split-v1',skip:true});
  for (const malformed of [collapsed.slice(0,-1),[...collapsed,{...collapsed[1]}],
    collapsed.map(job=>job.name==='package'?{...job,name:jobs()[2].name}:job),
    [jobs(true)[0],...jobs(true).filter(job=>job.name.startsWith('provider-qualification')),jobs(true)[3]]]) {
    assert.throws(()=>checkJobs(malformed,{layout:'split-v1',skip:true}),/candidate_jobs_incomplete/);
  }
  for(const mutate of [j=>j.pop(),j=>j.push(j[0]),j=>j[0].conclusion='skipped',j=>j[1].status='in_progress',j=>j[2].conclusion='failure',j=>j[3].name='wrong']) {
    const value=jobs();mutate(value);assert.throws(()=>checkJobs(value,{layout:'split-v1',skip:false}),/candidate_jobs_incomplete/);
  }
  assert.throws(()=>checkJobs(jobs(),{layout:'split-v1',skip:true}),/candidate_jobs_incomplete/);
});
function layoutGit(text, {entry, size, fail}={}) {
  const calls=[];
  const git=async(args,options)=>{
    calls.push({args,options});
    if(args[0]===fail)throw Error('git read failed');
    assert.ok(options.maxBuffer>0&&options.maxBuffer<=256*1024);
    if(args[0]==='ls-tree') {
      assert.deepEqual(args,['ls-tree',source,'--','.github/workflows/v02-release.yml']);
      return entry??`100644 blob ${'e'.repeat(40)}\t.github/workflows/v02-release.yml\n`;
    }
    if(args[0]==='cat-file') {
      assert.deepEqual(args,['cat-file','-s','e'.repeat(40)]);
      return size??`${Buffer.byteLength(text)}\n`;
    }
    assert.deepEqual(args,['show',`${source}:.github/workflows/v02-release.yml`]);
    return text;
  };
  return {git,calls};
}
test('job layout is explicit and closed, with disjoint legacy and split inventories',()=>{
  for(const layout of ['legacy-v1','split-v1']) {
    checkJobs(jobs(false,layout),{layout,skip:false});
    checkJobs(jobs(true,layout),{layout,skip:true});
    const other=layout==='legacy-v1'?'split-v1':'legacy-v1';
    assert.throws(()=>checkJobs(jobs(false,other),{layout,skip:false}),/candidate_jobs_incomplete/);
    assert.throws(()=>checkJobs(jobs(true,other),{layout,skip:true}),/candidate_jobs_incomplete/);
  }
  for(const options of [undefined,true,{}, {layout:'split-v1'}, {layout:'unknown',skip:false}]) {
    assert.throws(()=>checkJobs(jobs(),options),/invalid_candidate_layout/);
  }
  for(const target of TARGETS) for(const change of ['missing','failed','duplicate','renamed']) {
    const value=jobs(),index=value.findIndex(job=>job.name.startsWith('installed-tools (')&&job.name.includes(target));
    if(change==='missing')value.splice(index,1);
    if(change==='failed')value[index].conclusion='failure';
    if(change==='duplicate')value.push(value[index]);
    if(change==='renamed')value[index].name+=' extra';
    assert.throws(()=>checkJobs(value,{layout:'split-v1',skip:false}),/candidate_jobs_incomplete/);
  }
});
test('source workflow layout requires a bounded regular tracked file and exact top-level marker',async()=>{
  for(const [text,expected] of [
    ['name: historical\nenv:\n  OTHER: yes\njobs:\n','legacy-v1'],
    ['name: split\nenv:\n  MARMISH_CANDIDATE_LAYOUT: split-v1\njobs:\n','split-v1'],
  ]) {
    const {git,calls}=layoutGit(text);
    assert.equal(await candidateLayout(source,git),expected);
    assert.deepEqual(calls.map(call=>call.args[0]),['ls-tree','cat-file','show']);
    assert.equal(calls[2].options.maxBuffer,256*1024);
  }
  const bad=[
    '# MARMISH_CANDIDATE_LAYOUT: split-v1\nenv:\n',
    'jobs:\n  tool:\n    env:\n      MARMISH_CANDIDATE_LAYOUT: split-v1\n',
    'env:\n  OTHER: value\njobs:\n  tool:\n    run: |\n      MARMISH_CANDIDATE_LAYOUT: split-v1\n',
    'env:\n  "MARMISH_CANDIDATE_LAYOUT": split-v1\n',
    'env:\n  MARMISH_CANDIDATE_LAYOUT: "split-v1"\n',
    'env:\n  MARMISH_CANDIDATE_LAYOUT: split-v2\n',
    'env:\n  MARMISH_CANDIDATE_LAYOUT: split-v1\n  MARMISH_CANDIDATE_LAYOUT: split-v1\n',
    'env:\n  MARMISH_CANDIDATE_LAYOUT: split-v1\nenv:\n',
    '\ufeffenv:\n  OTHER: value\n',
    'env:\r\n  OTHER: value\r\n',
  ];
  for(const text of bad)await assert.rejects(candidateLayout(source,layoutGit(text).git),/invalid_candidate_layout/);
});
test('layout read failures and oversized files cannot be interpreted as legacy marker absence',async()=>{
  const text='env:\n  OTHER: value\n';
  for(const entry of ['',`120000 blob ${'e'.repeat(40)}\t.github/workflows/v02-release.yml\n`,
    `100644 blob ${'e'.repeat(40)}\twrong.yml\n`,
    `100644 blob ${'e'.repeat(40)}\t.github/workflows/v02-release.yml\n\n`,
    `100644 blob ${'e'.repeat(40)}\t.github/workflows/v02-release.yml\n`.repeat(2)]) {
    const {git,calls}=layoutGit(text,{entry});
    await assert.rejects(candidateLayout(source,git),/invalid_candidate_layout/);
    assert.deepEqual(calls.map(call=>call.args[0]),['ls-tree']);
  }
  for(const size of ['0\n','262145\n','-1\n','NaN\n','1e2\n','1\n\n']) {
    const {git,calls}=layoutGit(text,{size});
    await assert.rejects(candidateLayout(source,git),/invalid_candidate_layout/);
    assert.deepEqual(calls.map(call=>call.args[0]),['ls-tree','cat-file']);
  }
  for(const fail of ['ls-tree','cat-file','show']) {
    await assert.rejects(candidateLayout(source,layoutGit(text,{fail}).git),/git read failed/);
  }
  await assert.rejects(candidateLayout(source,layoutGit(text,{size:'1\n'}).git),/invalid_candidate_layout/);
  await assert.rejects(candidateLayout(source,layoutGit('x'.repeat(262145),{size:'262144\n'}).git),/invalid_candidate_layout/);
  await assert.rejects(candidateLayout('not-a-source',layoutGit(text).git),/invalid_candidate_layout/);
});
test('inspection selects layout from authenticated source, preserving historical candidates and rejecting partial split',async()=>{
  const f=fixture(),original=f.adapters.git;
  const legacy=layoutGit('env:\n  OTHER: value\n');
  f.jobs.jobs=jobs(false,'legacy-v1');f.jobs.total_count=11;
  f.adapters.git=(args,options)=>args[0]==='ls-tree'||args[0]==='cat-file'||args[1]?.endsWith('v02-release.yml')
    ?legacy.git(args,options):original(args,options);
  const result=await inspectCandidate(selection,f.adapters);
  assert.equal(result.inventory.source,source);
  assert.equal(result.inventory.artifacts.length,15);
  assert.ok(legacy.calls.every(call=>call.args[0]==='cat-file'||call.args[1].startsWith(source)));
  const split=fixture();split.jobs.jobs=jobs(false,'legacy-v1');split.jobs.total_count=11;
  await assert.rejects(inspectCandidate(selection,split.adapters),/candidate_jobs_incomplete/);
  const mismatch=fixture();mismatch.adapters.git=f.adapters.git;
  await assert.rejects(inspectCandidate(selection,mismatch.adapters),/candidate_jobs_incomplete/);
});
test('qualification reports are closed and bind platform/source/run with negative invariants',()=>{
  for(const target of TARGETS) for(const kind of ['quickstart','provider']) {
    const good=report(kind,target);checkReport(good,kind,{target,source,run:'12',attempt:'2'});
    for(const change of [{extra:true},{ok:false},{network_activation:true},{owned_state_removed:false},{elapsed_ms:900000}])
      assert.throws(()=>checkReport({...good,...change},kind,{target,source,run:'12',attempt:'2'}));
    if(kind==='provider') for(const change of [{source_commit:publisher},{target:'other'},{run_attempt:'1'},{item_count:3},{auto_keychain:false},{provider_packages:['$(bad)']}])
      assert.throws(()=>checkReport({...good,...change},kind,{target,source,run:'12',attempt:'2'}));
  }
});
test('manifest authenticates exact source/format/modes and each product byte',()=>{
  for(const target of TARGETS) {
    const files=product(target),bytes=files.get(`marmish-${version}-${target}.manifest`);
    checkManifest(bytes,files,version,source,target);
    for(const [from,to] of [[source,publisher],['profile=2','profile=3'],['0555','0444'],['glibc=2.39','glibc=2.40'],['notarized=no','notarized=yes']]) {
      if(!bytes.includes(from)) continue;
      assert.throws(()=>checkManifest(Buffer.from(bytes.toString().replace(from,to)),files,version,source,target));
    }
    const changed=new Map(files);changed.set(`marmish-${version}-${target}`,Buffer.from('changed'));
    assert.throws(()=>checkManifest(bytes,changed,version,source,target),/manifest_payload/);
  }
});
test('unpack requires exact regular members and rejects traversal, duplicate, link, extras and malformed ZIP',async()=>{
  assert.equal((await unpack(zip([{name:'a',bytes:'one'}]),['a'])).get('a').toString(),'one');
  for(const entries of [[{name:'../a',bytes:'x'}],[{name:'a',bytes:'x'},{name:'a',bytes:'y'}],
    [{name:'a',bytes:'target',mode:0o120777}],[{name:'a',bytes:'x'},{name:'extra',bytes:'y'}],[]]) {
    await assert.rejects(unpack(zip(entries),['a']),/invalid_(?:archive|artifact_zip)/);
  }
  await assert.rejects(unpack(Buffer.from('not zip'),['a']),/invalid_(?:archive|artifact_zip)/);
  const encrypted=zip([{name:'a',bytes:'x'}]);
  const central=encrypted.indexOf(Buffer.from([0x50,0x4b,0x01,0x02]));encrypted.writeUInt16LE(1,central+8);
  await assert.rejects(unpack(encrypted,['a']),/invalid_(?:archive|artifact_zip)/);
});
test('inspect full source-bound fixture produces exact immutable public inventory without execution',async()=>{
  const f=fixture(),result=await inspectCandidate(selection,f.adapters);
  assert.equal(result.skip,false);assert.equal(result.payload.size,15);
  assert.deepEqual([...result.payload.keys()].sort(),releaseNames(version));
  assert.equal(result.inventory.source,source);assert.equal(result.inventory.artifacts.length,15);
  assert.equal(result.inventory.reports.length,7);assert.ok(canonicalInventory(result.inventory).length>0);
  assert.equal(result.renderer.toString(),'not executed renderer');
  assert.deepEqual(f.artifacts.map(a=>a.name).sort(),expectedArtifacts('12','2').sort());
});
test('inspect refuses run/fork/event/attempt/artifact provenance before trusting bytes',async()=>{
  for(const [change,reason] of [
    [f=>f.run.head_repository.full_name='fork/marmish','invalid_candidate_provenance'],
    [f=>f.run.event='pull_request','invalid_candidate_provenance'],
    [f=>f.run.conclusion='failure','invalid_candidate_provenance'],
    [f=>f.latest.run_attempt=3,'candidate_attempt_changed'],
    [f=>f.artifacts.push(f.artifacts[0]),'duplicate_artifact'],
    [f=>f.artifacts[0].expired=true,'artifact_provenance'],
    [f=>f.artifacts[0].workflow_run.head_sha=publisher,'artifact_provenance'],
    [f=>f.artifacts[0].digest='sha256:'+'0'.repeat(64),'artifact_digest_mismatch'],
    [f=>f.jobs.jobs[1].conclusion='skipped','candidate_jobs_incomplete'],
    [f=>f.jobs.total_count=15,'jobs_bound'],
  ]) {
    const f=fixture();change(f);
    await assert.rejects(inspectCandidate(selection,f.adapters),new RegExp(reason));
  }
});
function replaceArtifact(f,name,entries) {
  const artifact=f.artifacts.find(a=>a.name===name),bytes=zip(entries);
  f.blobs.set(artifact.id,bytes);artifact.size_in_bytes=bytes.length;artifact.digest=`sha256:${digest(bytes)}`;
}
test('inspect independently rejects qualification, manifest, tooling and artifact-inventory tampering',async()=>{
  const target=TARGETS[0];
  for(const [name,entries,reason] of [
    [`v02-provider-${target}-12-2`,[{name:'provider.json',bytes:JSON.stringify({...report('provider',target),source_commit:publisher})}],'provider_failed'],
    [`v02-quickstart-${target}-12-2`,[{name:'quickstart.json',bytes:JSON.stringify({...report('quickstart',target),ok:false})}],'qualification_failed'],
    [`v02-installed-tools-${target}-12-2`,[{name:'linux-amd64-tooling.json',bytes:'{}'},
      {name:'marmish-installed-check',bytes:'x'},{name:'marmish-installed-fixture',bytes:'x'}],'invalid_tooling_record'],
  ]) {
    const f=fixture();replaceArtifact(f,name,entries);
    await assert.rejects(inspectCandidate(selection,f.adapters),new RegExp(reason));
  }
  const f=fixture(),files=product(target);
  files.set(`marmish-${version}-${target}`,Buffer.from('changed product'));
  replaceArtifact(f,`v02-candidate-${target}-12-2`,[...files].map(([name,bytes])=>({name,bytes})));
  await assert.rejects(inspectCandidate(selection,f.adapters),/manifest_payload/);
  const missing=fixture();missing.artifacts.pop();
  await assert.rejects(inspectCandidate(selection,missing.adapters),/candidate_artifact_inventory/);
});
test('verified unchanged skip requires exact eligibility-success and remaining skipped jobs',async()=>{
  const f=fixture(),before='d'.repeat(40);
  const eligibility={schema:1,repository:'Epiphytic/marmish',workflow_path:'.github/workflows/v02-release.yml',run_id:'12',run_attempt:'2',
    event:'push',source,before,version,caveated:false,decision:'skip-by-design',reason:'unchanged_version'};
  f.run.event='push';f.jobs.jobs=jobs(true);
  replaceArtifact(f,'v02-eligibility-12-2',[{name:'eligibility.json',bytes:JSON.stringify(eligibility)}]);
  f.state.artifacts=[f.artifacts[0]];
  const originalGit=f.adapters.git;
  f.adapters.git=async args=>args[0]==='rev-parse'?'false':args[0]==='rev-list'?`${source}\n${before}\n`:originalGit(args);
  assert.deepEqual(await inspectCandidate(selection,f.adapters),{skip:true,reason:'unchanged_version'});
  f.jobs.jobs[1].conclusion='success';
  await assert.rejects(inspectCandidate(selection,f.adapters),/candidate_jobs_incomplete/);
});
test('rerun selects only exact attempt artifacts before duplicate, inventory and byte verification',async()=>{
  const f=fixture();
  const prior=f.artifacts.map(artifact=>({...structuredClone(artifact),name:artifact.name.replace(/-12-2$/,'-12-1'),
    id:artifact.id+100,expired:true,digest:'old invalid digest'}));
  // Even duplicated unrelated names/IDs are outside the selected attempt's authority.
  f.artifacts.push(...prior,prior[0]);
  const result=await inspectCandidate(selection,f.adapters);
  assert.equal(result.inventory.artifacts.length,15);
  assert.ok(result.inventory.artifacts.every(artifact=>artifact.name.endsWith('-12-2')&&artifact.id<100));
  assert.ok(f.calls.every(call=>!call.includes('/artifacts/101/')));
  const selectedDuplicate=fixture();selectedDuplicate.artifacts.push({...selectedDuplicate.artifacts[0],id:999});
  await assert.rejects(inspectCandidate(selection,selectedDuplicate.adapters),/duplicate_artifact/);
});
test('caveated troubleshooting build remains unpublishable',async()=>{
  const f=fixture();
  const eligibility={schema:1,repository:'Epiphytic/marmish',workflow_path:'.github/workflows/v02-release.yml',run_id:'12',run_attempt:'2',
    event:'workflow_dispatch',source,before:null,version,caveated:true,decision:'build',reason:'eligible_version'};
  replaceArtifact(f,'v02-eligibility-12-2',[{name:'eligibility.json',bytes:JSON.stringify(eligibility)}]);
  await assert.rejects(inspectCandidate(selection,f.adapters),/eligibility_binding/);
});

test('0.3.0 and 0.3.1 candidates authenticate both relay reports and ten exact report pins',async()=>{
  for(const current of ['0.3.0','0.3.1']) {
    const f=fixture(current),result=await inspectCandidate(selection,f.adapters);
    assert.equal(result.inventory.version,current);assert.equal(result.inventory.reports.length,10);
    assert.equal(result.inventory.assets.length,15);canonicalInventory(result.inventory);
    for(const target of TARGETS) {
      const base=`v02-quickstart-${target}-12-2`;
      for(const [suffix,relay] of [['','explicit'],['-default-relays','default']]) {
        assert.equal(result.inventory.reports.find(r=>r.name===base+suffix).sha256,
          digest(Buffer.from(JSON.stringify(report('quickstart',target,current,relay)))));
      }
    }
  }
});
test('0.3 quickstart rejects absent, duplicate, extra, swapped, missing and tampered relay reports',async()=>{
  const target=TARGETS[0],name=`v02-quickstart-${target}-12-2`;
  const good=()=>[{name:'quickstart.json',bytes:JSON.stringify(report('quickstart',target,'0.3.0','explicit'))},
    {name:'quickstart-default-relays.json',bytes:JSON.stringify(report('quickstart',target,'0.3.0','default'))}];
  const changes=[entries=>entries.pop(),entries=>entries.push(entries[0]),entries=>entries.push({name:'extra.json',bytes:'{}'}),
    entries=>{[entries[0].bytes,entries[1].bytes]=[entries[1].bytes,entries[0].bytes];},
    entries=>{const value=JSON.parse(entries[1].bytes);delete value.relay_selection;entries[1].bytes=JSON.stringify(value);},
    entries=>{const value=JSON.parse(entries[1].bytes);value.stop_complete=false;entries[1].bytes=JSON.stringify(value);},
    entries=>{const value=JSON.parse(entries[0].bytes);value.relay_selection='unknown';entries[0].bytes=JSON.stringify(value);}];
  for(const change of changes) {
    const f=fixture('0.3.0'),entries=good();change(entries);replaceArtifact(f,name,entries);
    await assert.rejects(inspectCandidate(selection,f.adapters));
  }
  const old=fixture();replaceArtifact(old,name,good());await assert.rejects(inspectCandidate(selection,old.adapters),/invalid_archive/);
});
test('manifest journal formats are closed and bound to release family in both directions',()=>{
  for(const current of ['0.2.3','0.3.0','0.3.1']) for(const target of TARGETS) {
    const files=product(target,current),bytes=files.get(`marmish-${current}-${target}.manifest`);
    checkManifest(bytes,files,current,source,target);
    const expected=releaseFormats(current)[1],wrong=expected===7?6:7;
    assert.throws(()=>checkManifest(Buffer.from(bytes.toString().replace(`journal=${expected}`,`journal=${wrong}`)),files,current,source,target),/manifest_provenance/);
  }
});
test('current Rust format constants equal the authenticated 0.3 release tuple',()=>{
  const read=(file,name)=>{
    const text=readFileSync(new URL(`../../${file}`,import.meta.url),'utf8');
    const found=new RegExp(`const ${name}: [a-z0-9]+ = ([0-9]+);`).exec(text);
    assert.ok(found,`${file}:${name}`);return Number(found[1]);
  };
  const actual=[read('crates/marmish-native/src/identity.rs','PROFILE_FORMAT'),
    read('crates/marmish-native/src/journal.rs','JOURNAL_FORMAT'),read('crates/marmish-engine/src/store.rs','ENGINE_FORMAT'),
    read('crates/marmish-native/src/command_config.rs','CONFIG_FORMAT'),read('crates/marmish-native/src/command_wire.rs','COMMAND_FORMAT')];
  assert.deepEqual(actual,releaseFormats('0.3.0'));
  const metadata=readFileSync(new URL('../../crates/marmish-native/src/native_manifest.rs',import.meta.url),'utf8');
  for(const constant of ['PROFILE_FORMAT','JOURNAL_FORMAT','ENGINE_FORMAT','CONFIG_FORMAT','COMMAND_FORMAT']) assert.ok(metadata.includes(constant));
});
