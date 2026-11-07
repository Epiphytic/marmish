import test from 'node:test';
import assert from 'node:assert/strict';
import {POLICY,ARTIFACTS,ALLOWED_PATHS,COLD_ALLOWED_PATHS,checkColdReport,checkColdFailureReport,checkInputs,checkChanges,guardSource,checkCandidate,
  selectArtifact,checkZip,checkProduct,checkBootstrap,checkToolFiles,checkPins,checkReport,validateReport,requestedConcurrency,checkFailureReport,checkFailureMarker} from './macos-diagnostic.mjs';
import {digest} from './transport.mjs';
const harness='a'.repeat(40),oldHarness='b'.repeat(64);
const env={GITHUB_REPOSITORY:POLICY.repository,GITHUB_REF:'refs/heads/master',GITHUB_SHA:harness,
  HARNESS_COMMIT:harness,DIAGNOSTIC_ROOT:'/private/tmp/owned',PROBE:'/private/tmp/probe'};
test('diagnostic fixes candidate policy and separates harness inputs',()=>{
  checkInputs(env);
  assert.equal(POLICY.source,'571d98b77780d265a2fb29d84ae160fec72ba37b');
  for (const change of [{HARNESS_COMMIT:POLICY.source},{GITHUB_REPOSITORY:'fork/marmish'},
    {GITHUB_REF:'refs/heads/feature'},{GITHUB_SHA:'bad'},{DIAGNOSTIC_ROOT:'relative'},{PROBE:undefined}]) {
    assert.throws(()=>checkInputs({...env,...change}));
  }
});
test('closed source allowlist refuses rename deletion executable symlink and shared fixture edits',()=>{
  for (const name of ALLOWED_PATHS) checkChanges([['A',name,'100644']]);
  for (const row of [['D',ALLOWED_PATHS[0],'100644'],['R100',ALLOWED_PATHS[0],'100644'],
    ['M',ALLOWED_PATHS[0],'100755'],['A',ALLOWED_PATHS[0],'120000'],
    ['M','crates/marmish-native/src/installed_streaming_fixture.rs','100644'],
    ['M','tools/v02-actions/workflow.mjs','100644']]) assert.throws(()=>checkChanges([row]));
  assert.throws(()=>checkChanges([]));
});
function gitFixture(fail) {
  return args=>{
    if (fail?.(args)) throw new Error('git rejected');
    if(args[0] === 'rev-parse') return harness+'\n';
    if(args[0] === 'diff' && args.includes('--name-status')) return `A\0${ALLOWED_PATHS[0]}\0`;
    if(args[0] === 'ls-tree') return `100644 blob ${'c'.repeat(40)}\t${ALLOWED_PATHS[0]}\n`;
    return '';
  };
}
test('ancestry guard checks both directions plus exact HEAD and dirty tree',()=>{
  const calls=[];guardSource(env,args=>{calls.push(args);return gitFixture()(args);});
  assert(calls.some(a=>a.join(' ')===`merge-base --is-ancestor ${POLICY.source} ${harness}`));
  assert(calls.some(a=>a.join(' ')===`merge-base --is-ancestor ${harness} refs/remotes/origin/master`));
  for (const predicate of [a=>a[0]==='merge-base'&&a[2]===POLICY.source,
    a=>a[0]==='merge-base'&&a[2]===harness,a=>a[0]==='diff'&&a.includes('--quiet')]) {
    assert.throws(()=>guardSource(env,gitFixture(predicate)));
  }
  assert.throws(()=>guardSource(env,a=>a[0]==='rev-parse'?'c'.repeat(40):gitFixture()(a)));
});
const candidate=()=>({id:Number(POLICY.run),run_attempt:1,status:'completed',conclusion:'success',
  event:'workflow_dispatch',path:'.github/workflows/streaming-candidate.yml',head_branch:'master',
  repository:{full_name:POLICY.repository},head_repository:{full_name:POLICY.repository},head_sha:POLICY.source});
