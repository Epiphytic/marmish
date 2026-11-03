// CI-only local cohort owner. Never installed with Marmish.
import {spawn} from 'node:child_process';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {boundedFile, childEnvironment, requireThat, ROLES, RECORD_LIMIT} from './transport.mjs';

export const LOCAL_COHORTS = ['autoexit', 'uptime', 'benchmark', 'recovery', 'policy'];

// Always observe close (including pipe closure), even after exit or a signal.
// Continued draining after overflow prevents a full pipe from blocking teardown.
export async function runOwned(file, args, {cwd, env, deadline, signal,
  graceMs = 60_000, closeMs = 5_000, outputLimit = 65_536}) {
  requireThat(Number.isSafeInteger(deadline) && deadline > Date.now(), 'local_deadline');
  requireThat(!signal?.aborted, 'local_cancelled');
  const child = spawn(file, args, {cwd, env: childEnvironment(env), detached: true,
    stdio: ['ignore', 'pipe', 'pipe']});
  let failed = false, closed = false, bytes = 0, escalation, finalBound;
  const groupSignal = value => {
    if (!child.pid) return;
    try { process.kill(-child.pid, value); } catch (error) {
      if (error.code !== 'ESRCH') failed = true;
    }
  };
  const groupPresent = () => {
    if (!child.pid) return false;
    try { process.kill(-child.pid, 0); return true; } catch (error) {
      if (error.code !== 'ESRCH') { failed = true; return true; }
      return false;
    }
  };
  let resolveBound;
  const exhausted = new Promise(resolve => { resolveBound = resolve; });
  const terminate = () => {
    failed = true;
    if (escalation || closed) return;
    groupSignal('SIGTERM');
    escalation = setTimeout(() => {
      groupSignal('SIGKILL');
      finalBound = setTimeout(() => resolveBound(false), closeMs);
    }, graceMs);
  };
  const consume = chunk => {
    bytes += chunk.length;
    if (bytes > outputLimit) terminate();
  };
  child.stdout.on('data', consume);
  child.stderr.on('data', consume);
  child.stdout.on('error', terminate);
  child.stderr.on('error', terminate);
  child.on('error', terminate);
  const completion = new Promise(resolve => {
    child.once('close', (code, cause) => {
      closed = true;
      if (groupPresent()) { failed = true; groupSignal('SIGKILL'); }
      resolve(!failed && code === 0 && cause === null);
    });
  });
  const watchdog = setTimeout(terminate, Math.max(0, deadline - Date.now()));
  signal?.addEventListener('abort', terminate, {once: true});
  if (signal?.aborted) terminate();
  try {
    const passed = await Promise.race([completion, exhausted]);
    requireThat(passed && closed && !failed, 'local_child_failed');
  } finally {
    clearTimeout(watchdog); clearTimeout(escalation); clearTimeout(finalBound);
    signal?.removeEventListener('abort', terminate);
    groupSignal('SIGKILL');
    if (!closed) {
      child.stdout.destroy(); child.stderr.destroy(); child.unref();
    }
  }
}

export function localBudget(context, cohort, role, jobDeadline, current = Date.now()) {
  requireThat(LOCAL_COHORTS.includes(cohort) && context.cohort === cohort
    && context.schema === 1 && ROLES.includes(role), 'local_context');
  requireThat(Number.isSafeInteger(context.created_at) && Number.isSafeInteger(context.expires_at)
    && context.expires_at > context.created_at
    && context.expires_at - context.created_at <= 3300
    && context.created_at * 1000 <= current && context.expires_at * 1000 > current,
  'local_context_time');
  requireThat(Number.isSafeInteger(jobDeadline)
    && context.expires_at * 1000 + 360_000 <= jobDeadline, 'local_job_deadline');
  return Math.min(context.expires_at * 1000, jobDeadline - 300_000);
}

export async function localSequence(options, signal, run = runOwned) {
  const {contextFile, cohort, role, jobDeadline, tool, workspace, assets, bootstrap} = options;
  for (const file of [contextFile, tool, workspace, assets, bootstrap]) {
    requireThat(typeof file === 'string' && path.isAbsolute(file), 'local_path');
  }
  const context = JSON.parse(await boundedFile(contextFile, 131072));
  const deadline = localBudget(context, cohort, role, jobDeadline);
  const output = path.join(workspace, 'output');
  const owned = {cwd: path.dirname(workspace), env: process.env, deadline, signal};
  await run(tool, ['prepare', '--cohort', cohort, '--context', contextFile,
    '--assets', assets, '--bootstrap', bootstrap, '--workspace', workspace,
    '--role', role, '--output', output], owned);
  requireThat(!signal?.aborted && Date.now() < deadline, 'local_sequence_stopped');
  await run(tool, ['cohort', '--cohort', cohort, '--context', contextFile,
    '--workspace', workspace, '--output', path.join(output, `${role}-${cohort}-result.json`)], owned);
}

// Official artifact downloads are confined to separate directories. Copy only
// the six exact public regular files into a fresh flat rollup input directory.
export async function collectLocal(root, destination, cohort) {
  requireThat(LOCAL_COHORTS.includes(cohort), 'local_cohort');
  requireThat((await fs.readdir(root)).sort().join() === [...ROLES].sort().join(), 'local_roles');
  await fs.mkdir(destination, {mode: 0o700});
  for (const role of ROLES) {
    const directory = path.join(root, role);
    requireThat((await fs.lstat(directory)).isDirectory(), 'local_role_directory');
    const expected = [`${role}-identity-ready.json`, `${role}-${cohort}-result.json`].sort();
    requireThat((await fs.readdir(directory)).sort().join() === expected.join(), 'local_record_set');
    for (const name of expected) {
      const bytes = await boundedFile(path.join(directory, name), RECORD_LIMIT);
      await fs.writeFile(path.join(destination, name), bytes, {flag: 'wx', mode: 0o600});
    }
  }
}

async function main(env) {
  if (env.LOCAL_MODE === 'collect') {
    return collectLocal(env.LOCAL_RECORDS_ROOT, env.LOCAL_RECORDS_FLAT, env.TEST_COHORT);
  }
  requireThat(env.LOCAL_MODE === 'run', 'local_mode');
  const abort = new AbortController();
  const cancel = () => abort.abort();
  for (const name of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.on(name, cancel);
  try {
    await localSequence({contextFile: env.CONTEXT_FILE, cohort: env.TEST_COHORT,
      role: env.TEST_ROLE, jobDeadline: Number(env.JOB_DEADLINE_MS),
      tool: path.join(env.TOOLS_ROOT, 'marmish-installed-check'), workspace: env.MARMISH_WORK,
      assets: env.ASSETS_ROOT, bootstrap: path.join(env.BOOTSTRAP_ROOT, 'install.sh')}, abort.signal);
  } finally {
    for (const name of ['SIGTERM', 'SIGINT', 'SIGHUP']) process.removeListener(name, cancel);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.env).catch(() => {
    process.stderr.write('marmish installed local coordinator: invalid_or_failed\n');
    process.exitCode = 1;
  });
}
