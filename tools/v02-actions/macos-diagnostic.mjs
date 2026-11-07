// Build-only diagnostic acquisition. Product and harness provenance stay separate.
import {execFileSync} from 'node:child_process';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {ArtifactTransport, boundedFile, childEnvironment, digest, requireThat as need, safeBlobUrl} from './transport.mjs';
import {checkRun, checkTooling, sourceDigest, verifyTools, probeTool} from './workflow.mjs';
import {verifyArtifactZip} from './publish.mjs';

export const POLICY = Object.freeze({repository:'Epiphytic/marmish', run:'42358105270', attempt:'1',
  source:'571d98b77780d265a2fb29d84ae160fec72ba37b', target:'aarch64-apple-darwin'});
export const ARTIFACTS = Object.freeze({
  product:Object.freeze({id:13570516008,name:'v02-candidate-aarch64-apple-darwin-42358105270-1',bytes:47988691,
    sha256:'f6c27bfab5b3dd2370eb38fa93f99f1c48ae906a7f04c6f45a59bf2bc758f970'}),
  tooling:Object.freeze({id:13570605471,name:'v02-installed-tools-aarch64-apple-darwin-42358105270-1',bytes:9460623,
    sha256:'6cf4a04d8ee549bd86d9f7a256fefd7cfdcdb68eaccc189ba5c03b2c1315895c'}),
  bootstrap:Object.freeze({id:13570810425,name:'v02-pinned-bootstrap-42358105270-1',bytes:3543,
    sha256:'974f86f238a8798ad2e565f300930070073cc5097748df07c30a9626c871d2ba'}),
});
export const ALLOWED_PATHS = Object.freeze([
  '.github/workflows/streaming-macos-diagnostic.yml',
  'tools/v02-actions/macos-diagnostic.mjs', 'tools/v02-actions/macos-diagnostic.test.mjs',
  'crates/marmish-native/src/installed_check.rs',
  'crates/marmish-native/src/installed_macos_diagnostic.rs',
  'crates/marmish-native/src/installed_macos_diagnostic_tests.rs',
  'tools/v02-actions/macos-diagnostic-policy.test.mjs', 'docs/agent-spec.md',
  'docs/validation/streaming-candidate-571d98b.json',
  'docs/validation/streaming-installed-macos-failure.json',
  'docs/phases/streaming-macos-diagnostic.md', 'docs/reviews/streaming-macos-diagnostic.md',
  'docs/validation/streaming-macos-diagnostic.json',
]);
export const COLD_ALLOWED_PATHS = Object.freeze([...ALLOWED_PATHS,
  'docs/validation/streaming-macos-diagnostic.md',
  'docs/validation/streaming-installed-e9686c0-failure.json',
  'docs/validation/streaming-candidate-e9686c0.json',
  'docs/validation/streaming-candidate-e9686c0.md',
  'docs/phases/streaming-macos-cold-multiprincipal.md',
  'docs/reviews/streaming-macos-cold-multiprincipal.md',
  'docs/validation/streaming-macos-cold-multiprincipal.json',
  'crates/marmish-native/src/installed_macos_cold_multiprincipal.rs',
  'crates/marmish-native/src/installed_macos_cold_multiprincipal_tests.rs',
  '.github/workflows/streaming-macos-cold-multiprincipal.yml',
]);
function allowedPaths(mode) {
  need(mode === 'original' || mode === 'cold','diagnostic_mode');
  return mode === 'cold' ? COLD_ALLOWED_PATHS : ALLOWED_PATHS;
}
const hex = (value,n) => typeof value === 'string' && new RegExp(`^[0-9a-f]{${n}}$`).test(value);
const pin = bytes => ({sha256:digest(bytes),bytes:bytes.length});
const gitDefault = args => execFileSync('git',args,{encoding:'utf8',env:childEnvironment(process.env),maxBuffer:2*1024*1024});
export function checkInputs(env) {
  need(env.GITHUB_REPOSITORY === POLICY.repository && env.GITHUB_REF === 'refs/heads/master'
    && hex(env.HARNESS_COMMIT,40) && env.HARNESS_COMMIT === env.GITHUB_SHA, 'diagnostic_source');
  for (const name of ['DIAGNOSTIC_ROOT','PROBE']) need(typeof env[name] === 'string'
    && path.isAbsolute(env[name]),'diagnostic_path');
}
export function checkChanges(entries,mode = 'original') {
  const allowed=allowedPaths(mode);
  need(entries.length > 0,'diagnostic_empty_delta');
  for (const [status,name,fileMode] of entries) need(['A','M'].includes(status)
    && allowed.includes(name) && fileMode === '100644','diagnostic_source_delta');
}
export function guardSource(env,git = gitDefault,mode = 'original') {
  const allowed=allowedPaths(mode);
  checkInputs(env);
  need(git(['rev-parse','HEAD']).trim() === env.HARNESS_COMMIT,'diagnostic_head');
  git(['merge-base','--is-ancestor',POLICY.source,env.HARNESS_COMMIT]);
  git(['merge-base','--is-ancestor',env.HARNESS_COMMIT,'refs/remotes/origin/master']);
  git(['diff','--quiet','HEAD','--']);
  const parts = git(['diff','--no-renames','--name-status','-z',POLICY.source,env.HARNESS_COMMIT]).split('\0');
  need(parts.pop() === '' && parts.length % 2 === 0,'diagnostic_source_delta');
  const entries = [];
  for (let i=0;i<parts.length;i+=2) {
    const name = parts[i+1];
    need(allowed.includes(name),'diagnostic_source_delta');
    const tree = git(['ls-tree',env.HARNESS_COMMIT,'--',name]);
    entries.push([parts[i],name,tree.startsWith('100644 blob ') ? '100644' : 'invalid']);
  }
  checkChanges(entries,mode);
}
export function checkCandidate(run,latest) {
  const c = {repository:POLICY.repository,run_id:POLICY.run,run_attempt:POLICY.attempt,source:POLICY.source};
  checkRun(run,c,undefined,['.github/workflows/streaming-candidate.yml']);
  need(run.event === 'workflow_dispatch' && run.head_sha === POLICY.source
    && String(latest.id) === POLICY.run && String(latest.run_attempt) === POLICY.attempt
    && latest.head_sha === POLICY.source,'diagnostic_candidate_changed');
}
export function selectArtifact(entries,kind) {
  const expected = ARTIFACTS[kind];
  need(expected && Array.isArray(entries),'diagnostic_artifact_kind');
  const found = entries.filter(entry=>entry.name === expected.name);
  need(found.length === 1,'diagnostic_artifact_missing');
  const entry=found[0];
  need(entry.id === expected.id && entry.size_in_bytes === expected.bytes
    && entry.digest === `sha256:${expected.sha256}` && entry.expired === false
    && String(entry.workflow_run?.id) === POLICY.run && entry.workflow_run?.head_sha === POLICY.source
    && entry.workflow_run?.head_branch === 'master','diagnostic_artifact_changed');
  return expected;
}
export function checkZip(bytes,kind) {
  const expected=ARTIFACTS[kind];
  need(expected && bytes.length === expected.bytes && digest(bytes) === expected.sha256,'diagnostic_zip_digest');
  return verifyArtifactZip(bytes);
}
function exactFiles(files,names) {
  need(files.size === names.length && names.every(name=>files.has(name)),'diagnostic_members');
}
export function checkProduct(files) {
  const names=[...files.keys()].filter(name=>name.endsWith('.manifest'));
  need(names.length === 1,'diagnostic_manifest_count');
  const bytes=files.get(names[0]);
  need(bytes.length <= 4096 && bytes.every(byte=>byte < 128),'diagnostic_manifest');
  const lines=bytes.toString('ascii').split('\n');
  const version=lines[1]?.replace(/^version /,'');
  need(/^0\.2\.(0|[1-9][0-9]{0,19})(-rc\.[1-9][0-9]{0,19})?$/.test(version)
    && lines.length === 13 && lines[12] === '' && lines[0] === 'marmish-manifest 1'
    && lines[1] === `version ${version}` && lines[2] === `target ${POLICY.target}`
    && lines[3] === `commit ${POLICY.source}` && lines[4] === 'signing adhoc notarized=no'
    && lines[5] === 'minimum macos=15.0' && /^state profile=[1-9][0-9]* journal=[1-9][0-9]* engine=[1-9][0-9]* config=[1-9][0-9]* commands=[1-9][0-9]*$/.test(lines[6])
    && lines[11] === 'end','diagnostic_manifest');
  const suffix=`${version}-${POLICY.target}`;
  need(names[0] === `marmish-${suffix}.manifest`,'diagnostic_manifest_name');
  const payloads=['marmish','marmishd','marmish-worker','THIRD-PARTY-LICENSES'];
  exactFiles(files,[names[0],...payloads.map(name=>`${name}-${suffix}`)]);
  for (const [i,name] of payloads.entries()) {
    const match=/^file ([0-9a-f]{64}) ([1-9][0-9]*) (0555|0444) ([A-Za-z0-9-]+)$/.exec(lines[i+7]);
    const payload=files.get(`${name}-${suffix}`);
    need(match && match[4] === name && match[3] === (i===3 ? '0444':'0555')
      && payload.length === Number(match[2]) && digest(payload) === match[1],'diagnostic_payload');
  }
  return {version,manifest:pin(bytes)};
}
export function checkBootstrap(bytes,template,version,manifestDigest) {
  const text=bytes.toString('utf8');
  need(Buffer.from(text).equals(bytes),'diagnostic_bootstrap');
  const fields={VERSION:version,SHA_MACOS:manifestDigest};
  for (const arch of ['AMD64','ARM64']) {
    const name=arch.toLowerCase();
    const matches=[...text.matchAll(new RegExp(`^    sha_${name}='([0-9a-f]{64})'$`,'gm'))];
    need(matches.length === 1,'diagnostic_bootstrap');
    fields[`SHA_${arch}`]=matches[0][1];
  }
  let rendered=template.toString('utf8');
  for (const [name,value] of Object.entries(fields)) {
    const token=`@MARMISH_${name}@`;
    need(rendered.split(token).length === 2,'diagnostic_bootstrap_template');
    rendered=rendered.replace(token,value);
  }
  need(!rendered.includes('@MARMISH_') && Buffer.from(rendered).equals(bytes),'diagnostic_bootstrap');
}
export function checkToolFiles(files,oldHarness) {
  exactFiles(files,['macos-arm64-tooling.json','marmish-installed-check','marmish-installed-fixture']);
  const recordBytes=files.get('macos-arm64-tooling.json');
  need(recordBytes.length <= 65536,'diagnostic_tooling');
  const record=JSON.parse(recordBytes);
  checkTooling(record,'macos-arm64',POLICY.source,oldHarness);
  for (const [name,expected] of Object.entries(record.files)) {
    const actual=pin(files.get(name));
    need(actual.bytes === expected.bytes && actual.sha256 === expected.sha256,'diagnostic_tooling_digest');
  }
  return {tooling_record:pin(recordBytes),fixture:pin(files.get('marmish-installed-fixture'))};
}
async function writeFiles(directory,files) {
  await fs.mkdir(directory,{mode:0o700});
  for (const [name,bytes] of files) await fs.writeFile(path.join(directory,name),bytes,{flag:'wx',mode:0o600});
}
export async function acquire(env,mode = 'original') {
  guardSource(env,gitDefault,mode);
  const root=env.DIAGNOSTIC_ROOT;
  const stat=await fs.lstat(root);
  need(stat.isDirectory() && !stat.isSymbolicLink() && stat.uid === process.getuid()
    && (stat.mode & 0o077) === 0 && await fs.realpath(root) === root,'diagnostic_root');
  const transport=new ArtifactTransport({context:{repository:POLICY.repository,run_id:POLICY.run},
    token:env.GH_TOKEN,budgetFile:path.join(root,'api-budget.json')});
  const deadline=Date.now()+120000;
  const json=async route=>{
    const reply=await transport.request(`https://api.github.com/repos/${POLICY.repository}${route}`,true,2*1024*1024,deadline);
    need(reply.bytes,'diagnostic_api_redirect');return JSON.parse(reply.bytes);
  };
  checkCandidate(await json(`/actions/runs/${POLICY.run}/attempts/${POLICY.attempt}`),await json(`/actions/runs/${POLICY.run}`));
  const entries=await transport.list(deadline);
  const groups={};
  for (const kind of Object.keys(ARTIFACTS)) {
    const expected=selectArtifact(entries,kind);
    const redirect=await transport.request(`https://api.github.com/repos/${POLICY.repository}/actions/artifacts/${expected.id}/zip`,true,expected.bytes,deadline);
    need(redirect.redirect,'diagnostic_artifact_redirect');
    const reply=await transport.request(safeBlobUrl(redirect.redirect),false,expected.bytes,deadline);
    need(reply.bytes,'diagnostic_artifact_body');groups[kind]=checkZip(reply.bytes,kind);
  }
  const product=checkProduct(groups.product),oldHarness=sourceDigest(POLICY.source);
  const tools=checkToolFiles(groups.tooling,oldHarness);
  exactFiles(groups.bootstrap,['install.sh']);
  const bootstrap=groups.bootstrap.get('install.sh');
  checkBootstrap(bootstrap,Buffer.from(gitDefault(['show',`${POLICY.source}:scripts/install-native.sh`])),product.version,product.manifest.sha256);
  // Verify the newly compiled diagnostic separately; never run the old driver.
  probeTool(env.PROBE,env.HARNESS_COMMIT,POLICY.target);
  const probe=pin(await boundedFile(env.PROBE,256*1024*1024));
  await writeFiles(path.join(root,'assets'),groups.product);
  await writeFiles(path.join(root,'tools'),groups.tooling);
  await verifyTools(path.join(root,'tools'),'macos-arm64',POLICY.source,oldHarness,tools.tooling_record.sha256);
  await writeFiles(path.join(root,'bootstrap'),groups.bootstrap);
  const pins={schema:1,kind:'macos-stream-diagnostic-pins',qualification:false,repository:POLICY.repository,
    candidate_run_id:POLICY.run,candidate_attempt:POLICY.attempt,candidate_commit:POLICY.source,
    harness_commit:env.HARNESS_COMMIT,target:POLICY.target,version:product.version,old_harness_sha256:oldHarness,
    artifacts:ARTIFACTS,manifest:product.manifest,...tools,bootstrap:pin(bootstrap)};
  const pinBytes=Buffer.from(JSON.stringify(pins)+'\n');
  await fs.writeFile(path.join(root,'pins.json'),pinBytes,{flag:'wx',mode:0o600});
  await fs.writeFile(path.join(root,'provenance.json'),JSON.stringify({schema:1,kind:'macos-stream-diagnostic-provenance',
    qualification:false,pins_sha256:digest(pinBytes),product:pins,
    harness:{commit:env.HARNESS_COMMIT,source_sha256:sourceDigest(env.HARNESS_COMMIT),target:POLICY.target,probe}})+'\n',
  {flag:'wx',mode:0o600});
}
function closed(value, fields) {
  need(value !== null && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).sort().join('\0') === [...fields].sort().join('\0'),'diagnostic_record_fields');
}
const canonical = value => JSON.stringify(value && typeof value === 'object'
  ? Array.isArray(value) ? value.map(v=>JSON.parse(canonical(v)))
    : Object.fromEntries(Object.keys(value).sort().map(k=>[k,JSON.parse(canonical(value[k]))])) : value);
