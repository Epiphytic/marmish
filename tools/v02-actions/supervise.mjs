// Local JavaScript action: CI transport only, never installed with Marmish.
import {BoundedArtifactClient} from './upload-client.mjs';
import {spawn} from 'node:child_process';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {ArtifactTransport, ROLES, artifactName, binding, boundedFile,
  childEnvironment, digest, putImmutable, requireThat} from './transport.mjs';

import {isStreamingCohort, STREAMING_BOUNDS} from './streaming-policy.mjs';
import {supervisorDiagnostic} from './supervisor-diagnostic.mjs';

export function timing(context, jobDeadline, mode, now = Date.now()) {
  const streaming = isStreamingCohort(context.cohort);
  const lifetime = streaming ? STREAMING_BOUNDS.lifetime : 2220;
  const reserve = streaming ? STREAMING_BOUNDS.watchdogReserve : 900;
  const cleanup = streaming ? STREAMING_BOUNDS.artifactCleanup : 600;
  requireThat(Number.isSafeInteger(context.created_at) && Number.isSafeInteger(context.expires_at)
    && context.expires_at > context.created_at && context.expires_at - context.created_at <= lifetime
    && context.created_at * 1000 <= now + 5000 && Number.isSafeInteger(jobDeadline)
    && context.expires_at + reserve <= jobDeadline, 'invalid_deadline');
  const deadline = Math.min(context.expires_at, mode === 'exchange' ? context.created_at + 900 : Infinity) * 1000;
  requireThat(now < deadline, 'artifact_deadline');
  return {deadline, cleanupDeadline: (context.expires_at + cleanup) * 1000};
}

export function ownedChild(child) {
  let settled = false;
  let result;
  let signalled = false;
  const joined = new Promise(resolve => {
    const finish = value => { if (!settled) { settled = true; result = value; resolve(value); } };
    child.once('error', () => finish({code: null, signal: 'spawn_error'}));
    child.once('exit', (code, signal) => finish({code, signal}));
  });
  return {
    get result() { return result; },
    joined,
    terminate() { if (!settled && !signalled) { signalled = true; child.kill('SIGTERM'); } },
    force() { if (!settled) child.kill('SIGKILL'); },
  };
}

// Remote API polling and local publication have independent clocks. A long
// Retry-After must never prevent the driver's terminal records being uploaded.
export async function supervise(args) {
  const diagnostic = args.diagnosticState ?? supervisorDiagnostic(args.diagnostic, args.options?.role);
  try { return await superviseOwned(args, diagnostic); }
  catch (error) { diagnostic.failed(error); throw error; }
}

