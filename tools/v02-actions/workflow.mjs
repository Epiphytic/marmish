// CI-only candidate provenance and pre-execution verification. Never installed.
import {execFileSync} from 'node:child_process';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {ArtifactTransport, ROLES, artifactName, binding, boundedFile, childEnvironment, digest, requireThat} from './transport.mjs';
import {isStreamingCohort} from './streaming-policy.mjs';
import {validVersion} from './release-schema.mjs';
export const TARGETS = ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin'];
const TOOLS = ['marmish-installed-check', 'marmish-installed-fixture'];
export const SOURCE_PATHS = ['crates/marmish-native', 'crates/marmish-engine', 'crates/marmish-stream',
  'crates/marmish-core', 'crates/marmish-vault', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
  'scripts', 'tools/v02-actions', '.github/workflows/v02-release.yml',
  '.github/workflows/streaming-candidate.yml', '.github/workflows/streaming-installed.yml',
  '.github/workflows/v02-installed.yml',
  '.github/workflows/v02-version-lifecycle.yml', 'tools/marmish_v02_lifecycle_build.py',
  'tools/test_marmish_v02_lifecycle_build.py', 'tools/marmish_native_https_check.py',
  'tools/marmish_native_keychain_check.py', 'tools/marmish_native_quickstart_check.py',
  'tools/test_marmish_native_inspection.py', 'tools/marmish_native_licenses.py',
  'tools/test_marmish_native_licenses.py', 'tools/release-version-vectors.json'];
const hex = (s, n) => typeof s === 'string' && new RegExp(`^[0-9a-f]{${n}}$`).test(s);
export const PROVIDER_DIAGNOSTIC = Object.freeze({run:'41898839617', attempt:'1',
  source:'61f47606c5e7ba9048f196165a8025e7c2662839'});
export const DIAGNOSTIC_PATHS = Object.freeze([
  'tools/marmish_native_quickstart_check.py',
  'tools/marmish_native_keychain_check.py', 'tools/test_marmish_native_keychain_check.py',
  '.github/workflows/v02-provider-diagnostic.yml', 'tools/test_marmish_provider_diagnostic_workflow.py',
  'tools/v02-actions/workflow.mjs', 'tools/v02-actions/workflow.test.mjs',
  'docs/phases/native-provider-diagnostics.md', 'docs/reviews/native-provider-diagnostics-design-r1.md',
  'docs/reviews/native-provider-diagnostics-design-r2.md', 'docs/reviews/native-provider-diagnostics-code-r1.md',
  'docs/reviews/native-provider-diagnostics-code-r2.md', 'docs/validation/native-provider-diagnostics.md']);
const diagnosticJobs = [
  [126816261687,'package (ubuntu-24.04-arm, aarch64-unknown-linux-gnu, none, glibc=2.39)','success'],
  [126816262401,'package (macos-15, aarch64-apple-darwin, adhoc, macos=15.0)','success'],
  [126816262683,'package (ubuntu-24.04, x86_64-unknown-linux-gnu, none, glibc=2.39)','success'],
  [126848469757,'render','success'],
  [126848598035,'provider-qualification (macos-15, aarch64-apple-darwin)','failure']];