test('immutable candidate run attempt workflow and latest attempt are mandatory',()=>{
  checkCandidate(candidate(),candidate());
  for (const change of [{id:1},{run_attempt:2},{status:'in_progress'},{conclusion:'failure'},
    {head_sha:harness},{path:'.github/workflows/v02-release.yml'},{event:'push'},
    {repository:{full_name:'fork/marmish'}}]) assert.throws(()=>checkCandidate({...candidate(),...change},candidate()));
  for (const change of [{run_attempt:2},{head_sha:harness},{id:1}]) {
    assert.throws(()=>checkCandidate(candidate(),{...candidate(),...change}));
  }
});
function artifact(kind) {
  const p=ARTIFACTS[kind];return {...p,size_in_bytes:p.bytes,digest:`sha256:${p.sha256}`,expired:false,
    workflow_run:{id:Number(POLICY.run),head_sha:POLICY.source,head_branch:'master'}};
}
test('artifact selection binds exact immutable API ID ZIP hash size and source',()=>{
  for (const kind of Object.keys(ARTIFACTS)) {
    const a=artifact(kind);assert.equal(selectArtifact([a],kind),ARTIFACTS[kind]);
    for (const change of [{id:a.id+1},{digest:undefined},{digest:'sha256:'+'a'.repeat(64)},
      {size_in_bytes:a.bytes-1},{expired:true},{workflow_run:{id:Number(POLICY.run),head_sha:harness,head_branch:'master'}}]) {
      assert.throws(()=>selectArtifact([{...a,...change}],kind));
    }
    assert.throws(()=>selectArtifact([a,a],kind));assert.throws(()=>selectArtifact([],kind));
  }
  assert.throws(()=>selectArtifact([],'constructor'));
});
test('ZIP rejection precedes extraction without a matching frozen digest',()=>{
  assert.throws(()=>checkZip(Buffer.from('not a zip'),'product'),/zip_digest/);
  const wrong=Buffer.alloc(ARTIFACTS.bootstrap.bytes);
  assert.throws(()=>checkZip(wrong,'bootstrap'),/zip_digest/);
});
function product() {
  const version='0.2.3',suffix=`${version}-${POLICY.target}`,files=new Map();
  const names=['marmish','marmishd','marmish-worker','THIRD-PARTY-LICENSES'];
  const rows=names.map((name,i)=>{
    const bytes=Buffer.from(`immutable ${name}`);files.set(`${name}-${suffix}`,bytes);
    return `file ${digest(bytes)} ${bytes.length} ${i===3?'0444':'0555'} ${name}`;
  });
  const manifest=Buffer.from(['marmish-manifest 1',`version ${version}`,`target ${POLICY.target}`,
    `commit ${POLICY.source}`,'signing adhoc notarized=no','minimum macos=15.0',
    'state profile=2 journal=7 engine=1 config=1 commands=1',...rows,'end',''].join('\n'));
  files.set(`marmish-${suffix}.manifest`,manifest);return files;
}
test('streaming journal7 manifest verifies all four exact payloads without release journal6 assumption',()=>{
  const files=product(),checked=checkProduct(files);assert.equal(checked.version,'0.2.3');
  assert.equal(checked.manifest.bytes,[...files.values()].at(-1).length);
  for (const change of [f=>f.set('unrelated',Buffer.from('x')),f=>f.delete([...f.keys()][0]),
    f=>f.set([...f.keys()][0],Buffer.from('changed')),
    f=>{const name=[...f.keys()].at(-1);f.set(name,Buffer.from(f.get(name).toString().replace(POLICY.source,harness)));}]) {
    const changed=product();change(changed);assert.throws(()=>checkProduct(changed));
  }
});
const template=Buffer.from("#!/bin/sh\n    version='@MARMISH_VERSION@'\n    sha_amd64='@MARMISH_SHA_AMD64@'\n    sha_arm64='@MARMISH_SHA_ARM64@'\n    sha_macos='@MARMISH_SHA_MACOS@'\necho unchanged\n");
const mac='c'.repeat(64);
function bootstrap() {return Buffer.from(template.toString().replace('@MARMISH_VERSION@','0.2.3')
  .replace('@MARMISH_SHA_AMD64@','a'.repeat(64)).replace('@MARMISH_SHA_ARM64@','b'.repeat(64)).replace('@MARMISH_SHA_MACOS@',mac));}