const equal = (a,b) => canonical(a) === canonical(b);
function filePin(value) {
  closed(value,['sha256','bytes']);need(hex(value.sha256,64)
    && Number.isSafeInteger(value.bytes) && value.bytes > 0 && value.bytes <= 268435456,'diagnostic_file_pin');
}
export function checkPins(pins,harness) {
  closed(pins,['schema','kind','qualification','repository','candidate_run_id','candidate_attempt',
    'candidate_commit','harness_commit','target','version','old_harness_sha256','artifacts',
    'manifest','tooling_record','bootstrap','fixture']);
  need(pins.schema === 1 && pins.kind === 'macos-stream-diagnostic-pins' && pins.qualification === false
    && pins.repository === POLICY.repository && pins.candidate_run_id === POLICY.run
    && pins.candidate_attempt === POLICY.attempt && pins.candidate_commit === POLICY.source
    && pins.harness_commit === harness && pins.target === POLICY.target
    && typeof pins.version === 'string' && /^0\.2\.[0-9]+(?:-rc\.[0-9]+)?$/.test(pins.version)
    && hex(pins.old_harness_sha256,64) && equal(pins.artifacts,ARTIFACTS),'diagnostic_pins');
  for (const key of ['manifest','tooling_record','bootstrap','fixture']) filePin(pins[key]);
}
const observationFields = ['case','stage','failure','exit','signal','input_bytes','input_sha256',
  'stdin_written','stdout_bytes','stdout_sha256','stderr_bytes','stderr_sha256','input_epipe',
  'capture_overflow','capture_io','timed_out','interrupted','owned_group_clean','stream_present',
  'binding_equal','client_terminal','server_terminal','receipt_present','receipt_valid','output_match',
  'offsets_available','wall_ms'];
