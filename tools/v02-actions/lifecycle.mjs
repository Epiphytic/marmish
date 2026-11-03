// Build-only owner of synthetic-version qualification. Product children receive
// only the Rust checker's separately curated runtime environment.
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {runOwned} from './local.mjs';
import {candidateInputs, candidatePins, sourceDigest, validateCandidate, verifyTools, probeTool, TARGETS} from './workflow.mjs';
import {boundedFile, digest, requireThat, ROLES, RECORD_LIMIT} from './transport.mjs';

export function lifecycleBudget(started, internal, current = Date.now()) {
  requireThat(Number.isSafeInteger(started) && Number.isSafeInteger(internal)
    && started <= current && internal === started + 85 * 60_000, 'lifecycle_job_clock');
  const buildEnd = Math.min(started + 45 * 60_000, internal - 35 * 60_000);
  requireThat(current < buildEnd, 'lifecycle_build_deadline');
  return {buildEnd, internal};
}
export function runtimeExpiry(internal, current = Date.now()) {
  requireThat(Number.isSafeInteger(internal) && current + 2100_000 <= internal,
    'lifecycle_runtime_reserve');
  return Math.floor(current / 1000) + 1800;
}
function absolute(...values) {
  requireThat(values.every(v => typeof v === 'string' && path.isAbsolute(v)), 'lifecycle_path');
}
function roleTarget(role) {
  requireThat(ROLES.includes(role), 'lifecycle_role');
  return TARGETS[ROLES.indexOf(role)];
}
export async function lifecycleSequence(o, signal, run = runOwned, clock = Date.now) {
  const {env, sourceRoot, pins, toolRoot, assets, bootstrap, variants, workspace, output, role} = o;
  absolute(sourceRoot, pins, toolRoot, assets, bootstrap, variants, workspace, output);
  const c = candidateInputs(env), target = roleTarget(role);
  const {buildEnd, internal} = lifecycleBudget(o.started, o.internal, clock());
  const harness = sourceDigest(c.source);
  const bootstrapPin = await boundedFile(path.join(pins, 'bootstrap.sha256'), 64);
  requireThat(/^[a-f0-9]{64}$/.test(bootstrapPin.toString())
    && digest(await boundedFile(bootstrap, 131072)) === bootstrapPin.toString(), 'lifecycle_bootstrap_pin');
  const manifests = (await fs.readdir(assets)).filter(name => name.endsWith('.manifest'));
  requireThat(manifests.length === 1, 'lifecycle_manifest_count');
  const manifestPin = await boundedFile(path.join(pins, 'manifests', `${role}.manifest`), 4096);
  requireThat((await boundedFile(path.join(assets, manifests[0]), 4096)).equals(manifestPin),
    'lifecycle_manifest_pin');
  const ownPin = await boundedFile(path.join(pins, 'tooling', `${role}-tooling.json`), 65536);
  await verifyTools(toolRoot, role, c.source, harness, digest(ownPin));
  const driver = path.join(toolRoot, 'marmish-installed-check');
  for (const tool of ['marmish-installed-check', 'marmish-installed-fixture']) {
    probeTool(path.join(toolRoot, tool), c.source, target);
  }
  const base = {cwd: sourceRoot, env, signal, deadline: buildEnd};
  await run('/usr/bin/python3', [path.join(sourceRoot, 'tools/marmish_v02_lifecycle_build.py'),
    '--source', c.source, '--target', target, '--candidate-assets', assets,
    '--output', variants, '--deadline', String(Math.floor(internal / 1000)),
    '--build-deadline', String(Math.floor(buildEnd / 1000))], base);
  requireThat(!signal?.aborted, 'lifecycle_cancelled');
  const expires = runtimeExpiry(internal, clock());
  // No context existed during the build. Its one 1800s runtime starts here and
  // still fits the original job cutoff with the separate outer 300s margin.
  await fs.mkdir(output, {mode: 0o700});
  const candidate = path.join(output, 'candidate-context.json');
  const context = path.join(output, 'lifecycle-context.json');
  const result = path.join(output, 'lifecycle-result.json');
  const owned = {...base, deadline: Math.min(expires * 1000, internal - 300_000), graceMs: 290_000};
  await run(driver, ['context', '--repository', c.repository, '--run-id', env.GITHUB_RUN_ID,
    '--run-attempt', env.GITHUB_RUN_ATTEMPT, '--candidate-run-id', c.run_id,
    '--candidate-run-attempt', c.run_attempt, '--source', c.source, '--harness-sha256', harness,
    '--manifests', path.join(pins, 'manifests'), '--tooling-manifests', path.join(pins, 'tooling'),
    '--bootstrap', bootstrap, '--cohort', 'version-lifecycle', '--relay', c.relay,
    '--expires-at', String(expires), '--output', candidate], owned);
  await run(driver, ['lifecycle-context', '--context', candidate,
    '--variants', path.join(variants, 'variants.json'), '--internal-deadline', String(Math.floor(internal / 1000)),
    '--target', target, '--output', context], owned);
  await run(driver, ['lifecycle-run', '--context', context, '--workspace', workspace,
    '--output', result, '--candidate-assets', assets, '--variant-assets', variants,
    '--bootstrap', bootstrap, '--source-root', sourceRoot], owned);
}

