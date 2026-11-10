import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {EventEmitter} from 'node:events';
import {ArtifactTransport, artifactName, binding, childEnvironment, digest,
  putImmutable, safeBlobUrl, singleJson} from './transport.mjs';
import {ownedChild, supervise, timing} from './supervise.mjs';

const context = {schema: 1, repository: 'Epiphytic/marmish', run_id: '12', run_attempt: '2',
  source_commit: 'a'.repeat(40), cohort: 'matrix', created_at: 1000, expires_at: 3220};
const one = Buffer.from('UEsDBBQAAAAAABIGY12MwfuJDAAAAAwAAAATAAAAaWRlbnRpdHktcmVhZHkuanNvbnsic2NoZW1hIjoxfVBLAQIUAxQAAAAAABIGY12MwfuJDAAAAAwAAAATAAAAAAAAAAAAAACAAQAAAABpZGVudGl0eS1yZWFkeS5qc29uUEsFBgAAAAABAAEAQQAAAD0AAAAAAA==', 'base64');
const traversal = Buffer.from('UEsDBBQAAAAAABIGY11Dv6ajAgAAAAIAAAAWAAAALi4vaWRlbnRpdHktcmVhZHkuanNvbnt9UEsBAhQDFAAAAAAAEgZjXUO/pqMCAAAAAgAAABYAAAAAAAAAAAAAAIABAAAAAC4uL2lkZW50aXR5LXJlYWR5Lmpzb25QSwUGAAAAAAEAAQBEAAAANgAAAAAA', 'base64');
const two = Buffer.from('UEsDBBQAAAAAABIGY11Dv6ajAgAAAAIAAAATAAAAaWRlbnRpdHktcmVhZHkuanNvbnt9UEsDBBQAAAAAABIGY12DFtyMAQAAAAEAAAAFAAAAZXh0cmF4UEsBAhQDFAAAAAAAEgZjXUO/pqMCAAAAAgAAABMAAAAAAAAAAAAAAIABAAAAAGlkZW50aXR5LXJlYWR5Lmpzb25QSwECFAMUAAAAAAASBmNdgxbcjAEAAAABAAAABQAAAAAAAAAAAAAAgAEzAAAAZXh0cmFQSwUGAAAAAAIAAgB0AAAAVwAAAAAA', 'base64');
async function temporary(t) {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-action-test-'));
  t.after(() => fs.rm(directory, {recursive: true, force: true}));
  return directory;
}