function diagnosticPolicy(policy) {
  requireThat(policy === undefined || policy === PROVIDER_DIAGNOSTIC, 'invalid_candidate_policy');
  return policy === PROVIDER_DIAGNOSTIC;
}
export function cohortLifetime(env) {
  const name = env.TEST_COHORT ?? 'matrix';
  const seconds = isStreamingCohort(name) ? 3300 : {matrix: 2220, autoexit: 3300, uptime: 3300, benchmark: 3300, recovery: 3300, policy: 3300, 'version-lifecycle': 1800}[name];
  requireThat(Number.isSafeInteger(seconds), 'unsupported_cohort');
  return {name, seconds};
}
function keys(object, expected) {
  requireThat(object && typeof object === 'object' && !Array.isArray(object)
    && Object.keys(object).sort().join() === [...expected].sort().join(), 'invalid_tooling_record');
}
export function sourceDigest(source) {
  requireThat(hex(source, 40), 'invalid_source');
  return digest(execFileSync('git', ['ls-tree', '-r', source, '--', ...SOURCE_PATHS],
    {env: childEnvironment(process.env), maxBuffer: 2 * 1024 * 1024}));
}
export function candidateInputs(env, policy) {
  const diagnostic = diagnosticPolicy(policy);
  cohortLifetime(env);
  for (const key of ['CANDIDATE_RUN_ID', 'CANDIDATE_RUN_ATTEMPT']) {
    requireThat(/^[1-9][0-9]{0,15}$/.test(env[key] ?? ''), 'invalid_candidate_run');
  }
  requireThat(env.GITHUB_REPOSITORY === 'Epiphytic/marmish'
    && hex(env.SOURCE_COMMIT, 40) && hex(env.GITHUB_SHA, 40)
    && (diagnostic ? env.SOURCE_COMMIT === policy.source && env.CANDIDATE_RUN_ID === policy.run
      && env.CANDIDATE_RUN_ATTEMPT === policy.attempt : env.SOURCE_COMMIT === env.GITHUB_SHA), 'invalid_candidate_source');
  if (diagnostic) return {repository:env.GITHUB_REPOSITORY,run_id:policy.run,run_attempt:policy.attempt,source:policy.source};
  const relay = new URL(env.TEST_RELAY);
  requireThat(relay.protocol === 'wss:' && !relay.username && !relay.password && !relay.hash
    && relay.hostname && !relay.hostname.endsWith('.onion'), 'invalid_test_relay');
  return {repository: env.GITHUB_REPOSITORY, run_id: env.CANDIDATE_RUN_ID,
    run_attempt: env.CANDIDATE_RUN_ATTEMPT, source: env.SOURCE_COMMIT, relay: relay.href};
}
export const INSTALLED_CANDIDATE_PATHS = Object.freeze([
  '.github/workflows/v02-release.yml', '.github/workflows/streaming-candidate.yml',
]);
// Publication callers keep the release-only default; installed validation opts in.
export function checkRun(run, c, policy, paths = ['.github/workflows/v02-release.yml']) {
  const diagnostic = diagnosticPolicy(policy);
  requireThat(String(run.id) === c.run_id && String(run.run_attempt) === c.run_attempt
    && run.status === 'completed' && run.conclusion === (diagnostic ? 'failure' : 'success')
    && (diagnostic ? run.path === '.github/workflows/v02-release.yml'
      : paths.includes(run.path))
    && (diagnostic ? run.event === 'workflow_dispatch' : ['workflow_dispatch', 'push'].includes(run.event))
    && run.head_branch === 'master' && run.head_repository?.full_name === c.repository
    && run.repository?.full_name === c.repository && hex(run.head_sha, 40)
    && (run.path !== '.github/workflows/streaming-candidate.yml'
      || run.event === 'workflow_dispatch' && run.head_sha === c.source)
    && (!diagnostic || run.head_sha === policy.source && c.source === policy.source
      && c.run_id === policy.run && c.run_attempt === policy.attempt), 'invalid_candidate_provenance');
}
export function checkDiagnosticJobs(jobs) {
  for (const [id,name,conclusion] of diagnosticJobs) {
    const matches = jobs.filter(job => job.id === id || job.name === name);
    requireThat(matches.length === 1 && matches[0].id === id && matches[0].name === name
      && matches[0].status === 'completed' && matches[0].conclusion === conclusion, 'diagnostic_jobs');
  }
}
export function checkDiagnosticChanges(entries) {
  requireThat(entries.length > 0, 'diagnostic_empty_delta');
  for (const [status,name,mode] of entries) requireThat(['A','M'].includes(status)
    && DIAGNOSTIC_PATHS.includes(name) && mode === '100644', 'diagnostic_source_delta');
}
export function checkTooling(record, role, source, harness) {
  requireThat(ROLES.includes(role), 'invalid_role');
  keys(record, ['schema', 'target', 'source_commit', 'harness_sha256', 'files']);
  requireThat(record.schema === 1 && record.target === TARGETS[ROLES.indexOf(role)]
    && record.source_commit === source && record.harness_sha256 === harness, 'tooling_provenance');
  keys(record.files, TOOLS);
  for (const pin of Object.values(record.files)) {
    keys(pin, ['sha256', 'bytes']);
    requireThat(hex(pin.sha256, 64) && Number.isSafeInteger(pin.bytes)
      && pin.bytes > 0 && pin.bytes <= 256 * 1024 * 1024, 'tooling_pin');
  }
}
export async function verifyTools(directory, role, source, harness, expectedRecordDigest) {
  requireThat(ROLES.includes(role), 'invalid_role');
  const bytes = await boundedFile(path.join(directory, `${role}-tooling.json`), 65536);
  if (expectedRecordDigest) requireThat(digest(bytes) === expectedRecordDigest, 'tooling_record_digest');
  const record = JSON.parse(bytes);
  checkTooling(record, role, source, harness);
  for (const name of TOOLS) {
    const file = path.join(directory, name);
    const bin = await boundedFile(file, record.files[name].bytes);
    requireThat(bin.length === record.files[name].bytes && digest(bin) === record.files[name].sha256, 'tool_digest');
    await fs.chmod(file, 0o500);
  }
  return {record, bytes};
}
export function probeTool(file, source, target) {
  const reply = execFileSync(file, ['--release-info'], {env: childEnvironment(process.env),
    timeout: 10_000, killSignal: 'SIGKILL', maxBuffer: 4096, stdio: ['ignore', 'pipe', 'pipe']});
  const record = JSON.parse(reply);
  keys(record, ['schema', 'version', 'target', 'commit', 'formats']);
  requireThat(record.schema === 1 && validVersion(record.version)
    && record.commit === source && record.target === target
    && Array.isArray(record.formats) && record.formats.length === 5
    && record.formats.every(n => Number.isSafeInteger(n) && n > 0), 'tool_metadata');
}
async function publicCopy(source, target, limit) {
  await fs.writeFile(target, await boundedFile(source, limit), {flag: 'wx', mode: 0o600});
}
export async function validateCandidate(env, retentionUntil = 0, injectedTransport, policy) {
  const diagnostic = diagnosticPolicy(policy);
  const c = candidateInputs(env, policy);
  const transport = injectedTransport ?? new ArtifactTransport({context: c, token: env.GH_TOKEN,
    budgetFile: path.join(env.RESOLVE_ROOT, 'api-budget.json')});
  const deadline = Date.now() + 120_000;
  const reply = await transport.request(`https://api.github.com/repos/${c.repository}/actions/runs/${c.run_id}/attempts/${c.run_attempt}`,
    true, 2 * 1024 * 1024, deadline);
  requireThat(reply.bytes, 'candidate_api_redirect');
  checkRun(JSON.parse(reply.bytes), c, policy, INSTALLED_CANDIDATE_PATHS);
  if (diagnostic) {
    const jobs = [];
    let total;
    for (let page = 1; page <= 10; page++) {
      const response = await transport.request(`https://api.github.com/repos/${c.repository}/actions/runs/${c.run_id}/attempts/${c.run_attempt}/jobs?per_page=100&page=${page}`,
        true, 2 * 1024 * 1024, deadline);
      requireThat(response.bytes, 'diagnostic_jobs_redirect');
      const record = JSON.parse(response.bytes);
      requireThat(Number.isSafeInteger(record.total_count) && record.total_count > 0 && record.total_count <= 1000
        && Array.isArray(record.jobs) && record.jobs.length <= 100
        && (total === undefined || total === record.total_count), 'diagnostic_jobs_count');
      total = record.total_count;
      jobs.push(...record.jobs);
      requireThat(jobs.length <= total && new Set(jobs.map(job=>job.id)).size === jobs.length, 'diagnostic_jobs_count');
      if (jobs.length === total) break;
      requireThat(record.jobs.length === 100, 'diagnostic_jobs_count');
    }
    requireThat(jobs.length === total, 'diagnostic_jobs_count');
    checkDiagnosticJobs(jobs);
  }
  const artifacts = await transport.list(deadline);
  const names = diagnostic ? [`v02-candidate-aarch64-apple-darwin-${c.run_id}-${c.run_attempt}`,
    `v02-pinned-bootstrap-${c.run_id}-${c.run_attempt}`] : [...TARGETS.flatMap(target => [
    `v02-candidate-${target}-${c.run_id}-${c.run_attempt}`,
    `v02-installed-tools-${target}-${c.run_id}-${c.run_attempt}`]),
    `v02-pinned-bootstrap-${c.run_id}-${c.run_attempt}`];
  for (const name of names) {
    const matching = artifacts.filter(a => a.name === name);
    requireThat(matching.length === 1 && !matching[0].expired
      && (retentionUntil === 0 || Date.parse(matching[0].expires_at) >= retentionUntil), 'candidate_artifacts_missing');
  }
  return artifacts.filter(artifact=>names.includes(artifact.name));
}
export async function candidatePins(env) {
  const c = candidateInputs(env);
  const harness = sourceDigest(c.source);
  const manifests = path.join(env.RESOLVE_ROOT, 'manifests');
  const tooling = path.join(env.RESOLVE_ROOT, 'tooling');
  const output = path.join(env.RESOLVE_ROOT, 'output');
  for (const directory of [manifests, tooling, output]) await fs.mkdir(directory, {mode: 0o700});
  for (const [index, role] of ROLES.entries()) {
    const suffix = `${TARGETS[index]}-${c.run_id}-${c.run_attempt}`;
    const directory = path.join(env.ASSETS_ROOT, `v02-candidate-${suffix}`);
    const files = (await fs.readdir(directory)).filter(name => name.endsWith('.manifest'));
    requireThat(files.length === 1, 'candidate_manifest_count');
    await publicCopy(path.join(directory, files[0]), path.join(manifests, `${role}.manifest`), 4096);
    const toolDirectory = path.join(env.TOOLS_ROOT, `v02-installed-tools-${suffix}`);
    const checked = await verifyTools(toolDirectory, role, c.source, harness);
    await fs.writeFile(path.join(tooling, `${role}-tooling.json`), checked.bytes, {flag: 'wx', mode: 0o600});
  }
  return {c, harness, manifests, tooling, output};
}
async function resolve(env) {
  const {c, harness, manifests, tooling, output} = await candidatePins(env);
  const cohort = cohortLifetime(env);
  const driver = path.join(env.TOOLS_ROOT,
    `v02-installed-tools-${TARGETS[0]}-${c.run_id}-${c.run_attempt}`, TOOLS[0]);
  probeTool(driver, c.source, TARGETS[0]);
  const bootstrap = path.join(env.BOOTSTRAP_ROOT, 'install.sh');
  const args = ['context', '--repository', c.repository, '--run-id', env.GITHUB_RUN_ID,
    '--run-attempt', env.GITHUB_RUN_ATTEMPT, '--candidate-run-id', c.run_id,
    '--candidate-run-attempt', c.run_attempt, '--source', c.source,
    '--harness-sha256', harness, '--manifests', manifests, '--tooling-manifests', tooling,
    '--bootstrap', bootstrap, '--cohort', cohort.name, '--relay', c.relay,
    '--expires-at', String(Math.floor(Date.now() / 1000) + cohort.seconds),
    '--output', path.join(output, 'context.json')];
  execFileSync(driver, args, {env: childEnvironment(env), timeout: 30_000,
    killSignal: 'SIGKILL', maxBuffer: 65536, stdio: ['ignore', 'pipe', 'pipe']});
}
export async function verifyContext(env) {
  const context = JSON.parse(await boundedFile(env.CONTEXT, 65536));
  requireThat(context.cohort === cohortLifetime(env).name, 'workflow_cohort');
  requireThat(context.repository === env.GITHUB_REPOSITORY && context.source_commit === env.GITHUB_SHA
    && String(context.run_id) === env.GITHUB_RUN_ID && String(context.run_attempt) === env.GITHUB_RUN_ATTEMPT
    && context.harness_sha256 === sourceDigest(env.GITHUB_SHA), 'workflow_context');
  const role = env.ROLE;
  requireThat(String(context.candidate_run_id) === env.CANDIDATE_RUN_ID
    && String(context.candidate_run_attempt) === env.CANDIDATE_RUN_ATTEMPT
    && hex(context.tooling_manifests?.[role], 64) && hex(context.bootstrap_sha256, 64), 'context_pins');
  await verifyTools(env.TOOLS_ROOT, role, context.source_commit, context.harness_sha256,
    context.tooling_manifests?.[role]);
  const bootstrap = await boundedFile(path.join(env.BOOTSTRAP_ROOT, 'install.sh'), 131072);
  requireThat(digest(bootstrap) === context.bootstrap_sha256, 'bootstrap_digest');
  for (const name of TOOLS) probeTool(path.join(env.TOOLS_ROOT, name), context.source_commit, TARGETS[ROLES.indexOf(role)]);
}
export async function gather(env, injectedTransport) {
  const context = JSON.parse(await boundedFile(env.CONTEXT, 65536));
  requireThat(context.repository === env.GITHUB_REPOSITORY && context.source_commit === env.GITHUB_SHA
    && String(context.run_id) === env.GITHUB_RUN_ID && String(context.run_attempt) === env.GITHUB_RUN_ATTEMPT, 'rollup_context');
  const transport = injectedTransport ?? new ArtifactTransport({context, token: env.GH_TOKEN,
    budgetFile: path.join(env.RESOLVE_ROOT, 'api-budget.json')});
  const deadline = Date.now() + 120_000;
  const artifacts = await transport.list(deadline);
  await fs.mkdir(env.RECORDS_ROOT, {mode: 0o700});
  for (const role of ROLES) {
    for (const kind of ['identity-ready', 'server-ready', 'client-result', 'server-result']) {
      const bytes = await transport.download(artifacts, artifactName(binding(context, role), role, kind), `${kind}.json`, deadline);
      // Missing records stay absent: the pinned native rollup emits a sanitized FAIL.
      if (!bytes) continue;
      await fs.writeFile(path.join(env.RECORDS_ROOT, `${role}-${kind}.json`), bytes, {flag: 'wx', mode: 0o600});
    }
  }
}
export async function providerDiagnostic(env, assets = false) {
  const p = PROVIDER_DIAGNOSTIC;
  const file = path.join(env.DIAGNOSTIC_ROOT, 'provenance.json');
  await fs.mkdir(env.DIAGNOSTIC_ROOT, {recursive:true,mode:0o700});
  let envelope = {schema:1,diagnostic:true,product_source_commit:p.source,
    supervisor_source_commit:env.GITHUB_SHA,upstream_run:p.run,upstream_attempt:p.attempt,
    diagnostic_run:env.GITHUB_RUN_ID,diagnostic_attempt:env.GITHUB_RUN_ATTEMPT,guard:'pending'};
  if (assets) envelope = JSON.parse(await boundedFile(file,16384));
  await fs.writeFile(file,JSON.stringify(envelope,null,2)+'\n',{mode:0o600});
  try {
    if (!assets) {
      requireThat(env.GITHUB_REF === 'refs/heads/master' && hex(env.GITHUB_SHA,40), 'diagnostic_ref');
      const git = args => execFileSync('git',args,{encoding:'utf8',env:childEnvironment(env),maxBuffer:1024*1024});
      requireThat(git(['rev-parse','HEAD']).trim() === env.GITHUB_SHA, 'diagnostic_head');
      git(['merge-base','--is-ancestor',p.source,'HEAD']);
      git(['merge-base','--is-ancestor','HEAD','refs/remotes/origin/master']);
      const changed = git(['diff','--no-renames','--name-status','-z',p.source,'HEAD']).split('\0');
      requireThat(changed.pop() === '' && changed.length % 2 === 0,'diagnostic_delta');
      const entries = [];
      for (let i=0;i<changed.length;i+=2) {
        const tree = git(['ls-tree','HEAD','--',changed[i+1]]);
        entries.push([changed[i],changed[i+1],tree.startsWith('100644 blob ') ? '100644' : 'invalid']);
      }
      checkDiagnosticChanges(entries);
      const selected = await validateCandidate({...env,CANDIDATE_RUN_ID:p.run,CANDIDATE_RUN_ATTEMPT:p.attempt,
        SOURCE_COMMIT:p.source,RESOLVE_ROOT:env.DIAGNOSTIC_ROOT},0,undefined,p);
      envelope.artifacts = selected.map(a=>{
        requireThat(Number.isSafeInteger(a.id) && a.id > 0 && /^sha256:[0-9a-f]{64}$/.test(a.digest), 'diagnostic_artifact_metadata');
        return {id:a.id,name:a.name,sha256:a.digest.slice(7)};
      });
      envelope.guard = 'metadata_pass';
    } else {
      requireThat(envelope.guard === 'metadata_pass' && envelope.product_source_commit === p.source
        && envelope.supervisor_source_commit === env.GITHUB_SHA, 'diagnostic_envelope');
      const manifests = (await fs.readdir(env.ASSETS_ROOT)).filter(name=>name.endsWith('.manifest'));
      requireThat(manifests.length === 1,'diagnostic_manifest_count');
      const manifest = await boundedFile(path.join(env.ASSETS_ROOT,manifests[0]),4096);
      const lines = manifest.toString('utf8').split('\n');
      requireThat(lines.length === 13 && lines[0] === 'marmish-manifest 1'
        && lines[2] === 'target aarch64-apple-darwin' && lines[3] === `commit ${p.source}`
        && lines[11] === 'end' && lines[12] === '', 'diagnostic_manifest_source');
      envelope.manifest_sha256 = digest(manifest);
      envelope.bootstrap_sha256 = digest(await boundedFile(path.join(env.BOOTSTRAP_ROOT,'install.sh'),131072));
      envelope.guard = 'assets_pass';
    }
  } catch {
    envelope.guard = assets ? 'assets_refused' : 'metadata_refused';
    await fs.writeFile(file,JSON.stringify(envelope,null,2)+'\n',{mode:0o600});
    throw new Error('provider_diagnostic_refused');
  }
  await fs.writeFile(file,JSON.stringify(envelope,null,2)+'\n',{mode:0o600});
}
async function main() {
  const mode = process.argv[2];
  if (mode === 'validate-candidate') await validateCandidate(process.env);
  else if (mode === 'resolve') await resolve(process.env);
  else if (mode === 'verify-context') await verifyContext(process.env);
  else if (mode === 'gather') await gather(process.env);
  else if (mode === 'provider-diagnostic-validate') await providerDiagnostic(process.env);
  else if (mode === 'provider-diagnostic-assets') await providerDiagnostic(process.env,true);
  else throw new Error('invalid_workflow_mode');
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => { console.error('marmish installed workflow: verification failed'); process.exitCode = 1; });
}
