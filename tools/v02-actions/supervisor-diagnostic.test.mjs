import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {EventEmitter} from 'node:events';
import {supervise} from './supervise.mjs';
import {ArtifactTransport, artifactName, binding} from './transport.mjs';

const context = {schema: 1, repository: 'Epiphytic/marmish', run_id: '12', run_attempt: '2',
  source_commit: 'a'.repeat(40), cohort: 'matrix', created_at: 1000, expires_at: 3220};
async function temporary(t) {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-diagnostic-test-'));
  t.after(() => fs.rm(directory, {recursive: true, force: true}));
  return directory;
}

test('download failure emits once before owned child cleanup and preserves the original error', async t => {
  const directory = await temporary(t);
  const child = new EventEmitter();
  const signals = [];
  const captured = [];
  const error = new Error('artifact_http');
  child.kill = signal => {
    signals.push(signal);
    setImmediate(() => child.emit('exit', null, signal));
  };
  await assert.rejects(supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context',
    driver: '/driver', workspace: directory, output: directory, inbox: directory,
    identities: directory, jobDeadline: 4300}, context, now: () => 1100_000,
    artifact: {}, spawnChild: () => child,
    transport: {list: async () => [], download: async () => { throw error; }},
    diagnostic: line => { assert.deepEqual(signals, []); captured.push(line); },
    delay: () => new Promise(resolve => setImmediate(resolve))}), value => value === error);
  assert.deepEqual(signals, ['SIGTERM']);
  assert.equal(captured.length, 1);
  const line = JSON.parse(captured[0].slice('marmish installed diagnostic: '.length));
  assert.deepEqual(line, {schema: 1, component: 'supervisor', event: 'failed', stage: 'remote_download',
    category: 'artifact_http', role: 'linux-amd64', peer_role: 'linux-amd64'});
});

const PREFIX = 'marmish installed diagnostic: ';
const STAGES = ['setup', 'remote_list', 'remote_download', 'inbox_publish', 'local_record_read',
  'local_record_publish', 'child_terminal', 'bounds', 'cleanup'];
const ROLES = ['linux-amd64', 'linux-arm64', 'macos-arm64'];
const CATEGORIES = ['invalid_context', 'invalid_repository', 'invalid_run', 'invalid_source',
  'unsupported_cohort', 'invalid_role', 'invalid_kind', 'invalid_redirect', 'invalid_record_file',
  'record_changed', 'invalid_basename', 'record_conflict', 'archive_oversized', 'invalid_archive',
  'response_oversized', 'response_body_missing', 'missing_api_token', 'artifact_deadline',
  'artifact_budget', 'artifact_http', 'artifact_api_redirect', 'artifact_list_bound',
  'artifact_list_json', 'artifact_list_shape', 'artifact_list_total', 'artifact_list_items',
  'artifact_duplicate', 'artifact_metadata', 'artifact_redirect_missing', 'artifact_digest',
  'artifact_upload', 'invalid_deadline', 'invalid_mode', 'supervisor_cancelled',
  'published_record_changed', 'driver_failed', 'terminal_records_missing', 'local_identity_missing',
  'invalid_action_input', 'invalid_action_path', 'action_context_mismatch', 'unknown_error'];
const SECRET = 'SENTINEL:https://user:password@secret.invalid/path?token=private-npub';

function publicDiagnostic(line) {
  assert.equal(typeof line, 'string');
  assert.ok(line.startsWith(PREFIX));
  assert.ok(!line.startsWith('::'));
  assert.ok(!line.includes('\n') && !line.includes('\r'));
  assert.ok(Buffer.byteLength(line + '\n') <= 1024);
  assert.ok(!line.includes(SECRET));
  const value = JSON.parse(line.slice(PREFIX.length));
  const keys = ['category', 'component', 'event', 'peer_role', 'role', 'schema', 'stage'];
  if (Object.hasOwn(value, 'http_status') || Object.hasOwn(value, 'request_target')) {
    assert.equal(value.category, 'artifact_http');
    assert.ok(Number.isInteger(value.http_status) && value.http_status >= 100 && value.http_status <= 599);
    assert.ok(['api', 'blob'].includes(value.request_target));
    keys.push('http_status', 'request_target');
  }
  assert.deepEqual(Object.keys(value).sort(), keys.sort());
  assert.equal(value.schema, 1);
  assert.equal(value.component, 'supervisor');
  assert.equal(value.event, 'failed');
  assert.ok(STAGES.includes(value.stage));
  assert.ok(CATEGORIES.includes(value.category));
  assert.ok(value.role === null || ROLES.includes(value.role));
  assert.ok(value.peer_role === null || ROLES.includes(value.peer_role));
  return value;
}