test('artifact name binds run attempt and fixed role/kind', () => {
  const name = artifactName(binding(context, 'linux-amd64'), 'linux-amd64', 'client-result');
  assert.match(name, /^v02-installed-12-2-aaaaaaaaaaaa-matrix-linux-amd64-client-result$/);
  assert.notEqual(name, artifactName(binding({...context, run_attempt: '3'}, 'linux-amd64'), 'linux-amd64', 'client-result'));
  assert.throws(() => binding({...context, repository: '../bad'}, 'linux-amd64'));
  assert.throws(() => artifactName('x', '../bad', 'client-result'));
});
test('deadline enforces identity barrier and cleanup margin', () => {
  assert.equal(timing(context, 4300, 'exchange', 1100_000).deadline, 1900_000);
  assert.equal(timing(context, 4300, 'run', 1100_000).deadline, 3220_000);
  assert.throws(() => timing(context, 4119, 'run', 1100_000));
  assert.throws(() => timing(context, 4300, 'exchange', 1900_000));
});
test('child gets only explicit safe environment', () => {
  assert.deepEqual(childEnvironment({HOME: '/safe', TMPDIR: '/tmp', LANG: 'C', PATH: '/cargo/bin',
    GITHUB_TOKEN: 'secret', ACTIONS_RUNTIME_TOKEN: 'secret', LD_PRELOAD: 'bad', DYLD_LIBRARY_PATH: 'bad'}),
  {HOME: '/safe', TMPDIR: '/tmp', LANG: 'C', PATH: '/usr/bin:/bin:/usr/sbin:/sbin'});
});
test('ZIP accepts one exact JSON and refuses traversal, extra members and symlinks', async () => {
  assert.equal((await singleJson(one, 'identity-ready.json')).toString(), '{"schema":1}');
  await assert.rejects(singleJson(one, 'client-result.json'));
  await assert.rejects(singleJson(traversal, 'identity-ready.json'));
  await assert.rejects(singleJson(two, 'identity-ready.json'));
  const link = Buffer.from(one);
  const central = link.indexOf(Buffer.from([0x50, 0x4b, 0x01, 0x02]));
  link.writeUInt32LE((0o120777 << 16) >>> 0, central + 38);
  await assert.rejects(singleJson(link, 'identity-ready.json'));
});
test('inbox publication never overwrites conflicting bytes', async t => {
  const dir = await temporary(t);
  await putImmutable(dir, 'one.json', Buffer.from('{}'));
  await putImmutable(dir, 'one.json', Buffer.from('{}'));
  await assert.rejects(putImmutable(dir, 'one.json', Buffer.from('[]')), /record_conflict/);
  assert.equal(await fs.readFile(path.join(dir, 'one.json'), 'utf8'), '{}');
});
test('redirect validation refuses credentials, custom port, alternate host and cleartext', () => {
  assert.equal(safeBlobUrl('https://productionresult.blob.core.windows.net/a?sas=private').hostname,
    'productionresult.blob.core.windows.net');
  for (const url of ['https://evil.example/a', 'http://x.blob.core.windows.net/a',
    'https://u:p@x.blob.core.windows.net/a', 'https://x.blob.core.windows.net:444/a']) {
    assert.throws(() => safeBlobUrl(url));
  }
});
test('download binds ZIP digest and never forwards API token to blob storage', async t => {
  const dir = await temporary(t);
  const calls = [];
  const transport = new ArtifactTransport({context, token: 'private-token', budgetFile: path.join(dir, 'budget'),
    fetcher: async (url, options) => {
      calls.push({url: String(url), options});
      return calls.length === 1 ? new Response(null, {status: 302,
        headers: {location: 'https://x.blob.core.windows.net/archive?private'}}) : new Response(one);
    }});
  const entry = {name: 'expected', id: 42, expired: false, size_in_bytes: one.length, digest: `sha256:${digest(one)}`};
  assert.equal((await transport.download([entry], 'expected', 'identity-ready.json', Date.now() + 30_000)).toString(), '{"schema":1}');
  assert.equal(calls[0].options.headers.Authorization, 'Bearer private-token');
  assert.deepEqual(calls[1].options.headers, {});
  assert.equal(calls[1].options.redirect, 'manual');
  await assert.rejects(transport.download([entry, entry], 'expected', 'identity-ready.json', Date.now() + 30_000), /duplicate/);
  await assert.rejects(transport.download([{...entry, expired: true}], 'expected', 'identity-ready.json', Date.now() + 30_000), /metadata/);
});
test('list refuses silent truncation and persisted call budget cannot reset between modes', async t => {
  const dir = await temporary(t);
  const file = path.join(dir, 'budget');
  await fs.writeFile(file, '{"used":119}');
  const fetcher = async () => new Response('{"total_count":101,"artifacts":[]}');
  const first = new ArtifactTransport({context, token: 'x', budgetFile: file, fetcher});
  await assert.rejects(first.list(Date.now() + 1000), /artifact_list_total/);
  const second = new ArtifactTransport({context, token: 'x', budgetFile: file, fetcher});
  await assert.rejects(second.list(Date.now() + 1000), /artifact_budget/);
});
test('owned child receives TERM once and preserves actual joined status', async () => {
  const child = new EventEmitter();
  const signals = [];
  child.kill = signal => signals.push(signal);
  const owner = ownedChild(child);
  owner.terminate(); owner.terminate();
  assert.deepEqual(signals, ['SIGTERM']);
  child.emit('exit', 7, null);
  assert.deepEqual(await owner.joined, {code: 7, signal: null});
  owner.force();
  assert.deepEqual(signals, ['SIGTERM']);
});
test('driver terminal records are uploaded during run and exit failure remains red', async t => {
  const dir = await temporary(t);
  const options = {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: dir, output: dir, inbox: path.join(dir, 'inbox'), identities: '/identities', jobDeadline: 4300};
  for (const kind of ['server-ready', 'client-result', 'server-result']) await fs.writeFile(path.join(dir, `linux-amd64-${kind}.json`), '{}');
  const published = [];
  const child = new EventEmitter();
  child.kill = () => { setImmediate(() => child.emit('exit', null, 'SIGTERM')); };
  const artifact = {uploadArtifact: async (name, files, root) => {
    assert.equal(path.dirname(files[0]), root);
    assert.equal(await fs.readFile(files[0], 'utf8'), '{}');
    published.push(name);
    if (name.endsWith('server-result')) child.emit('exit', 7, null);
    return {id: published.length};
  }};
  await assert.rejects(supervise({options, context, artifact, now: () => 1100_000,
    transport: {list: () => { throw new Error('unexpected_download'); }},
    spawnChild: (_exe, args, opts) => {
      assert.equal(args[0], 'run'); assert.equal(opts.env.GITHUB_TOKEN, undefined); return child;
    }}), /driver_failed/);
  assert.equal(published.length, 3);
});
test('zero driver exit without both terminal records remains failure', async t => {
  const dir = await temporary(t);
  const child = new EventEmitter();
  child.kill = () => {};
  const options = {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300};
  await assert.rejects(supervise({options, context, now: () => 1100_000,
    artifact: {uploadArtifact: () => { throw new Error('unexpected'); }},
    transport: {list: async () => [], download: async () => null},
    delay: async () => { child.emit('exit', 0, null); }, spawnChild: () => child}), /terminal_records_missing/);
});
test('cancellation retains owned child until its actual exit', async t => {
  const dir = await temporary(t);
  const child = new EventEmitter();
  const signals = [];
  let joined = false;
  child.kill = signal => {
    signals.push(signal);
    setImmediate(() => { joined = true; child.emit('exit', null, 'SIGTERM'); });
  };
  const options = {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300};
  await assert.rejects(supervise({options, context, now: () => 1100_000, artifact: {}, transport: {},
    cancellation: {requested: true}, spawnChild: () => child}), /supervisor_cancelled/);
  assert.equal(joined, true);
  assert.deepEqual(signals, ['SIGTERM']);
});
test('published record mutation is refused after the immutable staging upload', async t => {
  const dir = await temporary(t);
  const record = path.join(dir, 'linux-amd64-identity-ready.json');
  await fs.writeFile(record, '{}');
  const options = {mode: 'exchange', role: 'linux-amd64', workspace: dir, output: dir,
    identities: path.join(dir, 'identities'), jobDeadline: 4300};
  await assert.rejects(supervise({options, context, now: () => 1100_000,
    artifact: {uploadArtifact: async (_name, files) => {
      await fs.writeFile(record, '[]');
      assert.equal(await fs.readFile(files[0], 'utf8'), '{}'); return {id: 1};
    }}, transport: {list: async () => [], download: async () => null}, delay: async () => {}}), /published_record_changed/);
});