test('rendered bootstrap matches exact old template and four substitutions',()=>{
  checkBootstrap(bootstrap(),template,'0.2.3',mac);
  for (const bytes of [Buffer.from(bootstrap().toString().replace('unchanged','changed')),
    Buffer.from(bootstrap().toString().replace('0.2.3','0.2.4')),
    Buffer.from(bootstrap().toString().replace(mac,'d'.repeat(64))),template,
    Buffer.from(bootstrap().toString().replace('a'.repeat(64),'A'.repeat(64)))]) {
    assert.throws(()=>checkBootstrap(bytes,template,'0.2.3',mac));
  }
  assert.throws(()=>checkBootstrap(bootstrap(),Buffer.concat([template,Buffer.from('@MARMISH_VERSION@')]),'0.2.3',mac));
});
function toolFiles() {
  const files=new Map([['marmish-installed-check',Buffer.from('old checker')],['marmish-installed-fixture',Buffer.from('old fixture')]]);
  files.set('macos-arm64-tooling.json',Buffer.from(JSON.stringify({schema:1,target:POLICY.target,source_commit:POLICY.source,
    harness_sha256:oldHarness,files:Object.fromEntries([...files].map(([name,b])=>[name,{sha256:digest(b),bytes:b.length}]))})));
  return files;
}
test('old fixture is selected exclusively from exact old tooling record with source fingerprint',()=>{
  const files=toolFiles();assert.equal(checkToolFiles(files,oldHarness).fixture.sha256,digest(Buffer.from('old fixture')));
  assert.throws(()=>checkToolFiles(files,'d'.repeat(64)));
  const changed=toolFiles();changed.set('marmish-installed-fixture',Buffer.from('new harness fixture'));
  assert.throws(()=>checkToolFiles(changed,oldHarness));
  const extra=toolFiles();extra.set('new-fixture',Buffer.from('x'));assert.throws(()=>checkToolFiles(extra,oldHarness));
});