function options(directory, extra = {}) {
  return {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: directory, output: directory, inbox: directory, identities: directory,
    jobDeadline: 4300, ...extra};
}
function childFixture() {
  const child = new EventEmitter();
  const signals = [];
  child.kill = signal => { signals.push(signal); setImmediate(() => child.emit('exit', null, signal)); };
  return {child, signals};
}
const immediate = () => new Promise(resolve => setImmediate(resolve));

// Import after the baseline RED test: that test needed only the existing supervise export.
import {supervisorDiagnostic} from './supervisor-diagnostic.mjs';
import {main} from './supervise.mjs';

test('every allowed category and stage produces only the closed bounded public schema', () => {
  for (const stage of STAGES) for (const category of CATEGORIES) {
    const captured = [];
    const diagnostic = supervisorDiagnostic(line => captured.push(line), 'linux-arm64');
    diagnostic.at(stage, 'macos-arm64');
    diagnostic.failed(new Error(category));
    assert.equal(captured.length, 1);
    assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
      stage, category, role: 'linux-arm64', peer_role: 'macos-arm64'});
  }
});

test('secret-bearing unknown errors and hostile getters cannot escape the public schema', () => {
  const throwing = Object.create(null, {message: {get() { throw new Error(SECRET); }}});
  const object = {message: {toString() { throw new Error(SECRET); }, toJSON() { return SECRET; }}};
  for (const error of [new Error(SECRET), SECRET, null, undefined, 0, false, {}, throwing, object,
    new Error(SECRET.repeat(10000))]) {
    const captured = [];
    const diagnostic = supervisorDiagnostic(line => captured.push(line), SECRET);
    diagnostic.at(SECRET, SECRET);
    diagnostic.failed(error);
    diagnostic.failed(new Error('artifact_http'));
    assert.equal(captured.length, 1);
    assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
      stage: 'setup', category: 'unknown_error', role: null, peer_role: null});
  }
  let reads = 0;
  const captured = [];
  supervisorDiagnostic(line => captured.push(line)).failed({get message() {
    return ++reads === 1 ? 'artifact_http' : SECRET;
  }});
  assert.equal(reads, 1);
  assert.equal(publicDiagnostic(captured[0]).category, 'artifact_http');
});

test('unknown child launch throws retain the original value and emit setup-safe finite diagnostics', async t => {
  const directory = await temporary(t);
  for (const error of [null, undefined, false, SECRET, new Error(SECRET)]) {
    const captured = [];
    await assert.rejects(supervise({options: options(directory), context,
      now: () => 1100_000, spawnChild: () => { throw error; },
      diagnostic: line => captured.push(line)}), value => value === error);
    assert.equal(captured.length, 1);
    assert.equal(publicDiagnostic(captured[0]).stage, 'child_terminal');
    assert.equal(publicDiagnostic(captured[0]).category, 'unknown_error');
  }
});

test('supervise pre-try binding and deadline errors emit setup once', async t => {
  const directory = await temporary(t);
  for (const [settings, ctx, category, role] of [
    [options(directory, {role: SECRET}), context, 'invalid_context', null],
    [options(directory), {...context, created_at: SECRET}, 'invalid_deadline', 'linux-amd64'],
    [options(directory, {mode: SECRET}), context, 'invalid_mode', 'linux-amd64'],
  ]) {
    const captured = [];
    await assert.rejects(supervise({options: settings, context: ctx, now: () => 1100_000,
      diagnostic: line => captured.push(line)}), new RegExp(category));
    assert.equal(captured.length, 1);
    assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
      stage: 'setup', category, role, peer_role: null});
  }
});

test('main owns missing input, path and context setup failures exactly once without process-env mutation', async t => {
  const directory = await temporary(t);
  const file = path.join(directory, 'context.json');
  await fs.writeFile(file, JSON.stringify(context));
  const valid = {INPUT_MODE: 'run', INPUT_CONTEXT: file, INPUT_ROLE: 'linux-arm64', INPUT_DRIVER: '/driver',
    INPUT_WORKSPACE: directory, INPUT_IDENTITIES: directory, INPUT_INBOX: directory, INPUT_OUTPUT: directory,
    'INPUT_JOB-DEADLINE': '4300', INPUT_TOKEN: SECRET};
  for (const [env, category, role] of [
    [{}, 'invalid_action_input', null],
    [{...valid, INPUT_DRIVER: SECRET}, 'invalid_action_path', 'linux-arm64'],
    [valid, 'action_context_mismatch', 'linux-arm64'],
  ]) {
    const captured = [];
    await assert.rejects(main({env, diagnostic: line => captured.push(line)}), new RegExp(category));
    assert.equal(captured.length, 1);
    assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
      stage: 'setup', category, role, peer_role: null});
  }
  await fs.writeFile(file, SECRET);
  const captured = [];
  await assert.rejects(main({env: valid, diagnostic: line => captured.push(line)}), SyntaxError);
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).category, 'unknown_error');
});