test('only network/ratelimit failures retry; malformed responses fail immediately', async t => {
  const dir = await temporary(t);
  let count = 0;
  const make = fetcher => new ArtifactTransport({context, token: 'x',
    budgetFile: path.join(dir, `budget-${count++}`), fetcher});
  const deadline = Date.now() + 30_000;
  await assert.rejects(make(async () => { throw new TypeError('fetch failed'); }).list(deadline), /artifact_retry/);
  const limited = make(async () => new Response(null, {status: 429, headers: {'retry-after': '300'}}));
  await assert.rejects(limited.list(deadline), error => error.message === 'artifact_retry' && error.retryAfter === 300_000);
  assert.equal(limited.used, 1);
  const forbidden = make(async () => new Response(null, {status: 403}));
  await assert.rejects(forbidden.list(deadline), error => {
    assert.equal(error.message, 'artifact_http');
    assert.deepEqual(error.artifactHttp, {status: 403, target: 'api'});
    return true;
  });
  assert.equal(forbidden.used, 1);
  await assert.rejects(make(async () => new Response(null)).list(deadline), /response_body_missing/);
  const invalid = make(async () => new Response(null, {status: 302, headers: {location: 'not a url'}}));
  const entry = {name: 'expected', id: 42, expired: false, size_in_bytes: one.length, digest: `sha256:${digest(one)}`};
  await assert.rejects(invalid.download([entry], 'expected', 'identity-ready.json', deadline), TypeError);
  assert.equal(invalid.used, 1);
});
test('API and ZIP size ceilings are enforced', async t => {
  const dir = await temporary(t);
  for (const [suffix, response] of [
    ['declared', new Response('{}', {headers: {'content-length': '2097153'}})],
    ['streamed', new Response(Buffer.alloc(2097153))],
  ]) {
    const transport = new ArtifactTransport({context, token: 'x', budgetFile: path.join(dir, suffix),
      fetcher: async () => response});
    await assert.rejects(transport.list(Date.now() + 1000), /response_oversized/);
  }
  await assert.rejects(singleJson(Buffer.alloc(16 * 1024 * 1024 + 1), 'identity-ready.json'), /oversized/);
});
test('successful exchange copies all three identities and preserves exact directory membership', async t => {
  const dir = await temporary(t);
  await fs.writeFile(path.join(dir, 'linux-amd64-identity-ready.json'), '{}');
  const identities = path.join(dir, 'identities');
  await supervise({options: {mode: 'exchange', role: 'linux-amd64', output: dir,
    identities, jobDeadline: 4300}, context, now: () => 1100_000,
    artifact: {uploadArtifact: async () => ({id: 1})},
    transport: {list: async () => [], download: async () => Buffer.from('{}')},
    delay: () => new Promise(resolve => setImmediate(resolve))});
  assert.deepEqual((await fs.readdir(identities)).sort(),
    ['linux-amd64', 'linux-arm64', 'macos-arm64'].map(role => `${role}-identity-ready.json`));
});
test('cancellation immediately signals driver and aborts in-flight API request', async t => {
  const dir = await temporary(t);
  const controller = new AbortController();
  const child = new EventEmitter();
  const signals = [];
  child.kill = signal => { signals.push(signal); setImmediate(() => child.emit('exit', null, signal)); };
  let aborted = false;
  await assert.rejects(supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context',
    driver: '/driver', workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => 1100_000, artifact: {}, spawnChild: () => child, signal: controller.signal,
    transport: {list: async (_deadline, signal) => new Promise((resolve, reject) => {
      signal.addEventListener('abort', () => { aborted = true; reject(new Error('aborted')); }, {once: true});
      setImmediate(() => { controller.abort(); assert.deepEqual(signals, ['SIGTERM']); });
    })}}), /supervisor_cancelled/);
  assert.equal(aborted, true);
  assert.deepEqual(signals, ['SIGTERM']);
});
test('a long API backoff cannot delay local terminal publication or joined exit', async t => {
  const dir = await temporary(t);
  const child = new EventEmitter();
  child.kill = () => {};
  const published = [];
  const diagnostics = [];
  let listed = 0;
  await supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => 1100_000, spawnChild: () => child, diagnostic: line => diagnostics.push(line),
    artifact: {uploadArtifact: async name => { published.push(name); return {id: published.length}; }},
    transport: {list: async () => {
      listed++;
      for (const kind of ['server-ready', 'client-result', 'server-result']) await fs.writeFile(path.join(dir, `linux-amd64-${kind}.json`), '{}');
      child.emit('exit', 0, null);
      const error = new Error('artifact_retry'); error.retryAfter = 300_000; throw error;
    }}, delay: () => new Promise(resolve => setImmediate(resolve))});
  assert.equal(listed, 1);
  assert.equal(published.length, 3);
  assert.deepEqual(diagnostics, []);
});
test('driver exit during last upload gets a final complete scan', async t => {
  const dir = await temporary(t);
  const child = new EventEmitter(); child.kill = () => {};
  for (const kind of ['client-result', 'server-result']) await fs.writeFile(path.join(dir, `linux-amd64-${kind}.json`), '{}');
  let count = 0;
  await supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => 1100_000, spawnChild: () => child,
    artifact: {uploadArtifact: async () => { if (++count === 2) child.emit('exit', 0, null); return {id: count}; }},
    transport: {}, delay: () => new Promise(resolve => setImmediate(resolve))});
  assert.equal(count, 2);
});
test('expired cleanup reserve forces but never fabricates a successful child join', async t => {
  const dir = await temporary(t);
  const child = new EventEmitter();
  const signals = [];
  child.kill = signal => { signals.push(signal); if (signal === 'SIGKILL') setImmediate(() => child.emit('exit', null, signal)); };
  let time = 1100_000;
  const controller = new AbortController();
  await assert.rejects(supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context',
    driver: '/driver', workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => time, spawnChild: () => child, artifact: {}, signal: controller.signal,
    transport: {list: async () => { time = 4000_001; controller.abort(); return []; }}}), /supervisor_cancelled/);
  assert.deepEqual(signals, ['SIGTERM', 'SIGKILL']);
});
test('a malformed in-flight artifact racing the final scan cannot become success', async t => {
  const dir = await temporary(t);
  const diagnostics = [];
  const child = new EventEmitter(); child.kill = () => {};
  let rejectPoll;
  let count = 0;
  await assert.rejects(supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context',
    driver: '/driver', workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => 1100_000, spawnChild: () => child, diagnostic: line => diagnostics.push(line),
    artifact: {uploadArtifact: async () => {
      if (++count === 2) setTimeout(() => rejectPoll(new Error('artifact_duplicate')), 40);
      return {id: count};
    }}, transport: {list: async () => new Promise((resolve, reject) => {
      rejectPoll = reject;
      (async () => {
        for (const kind of ['client-result', 'server-result']) await fs.writeFile(path.join(dir, `linux-amd64-${kind}.json`), '{}');
        child.emit('exit', 0, null);
      })().catch(reject);
    })}, delay: () => new Promise(resolve => setImmediate(resolve))}), /artifact_duplicate/);
  assert.equal(count, 2);
  assert.equal(diagnostics.length, 1);
  const diagnostic = JSON.parse(diagnostics[0].slice('marmish installed diagnostic: '.length));
  assert.equal(diagnostic.stage, 'remote_list');
  assert.equal(diagnostic.category, 'artifact_duplicate');
  assert.equal(diagnostic.peer_role, null);
});


