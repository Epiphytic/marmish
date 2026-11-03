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
  await assert.rejects(first.list(Date.now() + 1000), /list_bound/);
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
  await assert.rejects(make(async () => new Response(null, {status: 403})).list(deadline), /artifact_http/);
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
  let listed = 0;
  await supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context', driver: '/driver',
    workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => 1100_000, spawnChild: () => child,
    artifact: {uploadArtifact: async name => { published.push(name); return {id: published.length}; }},
    transport: {list: async () => {
      listed++;
      for (const kind of ['server-ready', 'client-result', 'server-result']) await fs.writeFile(path.join(dir, `linux-amd64-${kind}.json`), '{}');
      child.emit('exit', 0, null);
      const error = new Error('artifact_retry'); error.retryAfter = 300_000; throw error;
    }}, delay: () => new Promise(resolve => setImmediate(resolve))});
  assert.equal(listed, 1);
  assert.equal(published.length, 3);
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
  const child = new EventEmitter(); child.kill = () => {};
  let rejectPoll;
  let count = 0;
  await assert.rejects(supervise({options: {mode: 'run', role: 'linux-amd64', context: '/context',
    driver: '/driver', workspace: dir, output: dir, inbox: dir, identities: dir, jobDeadline: 4300},
    context, now: () => 1100_000, spawnChild: () => child,
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
});