test('local read, local publication and inbox publication keep their distinct finite stages', async t => {
  const directory = await temporary(t);
  const record = path.join(directory, 'linux-amd64-identity-ready.json');
  const captureFailure = async (transport, artifact, category, stage, peerRole = null) => {
    const captured = [];
    await assert.rejects(supervise({options: options(directory, {mode: 'exchange'}), context,
      now: () => 1100_000, transport, artifact, delay: immediate,
      diagnostic: line => captured.push(line)}), new RegExp(category));
    assert.equal(captured.length, 1);
    const value = publicDiagnostic(captured[0]);
    assert.equal(value.stage, stage); assert.equal(value.category, category); assert.equal(value.peer_role, peerRole);
  };
  await fs.mkdir(record);
  await captureFailure({}, {}, 'invalid_record_file', 'local_record_read');
  await fs.rmdir(record);
  await fs.writeFile(record, '{}');
  await captureFailure({}, {uploadArtifact: async () => ({id: 0})}, 'artifact_upload', 'local_record_publish');
  const identities = path.join(directory, 'inbox');
  await fs.mkdir(identities);
  await fs.writeFile(path.join(identities, 'linux-amd64-identity-ready.json'), '[]');
  const captured = [];
  await assert.rejects(supervise({options: options(directory, {mode: 'exchange', identities}), context,
    now: () => 1100_000, artifact: {uploadArtifact: async () => ({id: 1})}, delay: immediate,
    transport: {list: async () => [], download: async () => Buffer.from('{}')},
    diagnostic: line => captured.push(line)}), /record_conflict/);
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).stage, 'inbox_publish');
  assert.equal(publicDiagnostic(captured[0]).peer_role, 'linux-amd64');
});

test('loop deadline diagnostics use bounds rather than setup', async t => {
  const directory = await temporary(t);
  let calls = 0;
  const captured = [];
  await assert.rejects(supervise({options: options(directory, {mode: 'exchange'}), context,
    now: () => ++calls === 1 ? 1100_000 : 3220_001,
    diagnostic: line => captured.push(line)}), /artifact_deadline/);
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).stage, 'bounds');
});

test('sink throws and rejections never replace the failure or skip owned child cleanup', async t => {
  const directory = await temporary(t);
  for (const sink of [() => { throw new Error(SECRET); }, async () => { throw new Error(SECRET); }]) {
    const {child, signals} = childFixture();
    const error = new Error('artifact_http');
    await assert.rejects(supervise({options: options(directory), context, now: () => 1100_000,
      spawnChild: () => child, artifact: {}, delay: immediate, diagnostic: sink,
      transport: {list: async () => [], download: async () => { throw error; }}}), value => value === error);
    assert.deepEqual(signals, ['SIGTERM']);
    await immediate();
  }
});

test('cleanup failure preserves the initiating diagnostic and existing returned-error precedence', async t => {
  const directory = await temporary(t);
  const {child, signals} = childFixture();
  const cleanupError = new Error(SECRET);
  const captured = [];
  const remove = fs.rm;
  const mock = t.mock.method(fs, 'rm', async (...args) => { await remove(...args); throw cleanupError; });
  try {
    await assert.rejects(supervise({options: options(directory), context, now: () => 1100_000,
      spawnChild: () => child, artifact: {}, delay: immediate,
      diagnostic: line => { assert.deepEqual(signals, []); captured.push(line); },
      transport: {list: async () => [], download: async () => { throw new Error('artifact_http'); }}}),
    value => value === cleanupError);
  } finally { mock.mock.restore(); }
  assert.deepEqual(signals, ['SIGTERM']);
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).stage, 'remote_download');
  assert.equal(publicDiagnostic(captured[0]).category, 'artifact_http');
});