// Synthetic closed records exercise validation only; they are not runtime evidence.
function reportFixture(concurrent=false) {
  const f={sha256:'c'.repeat(64),bytes:1};
  const pins={schema:1,kind:'macos-stream-diagnostic-pins',qualification:false,repository:POLICY.repository,
    candidate_run_id:POLICY.run,candidate_attempt:POLICY.attempt,candidate_commit:POLICY.source,
    harness_commit:harness,target:POLICY.target,version:'0.2.3',old_harness_sha256:oldHarness,
    artifacts:ARTIFACTS,manifest:f,tooling_record:f,bootstrap:f,fixture:f};
  const cases=['fixture_direct','raw','zlib',...(concurrent?['concurrent-1','concurrent-2','concurrent-3']:[])];
  const observations=cases.map(name=>{
    const direct=name==='fixture_direct',terminal=direct?null:{kind:'exited',data:{code:7}};
    return {case:name,stage:direct?'fixture_direct':'receipt',failure:null,exit:7,signal:null,
      input_bytes:98304,input_sha256:f.sha256,stdin_written:98304,stdout_bytes:98304,stdout_sha256:f.sha256,
      stderr_bytes:direct?8192:8300,stderr_sha256:f.sha256,input_epipe:false,capture_overflow:false,
      capture_io:false,timed_out:false,interrupted:false,owned_group_clean:true,stream_present:!direct,
      binding_equal:!direct,client_terminal:terminal,server_terminal:terminal,receipt_present:true,
      receipt_valid:true,output_match:true,offsets_available:false,wall_ms:100};
  });
  const report={schema:1,kind:'macos-stream-diagnostic',qualification:false,same_host_only:true,
    relay:'wss://relay.us.whitenoise.chat',pins,probe_sha256:f.sha256,stage:'stream_capture',failure:null,
    observations,cleanup:Object.fromEntries(['client_cancel','server_cancel','client_idle','server_idle',
      'client_stop','server_stop'].map(k=>[k,true])),passed:true};
  return {report,pins,probe:f};
}
function verifyReport(f,changedComplete={},concurrent=false) {
  const bytes=Buffer.from(JSON.stringify(f.report));
  checkReport(f.report,{schema:1,kind:'macos-diagnostic-complete',report_sha256:digest(bytes),passed:true,
    ...changedComplete},bytes,f.pins,f.probe,concurrent);
}
test('closed diagnostic success records preserve nonqualifying provenance and exact cases',()=>{
  for(const concurrent of [false,true]) {const f=reportFixture(concurrent);checkPins(f.pins,harness);verifyReport(f,{},concurrent);}
  for(const mutate of [f=>f.pins.schema=true,f=>f.pins.artifacts={...ARTIFACTS,extra:ARTIFACTS.product},
    f=>f.pins.candidate_attempt='2',f=>f.pins.fixture={sha256:'c'.repeat(64),bytes:true}]) {
    const f=reportFixture();mutate(f);assert.throws(()=>checkPins(f.pins,harness));
  }
  for(const mutate of [f=>f.report.schema=true,f=>f.report.qualification=true,f=>f.report.passed=false,
    f=>f.report.extra='private',f=>f.report.cleanup.server_idle=false,
    f=>f.report.observations.pop(),f=>f.report.observations[1].case='fixture_direct',
    f=>f.report.observations[1].stdin_written=true,f=>f.report.observations[1].wall_ms='100',
    f=>f.report.observations[1].capture_io=true,f=>f.report.observations[1].receipt_valid=false,
    f=>f.report.observations[1].server_terminal={kind:'cancelled'},
    f=>f.report.observations[1].client_terminal={kind:'exited',data:{code:7,extra:true}},
    f=>f.report.probe_sha256='d'.repeat(64),f=>f.report.pins={...f.pins,harness_commit:'d'.repeat(40)}]) {
    const f=reportFixture();mutate(f);assert.throws(()=>verifyReport(f));
  }
  assert.throws(()=>verifyReport(reportFixture(),{schema:true}));
  assert.throws(()=>verifyReport(reportFixture(),{report_sha256:'d'.repeat(64)}));
});
test('credential-free validator verifies closed failed reports and marker precedence end to end',async()=>{
  const {promises:fs}=await import('node:fs');const os=await import('node:os');
  const path=await import('node:path');const {execFileSync}=await import('node:child_process');
  const {sourceDigest}=await import('./workflow.mjs');
  const root=await fs.mkdtemp(path.join(os.tmpdir(),'marmish-diag-'));
  try {
    await fs.mkdir(path.join(root,'output'));
    const commit=execFileSync('git',['rev-parse','HEAD'],{encoding:'utf8'}).trim();
    const f=reportFixture();f.pins.harness_commit=commit;
    const probe=Buffer.from('nonexecutable synthetic probe');f.probe={bytes:probe.length,sha256:digest(probe)};
    f.report.probe_sha256=f.probe.sha256;
    const probePath=path.join(root,'probe');await fs.writeFile(probePath,probe);
    const pinsBytes=Buffer.from(JSON.stringify(f.pins));
    await fs.writeFile(path.join(root,'pins.json'),pinsBytes);
    await fs.writeFile(path.join(root,'provenance.json'),JSON.stringify({schema:1,
      kind:'macos-stream-diagnostic-provenance',qualification:false,pins_sha256:digest(pinsBytes),
      product:f.pins,harness:{commit,source_sha256:sourceDigest(commit),target:POLICY.target,probe:f.probe}}));
    const checkEnv={...env,GITHUB_SHA:commit,HARNESS_COMMIT:commit,PROBE:probePath,
      DIAGNOSTIC_ROOT:root,RUN_CONCURRENT:'false'};
    const writeReport=async()=>{
      const bytes=Buffer.from(JSON.stringify(f.report));
      await fs.writeFile(path.join(root,'output/report.json'),bytes);
      await fs.writeFile(path.join(root,'output/complete.json'),JSON.stringify({schema:1,
        kind:'macos-diagnostic-complete',report_sha256:digest(bytes),passed:true}));
    };
    await writeReport();await validateReport(checkEnv);
    f.report.passed=false;f.report.failure='capture_io';await writeReport();
    await assert.rejects(validateReport(checkEnv),/diagnostic_failed_report/);
    f.report.passed=true;f.report.failure=null;await writeReport();
    const marker=path.join(root,'output/failure.json');
    await fs.writeFile(marker,JSON.stringify({schema:1,kind:'macos-diagnostic-failure',failure:'report_failed'}));
    await assert.rejects(validateReport(checkEnv),/diagnostic_failure_marker/);
    await fs.writeFile(marker,'malformed');await assert.rejects(validateReport(checkEnv));
    await fs.unlink(marker);await fs.symlink(path.join(root,'absent'),marker);
    await assert.rejects(validateReport(checkEnv));
    await fs.unlink(marker);
    f.report={...coldFixture().report,pins:f.pins,probe_sha256:f.probe.sha256};
    const coldEnv={...checkEnv,RUN_CONCURRENT:undefined};
    await writeReport();await validateReport(coldEnv,'cold');
    await assert.rejects(validateReport(checkEnv));
    f.report.passed=false;f.report.failure='topology_mismatch';await writeReport();
    await assert.rejects(validateReport(coldEnv,'cold'),/diagnostic_failed_report/);
    f.report.passed=true;f.report.failure=null;await writeReport();
    await fs.writeFile(marker,JSON.stringify({schema:1,kind:'macos-diagnostic-failure',failure:'report_failed'}));
    await assert.rejects(validateReport(coldEnv,'cold'),/diagnostic_failure_marker/);

  } finally {await fs.rm(root,{recursive:true,force:true});}
});