export function requestedConcurrency(value) {
  need(value === 'true' || value === 'false','diagnostic_concurrency');
  return value === 'true';
}
export function checkReport(report,complete,reportBytes,pins,probe,concurrent) {
  need(typeof concurrent === 'boolean','diagnostic_concurrency');
  closed(report,['schema','kind','qualification','same_host_only','relay','pins','probe_sha256',
    'stage','failure','observations','cleanup','passed']);
  closed(complete,['schema','kind','report_sha256','passed']);
  need(complete.schema === 1 && complete.kind === 'macos-diagnostic-complete'
    && complete.passed === true && complete.report_sha256 === digest(reportBytes),'diagnostic_complete');
  need(report.schema === 1 && report.kind === 'macos-stream-diagnostic' && report.qualification === false
    && report.same_host_only === true && report.relay === 'wss://relay.us.whitenoise.chat'
    && report.passed === true && report.failure === null && report.stage === 'stream_capture'
    && equal(report.pins,pins) && report.probe_sha256 === probe.sha256,'diagnostic_report');
  const cleanup=['client_cancel','server_cancel','client_idle','server_idle','client_stop','server_stop'];
  closed(report.cleanup,cleanup);need(cleanup.every(k=>report.cleanup[k] === true),'diagnostic_cleanup');
  need(Array.isArray(report.observations),'diagnostic_observations');
  const expected=['fixture_direct','raw','zlib'];
  if (concurrent) expected.push('concurrent-1','concurrent-2','concurrent-3');
  need(equal(report.observations.map(o=>o.case).sort(),expected.sort()),'diagnostic_cases');
  report.observations.forEach(o=>checkObservation(o));
}
function checkObservation(o,direct = o.case === 'fixture_direct') {
  closed(o,observationFields);
  need(o.stage === (direct ? 'fixture_direct' : 'receipt') && o.failure === null && o.exit === 7
    && o.signal === null && o.input_bytes === 98304 && o.stdin_written === 98304
    && o.stdout_bytes === 98304 && Number.isSafeInteger(o.stderr_bytes)
    && (direct ? o.stderr_bytes === 8192 : o.stderr_bytes > 8192 && o.stderr_bytes <= 1048576)
    && Number.isSafeInteger(o.wall_ms) && o.wall_ms >= 0 && o.wall_ms <= 1080000,'diagnostic_capture');
  for (const k of ['input_sha256','stdout_sha256','stderr_sha256']) need(hex(o[k],64),'diagnostic_hash');
  for (const k of ['input_epipe','capture_overflow','capture_io','timed_out','interrupted','offsets_available']) {
    need(o[k] === false,'diagnostic_capture_flag');
  }
  for (const k of ['owned_group_clean','receipt_present','receipt_valid','output_match']) need(o[k] === true,'diagnostic_success');
  need(o.stream_present === !direct && o.binding_equal === !direct,'diagnostic_binding');
  const terminal=direct ? null : {kind:'exited',data:{code:7}};
  need(equal(o.client_terminal,terminal) && equal(o.server_terminal,terminal),'diagnostic_terminal');
}

