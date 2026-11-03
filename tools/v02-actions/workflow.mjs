// CI-only candidate provenance and pre-execution verification. Never installed.
import {execFileSync} from 'node:child_process';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {ArtifactTransport, ROLES, artifactName, binding, boundedFile, childEnvironment, digest, requireThat} from './transport.mjs';
export const TARGETS = ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin'];
const TOOLS = ['marmish-installed-check', 'marmish-installed-fixture'];
const SOURCE_PATHS = ['crates/marmish-native', 'Cargo.lock', 'tools/v02-actions', '.github/workflows/v02-installed.yml'];
const hex = (s, n) => typeof s === 'string' && new RegExp(`^[0-9a-f]{${n}}$`).test(s);
export function cohortLifetime(env) {
  const name = env.TEST_COHORT ?? 'matrix';
  const seconds = {matrix: 2220, autoexit: 3300, uptime: 3300, benchmark: 3300, recovery: 3300, policy: 3300}[name];
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
export function candidateInputs(env) {
  cohortLifetime(env);
  for (const key of ['CANDIDATE_RUN_ID', 'CANDIDATE_RUN_ATTEMPT']) {
    requireThat(/^[1-9][0-9]{0,15}$/.test(env[key] ?? ''), 'invalid_candidate_run');
  }
  requireThat(env.GITHUB_REPOSITORY === 'Epiphytic/marmish'
    && hex(env.SOURCE_COMMIT, 40) && env.SOURCE_COMMIT === env.GITHUB_SHA, 'invalid_candidate_source');
  const relay = new URL(env.TEST_RELAY);
  requireThat(relay.protocol === 'wss:' && !relay.username && !relay.password && !relay.hash
    && relay.hostname && !relay.hostname.endsWith('.onion'), 'invalid_test_relay');
  return {repository: env.GITHUB_REPOSITORY, run_id: env.CANDIDATE_RUN_ID,
    run_attempt: env.CANDIDATE_RUN_ATTEMPT, source: env.SOURCE_COMMIT, relay: relay.href};
}
export function checkRun(run, c) {
  requireThat(String(run.id) === c.run_id && String(run.run_attempt) === c.run_attempt
    && run.status === 'completed' && run.conclusion === 'success'
    && run.path === '.github/workflows/v02-release.yml' && run.event === 'workflow_dispatch'
    && run.head_branch === 'master' && run.head_repository?.full_name === c.repository
    && run.repository?.full_name === c.repository && hex(run.head_sha, 40), 'invalid_candidate_provenance');
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
  requireThat(record.schema === 1 && /^0\.2\.[0-9]+(?:-rc\.[1-9][0-9]*)?$/.test(record.version)
    && record.commit === source && record.target === target
    && Array.isArray(record.formats) && record.formats.length === 5
    && record.formats.every(n => Number.isSafeInteger(n) && n > 0), 'tool_metadata');
}
async function publicCopy(source, target, limit) {
  await fs.writeFile(target, await boundedFile(source, limit), {flag: 'wx', mode: 0o600});
}
async function validateCandidate(env) {
  const c = candidateInputs(env);
  const transport = new ArtifactTransport({context: c, token: env.GH_TOKEN,
    budgetFile: path.join(env.RESOLVE_ROOT, 'api-budget.json')});
  const deadline = Date.now() + 120_000;
  const reply = await transport.request(`https://api.github.com/repos/${c.repository}/actions/runs/${c.run_id}/attempts/${c.run_attempt}`,
    true, 2 * 1024 * 1024, deadline);
  requireThat(reply.bytes, 'candidate_api_redirect');
  checkRun(JSON.parse(reply.bytes), c);
  const artifacts = await transport.list(deadline);
  const names = [...TARGETS.flatMap(target => [
    `v02-candidate-${target}-${c.run_id}-${c.run_attempt}`,
    `v02-installed-tools-${target}-${c.run_id}-${c.run_attempt}`]),
    `v02-pinned-bootstrap-${c.run_id}-${c.run_attempt}`];
  for (const name of names) {
    const matching = artifacts.filter(a => a.name === name);
    requireThat(matching.length === 1 && !matching[0].expired, 'candidate_artifacts_missing');
  }
}
async function resolve(env) {
  const c = candidateInputs(env);
  const cohort = cohortLifetime(env);
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
      requireThat(bytes, 'record_missing');
      await fs.writeFile(path.join(env.RECORDS_ROOT, `${role}-${kind}.json`), bytes, {flag: 'wx', mode: 0o600});
    }
  }
}
async function main() {
  const mode = process.argv[2];
  if (mode === 'validate-candidate') await validateCandidate(process.env);
  else if (mode === 'resolve') await resolve(process.env);
  else if (mode === 'verify-context') await verifyContext(process.env);
  else if (mode === 'gather') await gather(process.env);
  else throw new Error('invalid_workflow_mode');
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => { console.error('marmish installed workflow: verification failed'); process.exitCode = 1; });
}