export async function collectLifecycle(root, destination) {
  requireThat((await fs.readdir(root)).sort().join() === [...ROLES].sort().join(), 'lifecycle_roles');
  await fs.mkdir(destination, {mode: 0o700});
  for (const role of ROLES) {
    const directory = path.join(root, role);
    const m = await fs.lstat(directory);
    requireThat(m.isDirectory() && !m.isSymbolicLink(), 'lifecycle_role_directory');
    requireThat((await fs.readdir(directory)).sort().join()
      === 'lifecycle-context.json,lifecycle-result.json', 'lifecycle_record_set');
    for (const kind of ['context', 'result']) {
      const bytes = await boundedFile(path.join(directory, `lifecycle-${kind}.json`), RECORD_LIMIT);
      await fs.writeFile(path.join(destination, `${role}-${kind}.json`), bytes,
        {flag: 'wx', mode: 0o600});
    }
  }
}
export async function lifecycleRollup(env, run = runOwned) {
  const c = candidateInputs(env);
  absolute(env.TOOLS_ROOT, env.LIFECYCLE_RECORDS_FLAT, env.LIFECYCLE_ROLLUP);
  return run(path.join(env.TOOLS_ROOT, 'marmish-installed-check'),
    ['lifecycle-rollup', '--input', env.LIFECYCLE_RECORDS_FLAT, '--output', env.LIFECYCLE_ROLLUP,
      '--job-result', env.LIFECYCLE_JOB_RESULT, '--source', c.source,
      '--run-id', env.GITHUB_RUN_ID, '--attempt', env.GITHUB_RUN_ATTEMPT,
      '--harness-sha256', sourceDigest(c.source), '--candidate-run-id', c.run_id,
      '--candidate-run-attempt', c.run_attempt],
    {cwd: env.GITHUB_WORKSPACE, env, deadline: Date.now() + 300_000});
}
async function main(env) {
  if (env.LIFECYCLE_MODE === 'validate-candidate') {
    // A successful start needs retention through the complete 90m hosted window.
    return validateCandidate(env, Date.now() + 90 * 60_000);
  }
  if (env.LIFECYCLE_MODE === 'pins') {
    await candidatePins(env);
    const bootstrap = await boundedFile(path.join(env.BOOTSTRAP_ROOT, 'install.sh'), 131072);
    await fs.writeFile(path.join(env.RESOLVE_ROOT, 'bootstrap.sha256'), digest(bootstrap),
      {flag: 'wx', mode: 0o600});
    return;
  }
  if (env.LIFECYCLE_MODE === 'collect') {
    return collectLifecycle(env.LIFECYCLE_RECORDS_ROOT, env.LIFECYCLE_RECORDS_FLAT);
  }
  if (env.LIFECYCLE_MODE === 'verify-tools') {
    const c = candidateInputs(env), target = roleTarget(env.ROLE);
    const pin = await boundedFile(path.join(env.PINS_ROOT, 'tooling', `${env.ROLE}-tooling.json`), 65536);
    await verifyTools(env.TOOLS_ROOT, env.ROLE, c.source, sourceDigest(c.source), digest(pin));
    return probeTool(path.join(env.TOOLS_ROOT, 'marmish-installed-check'), c.source, target);
  }
  if (env.LIFECYCLE_MODE === 'rollup') {
    return lifecycleRollup(env);
  }
  requireThat(env.LIFECYCLE_MODE === 'run', 'lifecycle_mode');
  const abort = new AbortController(), cancel = () => abort.abort();
  for (const signal of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(signal, cancel);
  try {
    await lifecycleSequence({env, sourceRoot: env.GITHUB_WORKSPACE, pins: env.PINS_ROOT,
      toolRoot: env.TOOLS_ROOT, assets: env.ASSETS_ROOT,
      bootstrap: path.join(env.BOOTSTRAP_ROOT, 'install.sh'), variants: env.VARIANTS_ROOT,
      workspace: env.MARMISH_WORK, output: env.LIFECYCLE_OUTPUT, role: env.ROLE,
      started: Number(env.JOB_STARTED_MS), internal: Number(env.JOB_DEADLINE_MS)}, abort.signal);
  } finally {
    for (const signal of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.removeListener(signal, cancel);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.env).catch(() => {
    process.stderr.write('marmish version lifecycle coordinator: invalid_or_failed\n');
    process.exitCode = 1;
  });
}