const stages=['pins','install','fixture_direct','setup','server_ready','client_ready','stream_spawn',
  'stream_capture','stream_parse','client_journal','server_journal','receipt','idle','stop','report'];
const categories=['pins_invalid','install_failed','setup_failed','readiness_timeout','input_epipe',
  'capture_io','capture_overflow','child_nonzero','child_signal','stream_id_absent','stream_id_malformed',
  'list_absent','list_ambiguous','binding_mismatch','terminal_mismatch','hash_mismatch','length_mismatch',
  'receipt_absent','receipt_malformed','budget_exhausted','interrupted','cleanup_unconfirmed','report_failed'];
function boundedInt(value,max) {return Number.isSafeInteger(value) && value >= 0 && value <= max;}
function checkTerminal(value) {
  if(value === null) return;
  need(value && typeof value === 'object','diagnostic_terminal');
  if(value.kind === 'cancelled') {closed(value,['kind']);return;}
  closed(value,['kind','data']);
  const key={exited:'code',signaled:'signal',failed:'code'}[value.kind];
  need(key,'diagnostic_terminal');closed(value.data,[key]);
  need(value.kind === 'failed' ? ['unsupported','invalid','denied','conflict','gap','expired','capacity',
    'uncertain','not_found','closed'].includes(value.data.code) : boundedInt(value.data[key],255),'diagnostic_terminal');
}
export function checkFailureMarker(marker) {
  closed(marker,['schema','kind','failure']);
  need(marker.schema === 1 && marker.kind === 'macos-diagnostic-failure'
    && marker.failure === 'report_failed','diagnostic_failure_marker');
}
export function checkFailureReport(report,pins,probe,concurrent) {
  closed(report,['schema','kind','qualification','same_host_only','relay','pins','probe_sha256',
    'stage','failure','observations','cleanup','passed']);
  need(typeof concurrent === 'boolean' && report.schema === 1 && report.kind === 'macos-stream-diagnostic'
    && report.qualification === false && report.same_host_only === true
    && report.relay === 'wss://relay.us.whitenoise.chat' && report.passed === false
    && categories.includes(report.failure) && stages.includes(report.stage)
    && report.probe_sha256 === probe.sha256,'diagnostic_failed_report');
  need(equal(report.pins,pins) || (report.pins === null && report.stage === 'pins'
    && report.failure === 'pins_invalid'),'diagnostic_failed_pins');
  const cleanup=['client_cancel','server_cancel','client_idle','server_idle','client_stop','server_stop'];
  closed(report.cleanup,cleanup);
  need(cleanup.every(k=>typeof report.cleanup[k] === 'boolean'),'diagnostic_cleanup');
  const expected=['fixture_direct','raw','zlib',...(concurrent?['concurrent-1','concurrent-2','concurrent-3']:[])];
  need(Array.isArray(report.observations) && report.observations.length <= expected.length,'diagnostic_observations');
  const seen=new Set();
  for(const o of report.observations) {
    closed(o,observationFields);
    need(expected.includes(o.case) && !seen.has(o.case) && stages.includes(o.stage)
      && (o.failure === null || categories.includes(o.failure)),'diagnostic_failed_observation');seen.add(o.case);
    checkFailureObservation(o);
  }
}