test('cleanup-only failure emits once and successful exchange remains silent', async t => {
  const directory = await temporary(t);
  await fs.writeFile(path.join(directory, 'linux-amd64-identity-ready.json'), '{}');
  const captured = [];
  const run = () => supervise({options: options(directory, {mode: 'exchange',
    identities: path.join(directory, 'identities')}), context, now: () => 1100_000,
    artifact: {uploadArtifact: async () => ({id: 1})}, delay: immediate,
    transport: {list: async () => [], download: async () => Buffer.from('{}')},
    diagnostic: line => captured.push(line)});
  await run();
  assert.deepEqual(captured, []);
  const error = new Error(SECRET);
  const remove = fs.rm;
  const mock = t.mock.method(fs, 'rm', async (...args) => { await remove(...args); throw error; });
  try { await assert.rejects(run(), value => value === error); }
  finally { mock.mock.restore(); }
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).stage, 'cleanup');
  assert.equal(publicDiagnostic(captured[0]).category, 'unknown_error');
});

test('download error racing the final local PASS retains the remote download stage', async t => {
  const directory = await temporary(t);
  const {child, signals} = childFixture();
  const error = new Error('artifact_http');
  const captured = [];
  let rejectDownload;
  let uploads = 0;
  await assert.rejects(supervise({options: options(directory), context, now: () => 1100_000,
    spawnChild: () => child, delay: immediate, diagnostic: line => captured.push(line),
    artifact: {uploadArtifact: async () => {
      if (++uploads === 2) setTimeout(() => rejectDownload(error), 40);
      return {id: uploads};
    }}, transport: {list: async () => [], download: () => new Promise((resolve, reject) => {
      rejectDownload = reject;
      (async () => {
        for (const kind of ['client-result', 'server-result']) {
          await fs.writeFile(path.join(directory, `linux-amd64-${kind}.json`), '{}');
        }
        child.emit('exit', 0, null);
      })().catch(reject);
    })}}), value => value === error);
  assert.equal(uploads, 2);
  assert.deepEqual(signals, []);
  assert.equal(captured.length, 1);
  assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
    stage: 'remote_download', category: 'artifact_http', role: 'linux-amd64', peer_role: 'linux-amd64'});
});

test('main constructor setup failure is emitted before its listener cleanup', async t => {
  const directory = await temporary(t);
  const file = path.join(directory, 'context.json');
  await fs.writeFile(file, JSON.stringify(context));
  const env = {INPUT_MODE: 'run', INPUT_CONTEXT: file, INPUT_ROLE: 'linux-arm64', INPUT_DRIVER: '/driver',
    INPUT_WORKSPACE: directory, INPUT_IDENTITIES: directory, INPUT_INBOX: directory, INPUT_OUTPUT: directory,
    'INPUT_JOB-DEADLINE': '4300', INPUT_TOKEN: {length: 1}, GITHUB_REPOSITORY: context.repository,
    GITHUB_RUN_ID: context.run_id, GITHUB_RUN_ATTEMPT: context.run_attempt};
  const captured = [];
  const interrupts = process.listenerCount('SIGINT');
  const terminations = process.listenerCount('SIGTERM');
  await assert.rejects(main({env, diagnostic: line => captured.push(line)}), /missing_api_token/);
  assert.equal(process.listenerCount('SIGINT'), interrupts);
  assert.equal(process.listenerCount('SIGTERM'), terminations);
  assert.equal(captured.length, 1);
  assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
    stage: 'setup', category: 'missing_api_token', role: 'linux-arm64', peer_role: null});
});

test('supervise temporary-directory setup failure emits once before a child is created', async t => {
  const directory = await temporary(t);
  const captured = [];
  const error = new Error(SECRET);
  const mock = t.mock.method(fs, 'mkdtemp', async () => { throw error; });
  try {
    await assert.rejects(supervise({options: options(directory), context, now: () => 1100_000,
      spawnChild: () => assert.fail('no child before setup'), diagnostic: line => captured.push(line)}),
    value => value === error);
  } finally { mock.mock.restore(); }
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).stage, 'setup');
  assert.equal(publicDiagnostic(captured[0]).category, 'unknown_error');
});