test('list validates fatal shapes and bounds without retrying or accepting partial inventories', async t => {
  const dir = await temporary(t);
  const cases = [
    ['{', 'artifact_list_json'], ['null', 'artifact_list_shape'], ['[]', 'artifact_list_shape'],
    ['1', 'artifact_list_shape'],
    ...[undefined, '3', 0.5, -1, Number.MAX_SAFE_INTEGER + 1].map(total_count =>
      [JSON.stringify({total_count, artifacts: []}), 'artifact_list_total']),
    [JSON.stringify({total_count: 101, artifacts: []}), 'artifact_list_total'],
    [JSON.stringify({total_count: 0}), 'artifact_list_items'],
    [JSON.stringify({total_count: 0, artifacts: {}}), 'artifact_list_items'],
    [JSON.stringify({total_count: 3, artifacts: Array(101).fill({})}), 'artifact_list_items'],
  ];
  for (const [index, [body, category]] of cases.entries()) {
    let calls = 0;
    const transport = new ArtifactTransport({context, token: 'x', budgetFile: path.join(dir, String(index)),
      fetcher: async () => { calls++; return new Response(body); }});
    await assert.rejects(transport.list(Date.now() + 30_000), {message: category});
    assert.equal(calls, 1);
  }
  for (const size of [0, 100]) {
    const entries = Array(size).fill({});
    const transport = new ArtifactTransport({context, token: 'x', budgetFile: path.join(dir, `complete-${size}`),
      fetcher: async () => new Response(JSON.stringify({total_count: size, artifacts: entries}))});
    assert.deepEqual(await transport.list(Date.now() + 30_000), entries);
  }
});