function checkFailureObservation(o) {
  closed(o,observationFields);
  need(stages.includes(o.stage) && (o.failure === null || categories.includes(o.failure)),
    'diagnostic_failed_observation');
    need((o.exit === null || Number.isInteger(o.exit) && o.exit >= -2147483648 && o.exit <= 2147483647)
      && (o.signal === null || boundedInt(o.signal,2147483647)),'diagnostic_failed_status');
    for(const k of ['input_bytes','stdin_written','stdout_bytes','stderr_bytes']) need(boundedInt(o[k],1048576),'diagnostic_count');
    need(o.stdin_written <= o.input_bytes && boundedInt(o.wall_ms,1080000),'diagnostic_count');
    for(const k of ['input_sha256','stdout_sha256','stderr_sha256']) need(hex(o[k],64),'diagnostic_hash');
    for(const k of ['input_epipe','capture_overflow','capture_io','timed_out','interrupted','owned_group_clean',
      'stream_present','binding_equal','receipt_present','receipt_valid','output_match']) need(typeof o[k] === 'boolean','diagnostic_flag');
    need(o.offsets_available === false,'diagnostic_offsets');
    checkTerminal(o.client_terminal);checkTerminal(o.server_terminal);
}

const coldCases=['cold-1','cold-2','cold-3','direct-after'];
const coldFields=['schema','kind','qualification','same_host_only','relay','pins','probe_sha256',
  'stage','failure','observations','cleanup','passed','scenario','cold_before_direct',
  'local_outbound_fixture_traffic','principals'];