async function superviseOwned({options, context, transport, artifact, spawnChild = spawn,
  now = Date.now, delay, signal, cancellation = {requested: false}}, diagnostic) {
  const prefix = binding(context, options.role);
  requireThat(['exchange', 'run'].includes(options.mode), 'invalid_mode');
  const bounds = timing(context, options.jobDeadline, options.mode, now());
  const stage = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-actions-'));
  const controller = new AbortController();
  const published = new Map();
  const received = new Set();
  const outgoing = options.mode === 'exchange' ? ['identity-ready'] : ['server-ready', 'client-result', 'server-result'];
  const incoming = options.mode === 'exchange' ? ['identity-ready'] : ['server-ready', 'client-result'];
  let owner, remote, remoteError, remoteFailure;
  let nextPoll = 0;
  let success = false;
  let wake = () => {};
  const cancel = () => { owner?.terminate(); controller.abort(); wake(); };
  signal?.addEventListener('abort', cancel, {once: true});
  const pause = async ms => {
    if (delay) { await delay(ms); return; }
    await new Promise(resolve => {
      const timer = setTimeout(done, ms);
      function done() { clearTimeout(timer); wake = () => {}; resolve(); }
      wake = done;
      if (controller.signal.aborted) done();
    });
  };
  const poll = async () => {
    let interval = 30_000;
    let stage = 'remote_list';
    let peerRole = null;
    try {
      const entries = await transport.list(bounds.deadline, controller.signal);
      for (const role of ROLES) for (const kind of incoming) {
        controller.signal.throwIfAborted();
        const name = artifactName(prefix, role, kind);
        if (received.has(name)) continue;
        stage = 'remote_download'; peerRole = role;
        const bytes = await transport.download(entries, name, `${kind}.json`, bounds.deadline, controller.signal, true);
        if (bytes) {
          stage = 'inbox_publish';
          await putImmutable(options.mode === 'exchange' ? options.identities : options.inbox,
            `${role}-${kind}.json`, bytes);
          received.add(name);
        }
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        if (stage === 'remote_list') diagnostic.listObservation(error);
        if (error.message === 'artifact_retry') interval = Math.max(interval, error.retryAfter ?? 0);
        else { remoteError = error; remoteFailure = {stage, peerRole}; }
      }
    } finally { nextPoll = now() + interval; wake(); }
  };
  const finishSuccess = async () => {
    // A poll already in flight must settle before success. Aborting it first
    // could mask a malformed artifact that raced the final local upload.
    if (remote) await remote;
    if (remoteError) throw remoteError;
    diagnostic.at('bounds');
    requireThat(!controller.signal.aborted && !cancellation.requested, 'supervisor_cancelled');
    success = true;
  };
  try {
    if (options.mode === 'run') {
      diagnostic.at('child_terminal');
      owner = ownedChild(spawnChild(options.driver, ['run', '--context', options.context,
        '--workspace', options.workspace, '--identities', options.identities,
        '--inbox', options.inbox, '--output', options.output],
      {env: childEnvironment(process.env), stdio: ['ignore', 'inherit', 'inherit']}));
      owner.joined.then(() => wake());
    }
    if (signal?.aborted || cancellation.requested) cancel();
    for (;;) {
      diagnostic.at('bounds');
      requireThat(!controller.signal.aborted && !cancellation.requested, 'supervisor_cancelled');
      requireThat(now() < bounds.deadline, 'artifact_deadline');
      if (remoteError) throw remoteError;
      // Snapshot before any awaited reads: an exit during this scan requires
      // another complete scan, rather than falsely declaring a terminal missing.
      const exitedBeforeScan = owner?.result;
      for (const kind of outgoing) {
        diagnostic.at('local_record_read');
        let bytes;
        try { bytes = await boundedFile(path.join(options.output, `${options.role}-${kind}.json`)); }
        catch (error) {
          if (error.code === 'ENOENT' && !published.has(kind)) continue;
          throw error;
        }
        const hash = digest(bytes);
        if (published.has(kind)) {
          requireThat(published.get(kind) === hash, 'published_record_changed');
          continue;
        }
        diagnostic.at('local_record_publish');
        const directory = path.join(stage, kind);
        await fs.mkdir(directory, {mode: 0o700});
        const copy = path.join(directory, `${kind}.json`);
        await fs.writeFile(copy, bytes, {flag: 'wx', mode: 0o600});
        const uploaded = await artifact.uploadArtifact(artifactName(prefix, options.role, kind),
          [copy], directory, {retentionDays: 14, signal: controller.signal});
        requireThat(Number.isSafeInteger(uploaded.id) && uploaded.id > 0, 'artifact_upload');
        published.set(kind, hash);
      }
      if (exitedBeforeScan) {
        diagnostic.at('child_terminal');
        requireThat(exitedBeforeScan.code === 0 && exitedBeforeScan.signal === null, 'driver_failed');
        requireThat(published.has('client-result') && published.has('server-result'), 'terminal_records_missing');
        await finishSuccess();
        return;
      }
      if (options.mode === 'exchange' && received.size === ROLES.length) {
        diagnostic.at('local_record_publish');
        requireThat(published.has('identity-ready'), 'local_identity_missing');
        await finishSuccess();
        return;
      }
      if (!owner?.result && !remote && received.size < ROLES.length * incoming.length && now() >= nextPoll) {
        remote = poll().finally(() => { remote = undefined; });
      }
      await pause(owner?.result ? 1 : 1000);
    }
  } catch (error) {
    if (remoteFailure && error === remoteError) diagnostic.at(remoteFailure.stage, remoteFailure.peerRole);
    diagnostic.failed(error);
    throw error;
  } finally {
    diagnostic.at('cleanup');
    controller.abort(); wake();
    if (!success) owner?.terminate();
    signal?.removeEventListener('abort', cancel);
    if (remote) await remote;
    if (owner && !success && !owner.result) {
      owner.terminate();
      const remaining = Math.max(1, bounds.cleanupDeadline - now());
      let timeout;
      const ended = await Promise.race([owner.joined.then(() => true),
        new Promise(resolve => { timeout = setTimeout(() => resolve(false), remaining); })]);
      clearTimeout(timeout);
      if (!ended) {
        owner.force();
        let lastTimeout;
        await Promise.race([owner.joined, new Promise(resolve => { lastTimeout = setTimeout(resolve, 5000); })]);
        clearTimeout(lastTimeout);
      }
    }
    await fs.rm(stage, {recursive: true, force: true});
  }
}

export async function main({diagnostic, env = process.env} = {}) {
  const reporter = supervisorDiagnostic(diagnostic);
  try { await mainOwned(env, reporter); }
  catch (error) { reporter.failed(error); throw error; }
}

async function mainOwned(env, diagnostic) {
  const input = name => {
    const value = env[`INPUT_${name.toUpperCase()}`];
    requireThat(value && value.length <= 4096, 'invalid_action_input');
    return value;
  };
  const options = Object.fromEntries(['mode', 'context', 'role', 'driver', 'workspace', 'identities', 'inbox', 'output']
    .map(key => {
      const value = input(key);
      if (key === 'role') diagnostic.role(value);
      return [key, value];
    }));
  options.jobDeadline = Number(input('job-deadline'));
  for (const key of ['context', 'driver', 'workspace', 'identities', 'inbox', 'output']) {
    requireThat(path.isAbsolute(options[key]), 'invalid_action_path');
  }
  const token = input('token');
  const context = JSON.parse(await boundedFile(options.context, 32768));
  requireThat(env.GITHUB_REPOSITORY === context.repository
    && env.GITHUB_RUN_ID === String(context.run_id)
    && env.GITHUB_RUN_ATTEMPT === String(context.run_attempt), 'action_context_mismatch');
  const controller = new AbortController();
  const cancel = () => controller.abort();
  process.on('SIGINT', cancel);
  process.on('SIGTERM', cancel);
  try {
    const transport = new ArtifactTransport({context, token,
      budgetFile: path.join(options.workspace, '.actions-budget.json')});
    await supervise({options, context, transport, artifact: new BoundedArtifactClient(), signal: controller.signal, diagnosticState: diagnostic});
  } catch (error) {
    diagnostic.failed(error);
    throw error;
  } finally {
    diagnostic.at('cleanup');
    process.removeListener('SIGINT', cancel);
    process.removeListener('SIGTERM', cancel);
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(() => {
    // No error objects, URLs, tokens, profile paths, payloads or stack traces.
    console.error('marmish installed supervisor: failed; preserve private state and inspect sanitized records');
    process.exitCode = 1;
  });
}