// Exercise actual listing, authenticated downloads and immutable inbox writes;
// only GitHub/blob HTTP and the passage of time are deterministic fixtures.
async function listExchange(t, snapshots, used = 0) {
  const directory = await temporary(t);
  const created = Math.floor(Date.now() / 1000);
  const ctx = {...context, created_at: created, expires_at: created + 2220};
  const state = {now: (created + 100) * 1000, lists: 0, downloads: 0, pending: false,
    diagnostics: [], deadlines: [], signals: [], polls: [], onPause: async () => {}};
  const identities = path.join(directory, 'identities');
  const budgetFile = path.join(directory, 'budget');
  await fs.writeFile(budgetFile, JSON.stringify({used}));
  await fs.writeFile(path.join(directory, 'linux-amd64-identity-ready.json'), '{}');
  const entries = ['linux-amd64', 'linux-arm64', 'macos-arm64'].map((role, index) => ({
    name: artifactName(binding(ctx, role), role, 'identity-ready'), id: index + 1,
    expired: false, size_in_bytes: one.length, digest: `sha256:${digest(one)}`,
  }));
  const fetcher = async url => {
    if (String(url).endsWith('artifacts?per_page=100')) {
      const snapshot = snapshots[Math.min(state.lists++, snapshots.length - 1)];
      return new Response(JSON.stringify(snapshot(entries)));
    }
    if (String(url).endsWith('/zip')) {
      state.downloads++;
      return new Response(null, {status: 302, headers: {location: 'https://x.blob.core.windows.net/fixture'}});
    }
    return new Response(one);
  };
  const transport = new ArtifactTransport({context: ctx, token: 'x', budgetFile, fetcher});
  const list = transport.list.bind(transport);
  transport.list = async (deadline, signal) => {
    state.polls.push(state.now);
    state.deadlines.push(deadline); state.signals.push(signal); state.pending = true;
    try { return await list(deadline, signal); } finally { state.pending = false; }
  };
  return {state, entries, directory, identities, budgetFile, transport,
    run: extra => supervise({options: {mode: 'exchange', role: 'linux-amd64', output: directory,
      identities, jobDeadline: created + 4300}, context: ctx, now: () => state.now,
      artifact: {uploadArtifact: async () => ({id: 1})}, transport,
      diagnostic: line => state.diagnostics.push(JSON.parse(line.slice('marmish installed diagnostic: '.length))),
      delay: async () => {
        await new Promise(resolve => setImmediate(resolve));
        if (!state.pending) state.now += 1000;
        await state.onPause();
      }, ...extra})};
}
const completeList = entries => ({total_count: entries.length, artifacts: entries});