function checkColdBase(report,pins,probe,passed) {
  closed(report,coldFields);
  need(report.schema === 1 && report.kind === 'macos-cold-multiprincipal-diagnostic'
    && report.qualification === false && report.same_host_only === true
    && report.relay === 'wss://relay.us.whitenoise.chat' && report.passed === passed
    && report.probe_sha256 === probe.sha256 && report.scenario === 'cold-three-principals'
    && typeof report.cold_before_direct === 'boolean' && report.local_outbound_fixture_traffic === false,
  'diagnostic_cold_report');
  need(equal(report.pins,pins) || (!passed && report.pins === null && report.stage === 'pins'
    && report.failure === 'pins_invalid'),'diagnostic_failed_pins');
  closed(report.cleanup,['clients','server']);
  need(Array.isArray(report.cleanup.clients) && report.cleanup.clients.length === 3,'diagnostic_cleanup');
  for(const service of [...report.cleanup.clients,report.cleanup.server]) {
    closed(service,['cancel','idle','stop']);
    need(Object.values(service).every(v=>passed ? v === true : typeof v === 'boolean'),'diagnostic_cleanup');
  }
  need(Array.isArray(report.principals) && (passed ? report.principals.length === 3
    : report.principals.length === 0 || report.principals.length === 3),'diagnostic_principals');
  const roles=['linux-amd64','linux-arm64','macos-arm64'];
  for(const [i,principal] of report.principals.entries()) {
    closed(principal,['label','role','account_sha256','session_sha256','group_sha256','open_sha256','pair_valid']);
    need(principal.label === `client-${i+1}` && principal.role === roles[i]
      && hex(principal.account_sha256,64) && typeof principal.pair_valid === 'boolean','diagnostic_principal');
    for(const key of ['session_sha256','group_sha256','open_sha256']) {
      need(hex(principal[key],64) || (!passed && principal[key] === null),'diagnostic_principal_binding');
    }
    if(passed) need(principal.pair_valid === true,'diagnostic_pair');
  }
  if(passed) for(const key of ['account_sha256','session_sha256','group_sha256','open_sha256']) {
    need(new Set(report.principals.map(p=>p[key])).size === 3,'diagnostic_distinct_principals');
  }
}
export function checkColdReport(report,complete,reportBytes,pins,probe) {
  checkColdBase(report,pins,probe,true);
  closed(complete,['schema','kind','report_sha256','passed']);
  need(complete.schema === 1 && complete.kind === 'macos-diagnostic-complete'
    && complete.passed === true && complete.report_sha256 === digest(reportBytes),'diagnostic_complete');
  need(report.failure === null && report.stage === 'fixture_direct' && report.cold_before_direct === true
    && Array.isArray(report.observations) && equal(report.observations.map(o=>o.case),coldCases),'diagnostic_cold_order');
  report.observations.forEach(o=>checkObservation(o,o.case === 'direct-after'));
  need(report.observations.slice(0,3).every(o=>o.wall_ms <= 330000)
    && report.observations[3].wall_ms <= 15000,'diagnostic_cold_wall');
}
export function checkColdFailureReport(report,pins,probe) {
  checkColdBase(report,pins,probe,false);
  need([...categories,'topology_mismatch'].includes(report.failure) && stages.includes(report.stage),
    'diagnostic_failed_report');
  need(Array.isArray(report.observations) && report.observations.length <= 4,'diagnostic_observations');
  let previous=-1;
  for(const o of report.observations) {
    const index=coldCases.indexOf(o.case);
    need(index > previous && index >= 0,'diagnostic_cold_order');previous=index;
    checkFailureObservation(o);
  }
  if(report.observations.some(o=>o.case === 'direct-after')) need(report.cold_before_direct === true
    && equal(report.observations.map(o=>o.case),coldCases),'diagnostic_cold_order');
}