test('requested concurrency is mandatory and binds exact requested cells',()=>{
  assert.equal(requestedConcurrency('true'),true);assert.equal(requestedConcurrency('false'),false);
  for(const value of [undefined,null,true,false,'TRUE','1','']) assert.throws(()=>requestedConcurrency(value));
  assert.throws(()=>verifyReport(reportFixture(false),{},true));
  assert.throws(()=>verifyReport(reportFixture(true),{},false));
  const f=reportFixture();const bytes=Buffer.from(JSON.stringify(f.report));
  assert.throws(()=>checkReport(f.report,{schema:1,kind:'macos-diagnostic-complete',
    report_sha256:digest(bytes),passed:true},bytes,f.pins,f.probe));
});

test('failed reports remain closed sanitized diagnostic records without becoming successful',()=>{
  const f=reportFixture();f.report.passed=false;f.report.failure='budget_exhausted';
  f.report.observations[1].failure='terminal_mismatch';
  f.report.observations[1].server_terminal={kind:'failed',data:{code:'uncertain'}};
  checkFailureReport(f.report,f.pins,f.probe,false);
  checkFailureMarker({schema:1,kind:'macos-diagnostic-failure',failure:'report_failed'});
  assert.throws(()=>verifyReport(f));
  const mutateCases=[r=>r.failure='/private/account',r=>r.stage='stderr: secret',r=>r.raw_stderr='private',
    r=>r.observations[0].path='/private',r=>r.observations[0].failure='private error text',
    r=>r.observations[0].exit=true,r=>r.observations[0].stdin_written=true,
    r=>r.observations[0].stdout_bytes=1048577,r=>r.observations[0].wall_ms=1080001,
    r=>r.observations[0].capture_io='false',r=>r.observations[0].client_terminal={kind:'failed',data:{code:'private'}},
    r=>r.observations[0].server_terminal={kind:'exited',data:{code:true}},
    r=>r.observations.push(r.observations[0]),r=>r.probe_sha256='f'.repeat(64)];
  for(const mutate of mutateCases) {
    const r=structuredClone(f.report);mutate(r);
    assert.throws(()=>checkFailureReport(r,f.pins,f.probe,false));
  }
  for(const marker of [{schema:true,kind:'macos-diagnostic-failure',failure:'report_failed'},
    {schema:1,kind:'macos-diagnostic-failure',failure:'private'},
    {schema:1,kind:'macos-diagnostic-failure',failure:'report_failed',path:'/private'}]) {
    assert.throws(()=>checkFailureMarker(marker));
  }
  const early=structuredClone(f.report);early.pins=null;early.stage='pins';early.failure='pins_invalid';
  early.observations=[];checkFailureReport(early,f.pins,f.probe,false);
  early.stage='install';assert.throws(()=>checkFailureReport(early,f.pins,f.probe,false));
});