test('bounded shorter and longer list snapshots admit nothing until a complete exchange', async t => {
  for (const total of [3, 1]) {
    const fixture = await listExchange(t, [entries => ({total_count: total, artifacts: entries.slice(0, 2)}), completeList]);
    const {state} = fixture;
    let observedEmpty = false;
    state.onPause = async () => {
      if (state.diagnostics.length && state.lists === 1) {
        assert.equal(state.downloads, 0);
        await assert.rejects(fs.access(fixture.identities), {code: 'ENOENT'});
        observedEmpty = true;
      }
    };
    await fixture.run();
    assert.equal(observedEmpty, true);
    assert.equal(state.lists, 2);
    assert.ok(state.polls[1] - state.polls[0] >= 30_000);
    assert.equal(state.downloads, 3);
    assert.equal(new Set(state.deadlines).size, 1);
    assert.equal(new Set(state.signals).size, 1);
    assert.equal(JSON.parse(await fs.readFile(fixture.budgetFile)).used, 5);
    assert.equal(state.diagnostics.length, 1);
    assert.equal(state.diagnostics[0].reason, 'count_mismatch');
    assert.deepEqual((await fs.readdir(fixture.identities)).sort(),
      ['linux-amd64', 'linux-arm64', 'macos-arm64'].map(role => `${role}-identity-ready.json`));
    for (const name of await fs.readdir(fixture.identities)) {
      assert.equal(await fs.readFile(path.join(fixture.identities, name), 'utf8'), '{"schema":1}');
    }
  }
});

test('persistent list mismatch stops at the original deadline without identity admission', async t => {
  const fixture = await listExchange(t, [entries => ({total_count: 3, artifacts: entries.slice(0, 2)})]);
  await assert.rejects(fixture.run(), {message: 'artifact_deadline'});
  assert.equal(fixture.state.downloads, 0);
  assert.equal(new Set(fixture.state.deadlines).size, 1);
  assert.ok(fixture.state.now >= fixture.state.deadlines[0]);
  assert.ok(fixture.state.lists > 1 && fixture.state.lists < 120);
  await assert.rejects(fs.access(fixture.identities), {code: 'ENOENT'});
  assert.equal(fixture.state.diagnostics.at(-1).category, 'artifact_deadline');
  assert.equal(fixture.state.diagnostics.filter(event => event.event === 'failed').length, 1);
});

test('mismatch consumes the shared budget and another transport cannot reset it', async t => {
  const fixture = await listExchange(t, [entries => ({total_count: 3, artifacts: entries.slice(0, 2)})], 119);
  await assert.rejects(fixture.run(), {message: 'artifact_budget'});
  assert.equal(fixture.state.lists, 1);
  assert.equal(fixture.state.downloads, 0);
  const second = new ArtifactTransport({context, token: 'x', budgetFile: fixture.budgetFile,
    fetcher: async () => { assert.fail('exhausted budget must not fetch'); }});
  await assert.rejects(second.list(Date.now() + 30_000), {message: 'artifact_budget'});
  assert.equal(JSON.parse(await fs.readFile(fixture.budgetFile)).used, 120);
  await assert.rejects(fs.access(fixture.identities), {code: 'ENOENT'});
});