test('list observations use a separate closed schema without changing terminal diagnostic state', () => {
  const captured = [];
  const diagnostic = supervisorDiagnostic(line => captured.push(line), 'linux-arm64');
  diagnostic.at('local_record_read', 'macos-arm64');
  diagnostic.listObservation({artifactList: {reason: 'count_mismatch', total_count: 3, artifact_count: 2}});
  assert.equal(captured.length, 1);
  assert.ok(Buffer.byteLength(captured[0] + '\n') <= 1024);
  assert.deepEqual(JSON.parse(captured[0].slice(PREFIX.length)), {schema: 1, component: 'supervisor',
    event: 'artifact_list_observation', stage: 'remote_list', role: 'linux-arm64', peer_role: null,
    reason: 'count_mismatch', total_count: 3, artifact_count: 2});
  diagnostic.failed(new Error('published_record_changed'));
  diagnostic.failed(new Error('artifact_http'));
  assert.equal(captured.length, 2);
  assert.deepEqual(publicDiagnostic(captured[1]), {schema: 1, component: 'supervisor', event: 'failed',
    stage: 'local_record_read', category: 'published_record_changed', role: 'linux-arm64', peer_role: 'macos-arm64'});
});

test('list observation rejects hostile getters and non-whitelisted descriptors without leaking', () => {
  const captured = [];
  const diagnostic = supervisorDiagnostic(line => captured.push(line), 'linux-amd64');
  diagnostic.listObservation({get artifactList() { throw new Error(SECRET); }});
  for (const change of [{reason: SECRET}, {total_count: Number.MAX_SAFE_INTEGER + 1},
    {artifact_count: 101}, {total_count: SECRET}]) {
    diagnostic.listObservation({artifactList: {reason: 'count_mismatch', total_count: 3, artifact_count: 2, ...change}});
  }
  assert.deepEqual(captured, []);
});


test('HTTP failure detail has a closed paired schema and rejects malformed or hostile metadata', () => {
  const throwing = new Proxy({}, {get() { throw new Error(SECRET); }});
  const metadata = [null, undefined, SECRET, {}, {status: 404}, {target: 'api'},
    ...[99, 600, 404.5, NaN, Infinity, '404', SECRET].map(status => ({status, target: 'api'})),
    ...[null, 0, {}, SECRET].map(target => ({status: 404, target})), throwing];
  for (const observation of metadata) {
    const captured = [];
    const error = Object.assign(new Error('artifact_http'), {artifactHttp: observation});
    supervisorDiagnostic(line => captured.push(line)).failed(error);
    assert.equal(captured.length, 1);
    assert.equal(Object.hasOwn(publicDiagnostic(captured[0]), 'http_status'), false);
  }
  const captured = [];
  supervisorDiagnostic(line => captured.push(line)).failed(Object.defineProperty(
    new Error('artifact_http'), 'artifactHttp', {get() { throw new Error(SECRET); }}));
  assert.equal(captured.length, 1);
  assert.equal(Object.hasOwn(publicDiagnostic(captured[0]), 'http_status'), false);
  for (const target of ['api', 'blob']) for (const status of [100, 204, 304, 404, 599]) {
    const captured = [];
    const error = Object.assign(new Error('artifact_http'),
      {artifactHttp: {status, target, secret: SECRET, toJSON() { throw new Error(SECRET); }}});
    supervisorDiagnostic(line => captured.push(line)).failed(error);
    const result = publicDiagnostic(captured[0]);
    assert.equal(result.http_status, status);
    assert.equal(result.request_target, target);
  }
});

test('HTTP metadata and its values are read once and never touched for other categories', () => {
  let metadataReads = 0, statusReads = 0, targetReads = 0;
  const observation = {
    get status() { return ++statusReads === 1 ? 404 : SECRET; },
    get target() { return ++targetReads === 1 ? 'blob' : SECRET; },
  };
  const error = Object.defineProperty(new Error('artifact_http'), 'artifactHttp',
    {get() { return ++metadataReads === 1 ? observation : SECRET; }});
  const captured = [];
  const diagnostic = supervisorDiagnostic(line => captured.push(line));
  diagnostic.failed(error); diagnostic.failed(error);
  assert.deepEqual([metadataReads, statusReads, targetReads], [1, 1, 1]);
  assert.equal(captured.length, 1);
  assert.equal(publicDiagnostic(captured[0]).http_status, 404);
  assert.equal(publicDiagnostic(captured[0]).request_target, 'blob');
  for (const category of ['artifact_budget', SECRET]) {
    let reads = 0;
    const lines = [];
    supervisorDiagnostic(line => lines.push(line)).failed(Object.defineProperty(
      new Error(category), 'artifactHttp', {get() { reads++; throw new Error('must not read'); }}));
    assert.equal(reads, 0);
    assert.equal(Object.hasOwn(publicDiagnostic(lines[0]), 'http_status'), false);
  }
});