test('cold mode source guard adds only its exact reviewed paths without broadening original mode',()=>{
  assert.equal(COLD_ALLOWED_PATHS.length,ALLOWED_PATHS.length+10);
  for(const name of COLD_ALLOWED_PATHS) checkChanges([['M',name,'100644']],'cold');
  for(const name of COLD_ALLOWED_PATHS.filter(p=>!ALLOWED_PATHS.includes(p))) {
    assert.throws(()=>checkChanges([['A',name,'100644']]));
    const git=args=>args[0] === 'diff' && args.includes('--name-status') ? `A\0${name}\0` : gitFixture()(args);
    guardSource(env,git,'cold');assert.throws(()=>guardSource(env,git));
  }
  for(const row of [['M','crates/marmish-native/src/installed_streaming_cases.rs','100644'],
    ['M','crates/marmish-native/src/installed_streaming_fixture.rs','100644'],
    ['M','docs/validation/arbitrary.json','100644'],['R100',COLD_ALLOWED_PATHS.at(-1),'100644'],
    ['D',COLD_ALLOWED_PATHS.at(-1),'100644'],['A',COLD_ALLOWED_PATHS.at(-1),'100755'],
    ['A',COLD_ALLOWED_PATHS.at(-1),'120000']]) assert.throws(()=>checkChanges([row],'cold'));
  assert.throws(()=>checkChanges([['M',ALLOWED_PATHS[0],'100644']],'constructor'));
});
function coldFixture() {
  const f=reportFixture(true),old=f.report.observations;
  f.report={...f.report,kind:'macos-cold-multiprincipal-diagnostic',stage:'fixture_direct',
    scenario:'cold-three-principals',cold_before_direct:true,local_outbound_fixture_traffic:false,
    observations:[...old.slice(3).map((o,i)=>({...o,case:`cold-${i+1}`})),{...old[0],case:'direct-after'}],
    principals:['linux-amd64','linux-arm64','macos-arm64'].map((role,i)=>({label:`client-${i+1}`,role,
      account_sha256:String(i+1).repeat(64),session_sha256:String(i+4).repeat(64),
      group_sha256:String(i+7).repeat(64),open_sha256:['a','b','c'][i].repeat(64),pair_valid:true})),
    cleanup:{clients:Array.from({length:3},()=>({cancel:true,idle:true,stop:true})),
      server:{cancel:true,idle:true,stop:true}}};
  return f;
}
function verifyCold(f) {
  const bytes=Buffer.from(JSON.stringify(f.report));
  checkColdReport(f.report,{schema:1,kind:'macos-diagnostic-complete',passed:true,
    report_sha256:digest(bytes)},bytes,f.pins,f.probe);
}
test('cold success requires ordered distinct principals and rooms, Pair evidence and all four cleanups',()=>{
  verifyCold(coldFixture());
  for(const mutate of [f=>f.report.kind='macos-stream-diagnostic',f=>f.report.scenario='warm-only',
    f=>f.report.cold_before_direct=false,f=>f.report.local_outbound_fixture_traffic=true,
    f=>f.report.observations.reverse(),f=>f.report.observations.splice(0,3),
    f=>f.report.principals.pop(),f=>f.report.principals[0].pair_valid=false,
    f=>f.report.principals[1].account_sha256=f.report.principals[0].account_sha256,
    ...['session_sha256','group_sha256','open_sha256'].map(key=>f=>f.report.principals[1][key]=f.report.principals[0][key]),
    f=>f.report.principals[0].open_sha256=null,f=>f.report.principals[0].role='server',
    f=>f.report.principals[0].private_path='/private/state',f=>f.report.cleanup.clients[1].idle=false,
    f=>f.report.cleanup.server.stop=false,f=>f.report.observations[0].wall_ms=330001,
    f=>f.report.observations[3].wall_ms=15001,f=>f.report.observations[0].server_terminal={kind:'cancelled'},
    f=>f.report.observations[0].offsets_available=true]) {
    const f=coldFixture();mutate(f);assert.throws(()=>verifyCold(f));
  }
  const f=coldFixture();assert.throws(()=>verifyReport(f));
});
test('closed cold failures retain failed cold then successful direct without converting to pass',()=>{
  const make=()=>{const f=coldFixture();f.report.passed=false;f.report.failure='hash_mismatch';
    f.report.observations[0].failure='hash_mismatch';f.report.observations[0].output_match=false;return f;};
  const f=make();checkColdFailureReport(f.report,f.pins,f.probe);assert.throws(()=>verifyCold(f));
  for(const mutate of [f=>f.report.observations.splice(0,3),f=>f.report.cold_before_direct=false,
    f=>f.report.failure='private arbitrary error',f=>f.report.observations[0].private='raw',
    f=>f.report.cleanup.clients[0].idle=1,f=>f.report.principals[0].session_sha256='bad',
    f=>f.report.observations[0].wall_ms=true]) {
    const f=make();mutate(f);assert.throws(()=>checkColdFailureReport(f.report,f.pins,f.probe));
  }
  const early=make();early.report.stage='pins';early.report.failure='pins_invalid';early.report.pins=null;
  early.report.principals=[];early.report.observations=[];early.report.cold_before_direct=false;
  checkColdFailureReport(early.report,early.pins,early.probe);
});