test('abort during mismatch wait makes no further requests or identity admission', async t => {
  const fixture = await listExchange(t, [entries => ({total_count: 3, artifacts: entries.slice(0, 2)})]);
  const controller = new AbortController();
  fixture.state.onPause = async () => {
    if (fixture.state.diagnostics.length) controller.abort();
  };
  await assert.rejects(fixture.run({signal: controller.signal}), {message: 'supervisor_cancelled'});
  assert.equal(fixture.state.lists, 1);
  assert.equal(fixture.state.downloads, 0);
  await assert.rejects(fs.access(fixture.identities), {code: 'ENOENT'});
});

test('a list mismatch followed by a duplicate stays fatal at remote_download', async t => {
  const fixture = await listExchange(t, [entries => ({total_count: 3, artifacts: entries.slice(0, 2)}),
    entries => ({total_count: 4, artifacts: [...entries, entries[0]]})]);
  await assert.rejects(fixture.run(), {message: 'artifact_duplicate'});
  assert.equal(fixture.state.lists, 2);
  assert.equal(fixture.state.downloads, 0);
  assert.equal(fixture.state.diagnostics[0].event, 'artifact_list_observation');
  const failure = fixture.state.diagnostics.at(-1);
  assert.equal(failure.category, 'artifact_duplicate');
  assert.equal(failure.stage, 'remote_download');
  assert.equal(failure.peer_role, 'linux-amd64');
  await assert.rejects(fs.access(fixture.identities), {code: 'ENOENT'});
});

test('a local fatal after mismatch observation keeps its own diagnostic stage', async t => {
  const fixture = await listExchange(t, [entries => ({total_count: 3, artifacts: entries.slice(0, 2)})]);
  fixture.state.onPause = async () => {
    if (fixture.state.diagnostics.length) {
      await fs.writeFile(path.join(fixture.directory, 'linux-amd64-identity-ready.json'), '[]');
    }
  };
  await assert.rejects(fixture.run(), {message: 'published_record_changed'});
  assert.equal(fixture.state.diagnostics[0].stage, 'remote_list');
  const failure = fixture.state.diagnostics.at(-1);
  assert.equal(failure.event, 'failed');
  assert.equal(failure.stage, 'local_record_read');
  assert.equal(failure.category, 'published_record_changed');
});


test('an aborted poll suppresses the mismatch observation', async t => {
  const fixture = await listExchange(t, [entries => ({total_count: 3, artifacts: entries.slice(0, 2)})]);
  const controller = new AbortController();
  const list = fixture.transport.list.bind(fixture.transport);
  fixture.transport.list = async (...args) => {
    try { return await list(...args); }
    catch (error) { controller.abort(); throw error; }
  };
  await assert.rejects(fixture.run({signal: controller.signal}), {message: 'supervisor_cancelled'});
  assert.equal(fixture.state.lists, 1);
  assert.equal(fixture.state.downloads, 0);
  assert.equal(fixture.state.diagnostics.filter(event => event.event === 'artifact_list_observation').length, 0);
});


test('terminal HTTP errors retain only exact status and trusted request class without retry', async t => {
  const dir = await temporary(t);
  for (const api of [true, false]) for (const status of [204, 304, 401, 403, 404, 410, 422, 599]) {
    let calls = 0;
    const transport = new ArtifactTransport({context, token: 'private-token',
      budgetFile: path.join(dir, `budget-${api}-${status}`), fetcher: async () => {
        calls++; return new Response(null, {status, statusText: 'PRIVATE response text'});
      }});
    await assert.rejects(transport.request('https://api.github.com/private?secret', api, 1024,
      Date.now() + 30_000), error => {
      assert.equal(error.message, 'artifact_http');
      assert.deepEqual(error.artifactHttp, {status, target: api ? 'api' : 'blob'});
      assert.deepEqual(Object.keys(error).sort(), ['artifactHttp']);
      return true;
    });
    assert.equal(calls, 1);
    assert.equal(transport.used, api ? 1 : null);
  }
});