test('actual blob HTTP failure emits detail before child cleanup', async t => {
  const directory = await temporary(t);
  const created = Math.floor(Date.now() / 1000);
  const ctx = {...context, created_at: created, expires_at: created + 2220};
  const entry = {name: artifactName(binding(ctx, 'linux-amd64'), 'linux-amd64', 'server-ready'),
    id: 42, expired: false, size_in_bytes: 100, digest: `sha256:${'b'.repeat(64)}`};
  let requests = 0;
  const transport = new ArtifactTransport({context: ctx, token: SECRET,
    budgetFile: path.join(directory, 'budget'), fetcher: async () => {
      requests++;
      if (requests === 1) return new Response(JSON.stringify({total_count: 1, artifacts: [entry]}));
      if (requests === 2) return new Response(null, {status: 302,
        headers: {location: 'https://x.blob.core.windows.net/private?secret'}});
      return new Response(null, {status: 404});
    }});
  const {child, signals} = childFixture();
  const captured = [];
  await assert.rejects(supervise({options: options(directory, {jobDeadline: created + 3300}),
    context: ctx, now: Date.now, artifact: {}, transport, spawnChild: () => child, delay: immediate,
    diagnostic: line => { assert.deepEqual(signals, []); captured.push(line); }}), error => {
    assert.equal(error.message, 'artifact_http');
    assert.deepEqual(error.artifactHttp, {status: 404, target: 'blob'});
    return true;
  });
  assert.deepEqual(signals, ['SIGTERM']);
  assert.equal(requests, 3);
  assert.equal(transport.used, 2);
  assert.equal(captured.length, 1);
  assert.deepEqual(publicDiagnostic(captured[0]), {schema: 1, component: 'supervisor', event: 'failed',
    stage: 'remote_download', category: 'artifact_http', role: 'linux-amd64', peer_role: 'linux-amd64',
    http_status: 404, request_target: 'blob'});
});

test('listed ZIP API 404 retries terminate with closed deadline or shared budget diagnostics', async t => {
  for (const used of [0, 118]) {
    const directory = await temporary(t);
    const created = Math.floor(Date.now() / 1000);
    const ctx = {...context, created_at: created, expires_at: created + 2220};
    const identities = path.join(directory, 'identities');
    const budgetFile = path.join(directory, 'budget');
    await fs.writeFile(budgetFile, JSON.stringify({used}));
    await fs.writeFile(path.join(directory, 'linux-amd64-identity-ready.json'), '{}');
    const entry = {name: artifactName(binding(ctx, 'linux-amd64'), 'linux-amd64', 'identity-ready'),
      id: 42, expired: false, size_in_bytes: 100, digest: `sha256:${'b'.repeat(64)}`};
    let clock = (created + 100) * 1000, active = 0, downloads = 0;
    const polls = [], captured = [];
    const transport = new ArtifactTransport({context: ctx, token: SECRET, budgetFile,
      fetcher: async url => {
        if (String(url).endsWith('/zip')) {
          downloads++;
          return new Response(null, {status: 404});
        }
        return new Response(JSON.stringify({total_count: 1, artifacts: [entry]}));
      }});
    for (const method of ['list', 'download']) {
      const original = transport[method].bind(transport);
      transport[method] = async (...args) => {
        if (method === 'list') polls.push(clock);
        active++;
        try { return await original(...args); } finally { active--; }
      };
    }
    const category = used ? 'artifact_budget' : 'artifact_deadline';
    await assert.rejects(supervise({options: options(directory,
      {mode: 'exchange', identities, jobDeadline: created + 3300}), context: ctx,
      now: () => clock, artifact: {uploadArtifact: async () => ({id: 1})}, transport,
      delay: async () => { await immediate(); if (!active) clock += 1000; },
      diagnostic: line => captured.push(line)}), {message: category});
    assert.ok(downloads >= 1);
    assert.ok(polls.length >= 2);
    for (let index = 1; index < polls.length; index++) assert.ok(polls[index] - polls[index - 1] >= 30_000);
    await assert.rejects(fs.access(identities), {code: 'ENOENT'});
    assert.equal(captured.length, 1);
    const result = publicDiagnostic(captured[0]);
    assert.equal(result.category, category);
    assert.equal(result.stage, used ? 'remote_list' : 'bounds');
    assert.equal(Object.hasOwn(result, 'http_status'), false);
    if (used) assert.equal(JSON.parse(await fs.readFile(budgetFile)).used, 120);
    else assert.ok(transport.used < 120);
  }
});
