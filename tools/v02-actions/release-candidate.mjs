// Read-only release qualification. Artifact bytes never execute in this process.
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {pathToFileURL} from 'node:url';
import {verifyArtifactZip} from './publish.mjs';
import {ArtifactTransport, boundedFile, digest, safeBlobUrl, childEnvironment, ROLES} from './transport.mjs';
import {checkRun, checkTooling, sourceDigest} from './workflow.mjs';
import {loadInspection} from './release-render.mjs';
import {verifyEligibility, versionAt} from './eligibility.mjs';
import {TARGETS, REPOSITORY, HEX40, HEX64, MAX_ASSET, validId, parseVersion, exactKeys, canonicalInventory, releaseNames, requireRelease as need, validateEligibility} from './release-schema.mjs';
const LIMIT = 800 * 1024 * 1024;
export async function unpack(zip, expected) {
  need(Buffer.isBuffer(zip) && zip.length <= LIMIT, 'archive_bound');
  const files = verifyArtifactZip(zip);
  need(files.size === expected.length && expected.every(name => files.has(name)), 'invalid_archive');
  return files;
}
export function expectedArtifacts(run, attempt) {
  const suffix = `${run}-${attempt}`;
  return [`v02-eligibility-${suffix}`, ...TARGETS.flatMap(t => [`v02-candidate-${t}-${suffix}`, `v02-installed-tools-${t}-${suffix}`, `v02-quickstart-${t}-${suffix}`, `v02-provider-${t}-${suffix}`]), `v02-render-tool-${suffix}`, `v02-pinned-bootstrap-${suffix}`];
}
const CANDIDATE_WORKFLOW = '.github/workflows/v02-release.yml';
const WORKFLOW_LIMIT = 256 * 1024;
const LAYOUT_MARKER = 'MARMISH_CANDIDATE_LAYOUT';
export async function candidateLayout(source, git) {
  need(HEX40.test(source), 'invalid_candidate_layout');
  const entry = await git(['ls-tree', source, '--', CANDIDATE_WORKFLOW], {maxBuffer: 1024});
  const match = /^100644 blob ([0-9a-f]{40})\t\.github\/workflows\/v02-release\.yml\n?$/.exec(entry);
  need(match && match[0] === entry, 'invalid_candidate_layout');
  const sizeText = await git(['cat-file', '-s', match[1]], {maxBuffer: 128});
  need(/^[1-9][0-9]*\n?$/.test(sizeText), 'invalid_candidate_layout');
  const size = Number(sizeText.trim());
  need(Number.isSafeInteger(size) && size <= WORKFLOW_LIMIT
    && (sizeText === String(size) || sizeText === `${size}\n`), 'invalid_candidate_layout');
  const text = await git(['show', `${source}:${CANDIDATE_WORKFLOW}`], {maxBuffer: WORKFLOW_LIMIT});
  need(typeof text === 'string' && Buffer.byteLength(text) === size
    && !text.includes('\uFEFF') && !text.includes('\r'), 'invalid_candidate_layout');
  const count = text.split(LAYOUT_MARKER).length - 1;
  if (count === 0) return 'legacy-v1';
  need(count === 1, 'invalid_candidate_layout');
  const lines = text.split('\n'), environments = lines.flatMap((line, index) => line === 'env:' ? [index] : []);
  need(environments.length === 1, 'invalid_candidate_layout');
  const start = environments[0];
  const next = lines.findIndex((line, index) => index > start && /^[^\s]/.test(line));
  const end = next === -1 ? lines.length : next;
  const marker = lines.findIndex(line => line.includes(LAYOUT_MARKER));
  need(marker > start && marker < end && lines[marker] === `  ${LAYOUT_MARKER}: split-v1`, 'invalid_candidate_layout');
  return 'split-v1';
}
export function checkJobs(jobs, options) {
  need(options && ['legacy-v1', 'split-v1'].includes(options.layout) && typeof options.skip === 'boolean', 'invalid_candidate_layout');
  need(Array.isArray(jobs), 'invalid_jobs');
  const {layout, skip} = options;
  const runners = ['ubuntu-24.04', 'ubuntu-24.04-arm', 'macos-15'];
  const names = ['release-eligibility', 'render', ...TARGETS.flatMap((t, i) => [
    `package (${runners[i]}, ${t}, ${i === 2 ? 'adhoc, macos=15.0' : 'none, glibc=2.39'})`,
    ...(layout === 'split-v1' ? [`installed-tools (${runners[i]}, ${t})`] : []),
    `install-quickstart (${runners[i]}, ${t})`, `provider-qualification (${runners[i]}, ${t})`])];
  if (skip) {
    checkSkippedJobs(jobs, names, layout);
    return;
  }
  need(jobs.length === names.length && names.every(name => {
    const selected = jobs.filter(j => j.name === name);
    return selected.length === 1 && selected[0].status === 'completed' && selected[0].conclusion === 'success';
  }), 'candidate_jobs_incomplete');
}
function checkSkippedJobs(jobs, names, layout) {
  const eligibility = jobs.filter(job => job.name === 'release-eligibility');
  need(eligibility.length === 1 && eligibility[0].status === 'completed' && eligibility[0].conclusion === 'success'
    && new Set(jobs.map(job => job.name)).size === jobs.length, 'candidate_jobs_incomplete');
  const remaining = jobs.filter(job => job.name !== 'release-eligibility');
  need(remaining.every(job => job.status === 'completed' && job.conclusion === 'skipped'), 'candidate_jobs_incomplete');
  const expected = ['render'];
  for (const category of ['package', ...(layout === 'split-v1' ? ['installed-tools'] : []), 'install-quickstart','provider-qualification']) {
    const expanded = names.filter(name => name.startsWith(`${category} (`));
    const actual = remaining.filter(job => job.name === category || expanded.includes(job.name));
    const collapsed = actual.length === 1 && actual[0].name === category;
    need(collapsed || (actual.length === expanded.length && expanded.every(name => actual.some(job => job.name === name))), 'candidate_jobs_incomplete');
    expected.push(...(collapsed ? [category] : expanded));
  }
  need(remaining.length === expected.length && expected.every(name => remaining.some(job => job.name === name)), 'candidate_jobs_incomplete');
}
function checkQuickstart(value, common, minor, relaySelection) {
  exactKeys(value, [...common,'key_storage','role','inbound_enabled','prompts_completed','identity_created','repeat_same_identity','repeat_owner_opened','stop_complete', ...(minor === 3 ? ['relay_selection'] : [])]);
  if (minor === 3) need(['explicit', 'default'].includes(relaySelection) && value.relay_selection === relaySelection, 'quickstart_relay_selection');
  need(value.kind === 'installed_quickstart_pty' && value.key_storage === 'file' && value.role === 'both'
    && value.inbound_enabled === false && value.prompts_completed === 5 && value.identity_created === true
    && value.repeat_same_identity === true && value.repeat_owner_opened === false && value.stop_complete === true, 'quickstart_failed');
}
export function checkReport(value, kind, {target, source, run, attempt, version = '0.2.3', relaySelection = 'explicit'}) {
  const parsed = parseVersion(version);
  need(parsed, 'invalid_release_version');
  const common = ['schema','kind','ok','python_in_runtime_path','network_activation','owned_state_removed','elapsed_ms'];
  if (kind === 'quickstart') {
    checkQuickstart(value, common, parsed.minor, relaySelection);
  } else {
    const platform = target.endsWith('darwin') ? ['owned_account_removed','runner_keychain_unchanged','supervisor_interpreter_unchanged'] : [];
    // The supervisor preserves the inner fixture's finite failure field on macOS.
    exactKeys(value, [...common,'owned_children_reaped','auto_keychain','distinct_named_identities','fresh_reload','item_count',
      'locked_refused','unlocked_fresh_reload','no_plaintext_fallback', ...(target.endsWith('darwin') ? [] : ['absent_provider_files']),
      'source_commit','target','run_id','run_attempt','provider_packages', ...platform]);
    need(value.kind === 'installed_auto_provider' && value.source_commit === source && value.target === target
      && value.run_id === run && value.run_attempt === attempt && value.item_count === 4
      && ['owned_children_reaped','auto_keychain','distinct_named_identities','fresh_reload','locked_refused',
        'unlocked_fresh_reload','no_plaintext_fallback', ...(target.endsWith('darwin') ? [] : ['absent_provider_files']), ...platform].every(k => value[k] === true)
      && Array.isArray(value.provider_packages) && value.provider_packages.length <= 16
      && value.provider_packages.every(p => typeof p === 'string' && /^[A-Za-z0-9:+.=_~-]{1,200}$/.test(p)), 'provider_failed');
  }
  need(value.schema === 1 && value.ok === true && value.python_in_runtime_path === false
    && value.network_activation === false && value.owned_state_removed === true
    && Number.isSafeInteger(value.elapsed_ms) && value.elapsed_ms >= 0 && value.elapsed_ms < 900000, 'qualification_failed');
}
export function releaseFormats(version) {
  const parsed = parseVersion(version);
  need(parsed, 'invalid_release_version');
  return [2, parsed.minor === 2 ? 6 : 7, 1, 1, 1];
}
export function checkManifest(bytes, files, version, source, target) {
  need(bytes.length <= 4096, 'manifest_bound');
  const text = bytes.toString('ascii'), lines = text.split('\n');
  need(Buffer.from(text, 'ascii').equals(bytes) && lines.length === 13 && lines[12] === ''
    && lines[0] === 'marmish-manifest 1' && lines[1] === `version ${version}` && lines[2] === `target ${target}`
    && lines[3] === `commit ${source}` && lines[4] === `signing ${target.endsWith('darwin') ? 'adhoc' : 'none'} notarized=no`
    && lines[5] === `minimum ${target.endsWith('darwin') ? 'macos=15.0' : 'glibc=2.39'}`
    && lines[6] === `state ${['profile','journal','engine','config','commands'].map((key, i) => `${key}=${releaseFormats(version)[i]}`).join(' ')}` && lines[11] === 'end', 'manifest_provenance');
  for (const [i, prefix] of ['marmish','marmishd','marmish-worker','THIRD-PARTY-LICENSES'].entries()) {
    const name = `${prefix}-${version}-${target}`, payload = files.get(name);
    const match = /^file ([0-9a-f]{64}) ([1-9][0-9]*) (0555|0444) ([A-Za-z0-9._-]+)$/.exec(lines[i + 7]);
    need(match && match[4] === prefix && match[3] === (i === 3 ? '0444' : '0555') && payload
      && payload.length === Number(match[2]) && digest(payload) === match[1], 'manifest_payload');
  }
}
async function download(transport, artifact, names, context, deadline) {
  need(artifact && !artifact.expired && Number.isSafeInteger(artifact.id) && artifact.id > 0
    && Number.isSafeInteger(artifact.size_in_bytes) && artifact.size_in_bytes > 0 && artifact.size_in_bytes <= LIMIT
    && /^sha256:[0-9a-f]{64}$/.test(artifact.digest) && String(artifact.workflow_run?.id) === context.run_id
    && artifact.workflow_run?.head_sha === context.source && artifact.workflow_run?.head_branch === 'master', 'artifact_provenance');
  const reply = await transport.request(`https://api.github.com/repos/${REPOSITORY}/actions/artifacts/${artifact.id}/zip`, true, LIMIT, deadline);
  need(reply.redirect, 'artifact_redirect_missing');
  const blob = await transport.request(safeBlobUrl(reply.redirect), false, LIMIT, deadline);
  need(blob.bytes && `sha256:${digest(blob.bytes)}` === artifact.digest, 'artifact_digest_mismatch');
  return unpack(blob.bytes, names);
}
function reportFiles(kind, version) {
  return [`${kind}.json`, ...(kind === 'quickstart' && parseVersion(version).minor === 3 ? ['quickstart-default-relays.json'] : [])];
}
function inspectReports(files, kind, context) {
  return [...files].map(([name, bytes]) => {
    const isDefault = name === 'quickstart-default-relays.json';
    checkReport(JSON.parse(bytes), kind, {...context, relaySelection: isDefault ? 'default' : 'explicit'});
    return {name: context.artifactName + (isDefault ? '-default-relays' : ''), sha256: digest(bytes)};
  });
}
export async function inspectCandidate(input, {api, git, transport, harnessDigest = sourceDigest}) {
  need(validId(input.run_id) && validId(input.run_attempt) && HEX40.test(input.publisher_sha), 'invalid_candidate_selection');
  const base = `/repos/${REPOSITORY}`;
  const run = await api(`${base}/actions/runs/${input.run_id}/attempts/${input.run_attempt}`);
  const context = {...input, repository: REPOSITORY, source: run.head_sha};
  checkRun(run, context);
  const latest = await api(`${base}/actions/runs/${input.run_id}`);
  need(latest.run_attempt === run.run_attempt, 'candidate_attempt_changed');
  const eligibilityName = `v02-eligibility-${input.run_id}-${input.run_attempt}`;
  const listed = await transport.list(Date.now() + 120000);
  need(Array.isArray(listed), 'invalid_artifact_list');
  // GitHub lists every rerun's artifacts together; trust only this whole attempt.
  const suffix = `-${input.run_id}-${input.run_attempt}`;
  const artifacts = listed.filter(artifact => typeof artifact.name === 'string' && artifact.name.endsWith(suffix));
  need(artifacts.length <= 100 && new Set(artifacts.map(a => a.id)).size === artifacts.length
    && new Set(artifacts.map(a => a.name)).size === artifacts.length, 'duplicate_artifact');
  const take = async (name, names) => download(transport, artifacts.find(a => a.name === name), names, context, Date.now() + 120000);
  const eligibilityBytes = (await take(eligibilityName, ['eligibility.json'])).get('eligibility.json');
  const eligibility = validateEligibility(JSON.parse(eligibilityBytes));
  need(eligibility.source === context.source && eligibility.run_id === input.run_id && eligibility.run_attempt === input.run_attempt
    && eligibility.event === run.event && eligibility.caveated === false, 'eligibility_binding');
  await verifyEligibility(eligibility, {api, git, allowSelectedTag: true});
  const jobsReply = await api(`${base}/actions/runs/${input.run_id}/attempts/${input.run_attempt}/jobs?per_page=100`);
  need(jobsReply.total_count === jobsReply.jobs?.length && jobsReply.total_count <= 100, 'jobs_bound');
  const skip = eligibility.decision === 'skip-by-design';
  checkJobs(jobsReply.jobs, {layout: await candidateLayout(context.source, git), skip});
  if (skip) return {skip: true, reason: eligibility.reason};
  need(eligibility.decision === 'build', 'candidate_ineligible');
  const names = expectedArtifacts(input.run_id, input.run_attempt);
  need(artifacts.length === names.length && names.every(n => artifacts.some(a => a.name === n)), 'candidate_artifact_inventory');
  const version = await versionAt(context.source, git);
  need(version === eligibility.version, 'candidate_version_changed');
  const payload = new Map(), reports = [{name: eligibilityName, sha256: digest(eligibilityBytes)}];
  for (const [index, target] of TARGETS.entries()) {
    const suffix = `${target}-${input.run_id}-${input.run_attempt}`;
    const productNames = releaseNames(version).filter(n => n.endsWith(target) || n.endsWith(`${target}.manifest`));
    const product = await take(`v02-candidate-${suffix}`, productNames);
    checkManifest(product.get(`marmish-${version}-${target}.manifest`), product, version, context.source, target);
    for (const [name, bytes] of product) payload.set(name, bytes);
    const tooling = await take(`v02-installed-tools-${suffix}`, [`${ROLES[index]}-tooling.json`, 'marmish-installed-check','marmish-installed-fixture']);
    const record = JSON.parse(tooling.get(`${ROLES[index]}-tooling.json`));
    checkTooling(record, ROLES[index], context.source, harnessDigest(context.source));
    for (const [name, pin] of Object.entries(record.files)) need(digest(tooling.get(name)) === pin.sha256 && tooling.get(name).length === pin.bytes, 'tooling_digest');
    for (const kind of ['quickstart','provider']) {
      const artifactName = `v02-${kind}-${suffix}`;
      const files = await take(artifactName, reportFiles(kind, version));
      reports.push(...inspectReports(files, kind, {artifactName, target, source: context.source,
        run: input.run_id, attempt: input.run_attempt, version}));
    }
  }
  const bootstrap = (await take(`v02-pinned-bootstrap-${input.run_id}-${input.run_attempt}`, ['install.sh'])).get('install.sh');
  need(bootstrap.length > 0 && bootstrap.length <= 131072, 'bootstrap_bound');
  const renderer = (await take(`v02-render-tool-${input.run_id}-${input.run_attempt}`, ['marmish-release-tool'])).get('marmish-release-tool');
  const inventory = {schema: 1, repository: REPOSITORY, publisher_sha: context.source, source: context.source, version,
    candidate: {run_id: input.run_id, run_attempt: input.run_attempt},
    artifacts: artifacts.map(a => ({id: a.id,name: a.name,digest: a.digest})),
    assets: [...payload].map(([name, bytes]) => ({name,size: bytes.length,sha256: digest(bytes)})),
    bootstrap_sha256: digest(bootstrap), reports};
  canonicalInventory(inventory);
  return {skip: false, inventory, payload, bootstrap, renderer};
}
export async function writeInspection(result, root) {
  await fs.mkdir(root, {recursive: false, mode: 0o700});
  if (result.skip) {
    const bytes = Buffer.from(JSON.stringify({schema: 1,skip: true,reason: result.reason}) + '\n');
    await fs.writeFile(path.join(root,'inspection.json'), bytes, {flag:'wx'}); return digest(bytes);
  }
  await fs.mkdir(path.join(root,'assets')); await fs.mkdir(path.join(root,'renderer')); await fs.mkdir(path.join(root,'bootstrap'));
  for (const [name, bytes] of result.payload) await fs.writeFile(path.join(root,'assets',name), bytes, {flag:'wx'});
  await fs.writeFile(path.join(root,'renderer','marmish-release-tool'), result.renderer, {flag:'wx'});
  await fs.writeFile(path.join(root,'bootstrap','install.sh'), result.bootstrap, {flag:'wx'});
  const bytes = Buffer.from(JSON.stringify({schema:1,skip:false,inventory:JSON.parse(canonicalInventory(result.inventory)),renderer_sha256:digest(result.renderer)}) + '\n');
  await fs.writeFile(path.join(root,'inspection.json'), bytes, {flag:'wx'}); return digest(bytes);
}
async function main(env) {
  need(env.GITHUB_REPOSITORY === REPOSITORY && HEX40.test(env.GITHUB_SHA ?? ''), 'invalid_publisher');
  const transport = new ArtifactTransport({context:{repository:REPOSITORY,run_id:env.CANDIDATE_RUN_ID},token:env.GH_TOKEN,budgetFile:path.join(env.RUNNER_TEMP,'release-api-budget.json')});
  const api = async route => { const r = await transport.request(`https://api.github.com${route}`,true,2*1024*1024,Date.now()+120000); need(r.bytes,'api_redirect'); return JSON.parse(r.bytes); };
  const git = (args, options = {}) => Promise.resolve(execFileSync('git',args,{env:childEnvironment(env),maxBuffer:options.maxBuffer ?? 2*1024*1024}).toString());
  const result = await inspectCandidate({run_id:env.CANDIDATE_RUN_ID,run_attempt:env.CANDIDATE_RUN_ATTEMPT,publisher_sha:env.GITHUB_SHA},{api,git,transport});
  if (process.argv[2] === 'verify') {
    need(!result.skip, 'unexpected_verify_skip');
    const checked = await loadInspection(env.INSPECTION_ROOT,env.INSPECTION_SHA256);
    const inventory = canonicalInventory(result.inventory);
    need(canonicalInventory(checked.inventory).equals(inventory) && digest(result.renderer) === checked.report.renderer_sha256, 'inspection_revalidation_mismatch');
    const rendered = await boundedFile(path.join(env.RENDER_ROOT,'install.sh'),131072);
    const report = JSON.parse(await boundedFile(path.join(env.RENDER_ROOT,'render.json'),4096));
    exactKeys(report,['schema','inspection_sha256','bootstrap_sha256']);
    need(report.schema === 1 && report.inspection_sha256 === env.INSPECTION_SHA256 && report.bootstrap_sha256 === digest(result.bootstrap)
      && rendered.equals(result.bootstrap), 'reproduction_verification_failed');
    execFileSync('/bin/sh',['-n',path.join(env.RENDER_ROOT,'install.sh')],{env:childEnvironment(env),timeout:10000});
    await fs.mkdir(env.INVENTORY_ROOT,{mode:0o700});
    await fs.writeFile(path.join(env.INVENTORY_ROOT,'inventory.json'),inventory,{flag:'wx'});
    await fs.appendFile(env.GITHUB_OUTPUT,`inventory_sha256=${digest(inventory)}\n`);
  } else {
    const hash = await writeInspection(result, env.INSPECTION_ROOT);
    await fs.appendFile(env.GITHUB_OUTPUT, `skip=${result.skip}\ninspection_sha256=${hash}\n`);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main(process.env).catch(error=>{console.error(`marmish candidate: ${error.message}`);process.exitCode=1;});