export async function validateReport(env,mode = 'original') {
  allowedPaths(mode);
  checkInputs(env);
  const concurrent=mode === 'cold' ? false : requestedConcurrency(env.RUN_CONCURRENT);
  const root=env.DIAGNOSTIC_ROOT;
  let failureMarker=false;
  try { await fs.lstat(path.join(root,'output/failure.json'));failureMarker=true; }
  catch(error) { if(error.code !== 'ENOENT') throw error; }
  const read=async(name,limit)=>boundedFile(path.join(root,name),limit);
  const pinsBytes=await read('pins.json',16384),pins=JSON.parse(pinsBytes);
  checkPins(pins,env.HARNESS_COMMIT);
  const provenance=JSON.parse(await read('provenance.json',65536));
  closed(provenance,['schema','kind','qualification','pins_sha256','product','harness']);
  closed(provenance.harness,['commit','source_sha256','target','probe']);
  filePin(provenance.harness.probe);
  need(provenance.schema === 1 && provenance.kind === 'macos-stream-diagnostic-provenance'
    && provenance.qualification === false && provenance.pins_sha256 === digest(pinsBytes)
    && equal(provenance.product,pins) && provenance.harness.commit === env.HARNESS_COMMIT
    && provenance.harness.source_sha256 === sourceDigest(env.HARNESS_COMMIT)
    && provenance.harness.target === POLICY.target,'diagnostic_provenance');
  const probe=pin(await boundedFile(env.PROBE,268435456));
  need(equal(probe,provenance.harness.probe),'diagnostic_probe_changed');
  const reportBytes=await read('output/report.json',1048576),report=JSON.parse(reportBytes);
  if(failureMarker) checkFailureMarker(JSON.parse(await read('output/failure.json',16384)));
  if(report.passed === false) {
    if(mode === 'cold') checkColdFailureReport(report,pins,probe);
    else checkFailureReport(report,pins,probe,concurrent);
    throw new Error('diagnostic_failed_report');
  }
  const complete=JSON.parse(await read('output/complete.json',16384));
  if(mode === 'cold') checkColdReport(report,complete,reportBytes,pins,probe);
  else checkReport(report,complete,reportBytes,pins,probe,concurrent);
  need(!failureMarker,'diagnostic_failure_marker');
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args=process.argv.slice(2);
  const commands={'validate-report':validateReport,'acquire-cold':env=>acquire(env,'cold'),
    'validate-cold-report':env=>validateReport(env,'cold')};
  const run=args.length === 0 ? acquire : args.length === 1 && Object.hasOwn(commands,args[0]) ? commands[args[0]] : null;
  Promise.resolve().then(()=>{need(run,'diagnostic_arguments');return run(process.env);})
    .catch(()=>{console.error('marmish macOS diagnostic: provenance verification failed');process.exitCode=1;});
}
