import test from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import {publishRelease, githubAdapters, verifyArtifactZip, transferDeadlineMs} from './publish.mjs';
import {canonicalInventory, inventoryDigest, releaseNames, releaseReportNames} from './release-schema.mjs';
function zipFiles(files) {
  const parts = [], central = []; let offset = 0;
  for (const [name, content] of files) {
    let crc = 0xffffffff;
    for (const byte of content) { crc ^= byte; for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0); }
    crc = (crc ^ 0xffffffff) >>> 0;
    const label = Buffer.from(name), local = Buffer.alloc(30), header = Buffer.alloc(46);
    local.writeUInt32LE(0x04034b50); local.writeUInt32LE(crc, 14); local.writeUInt32LE(content.length, 18);
    local.writeUInt32LE(content.length, 22); local.writeUInt16LE(label.length, 26);
    header.writeUInt32LE(0x02014b50); header.writeUInt32LE(crc, 16); header.writeUInt32LE(content.length, 20);
    header.writeUInt32LE(content.length, 24); header.writeUInt16LE(label.length, 28); header.writeUInt32LE(offset, 42);
    parts.push(local, label, content); central.push(header, label); offset += local.length + label.length + content.length;
  }
  const directory = Buffer.concat(central), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50); end.writeUInt16LE(files.length, 8); end.writeUInt16LE(files.length, 10);
  end.writeUInt32LE(directory.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, directory, end]);
}
function fixture(version = '0.2.3') {
  const source = 'a'.repeat(40), bootstrapCommit = 'b'.repeat(40), tagObject = 'c'.repeat(40);
  const bootstrap = Buffer.from('#!/bin/sh\necho pinned\n');
  const archives = new Map([[10, zipFiles(releaseNames(version).map(name => [name, Buffer.from(name)]))],
    [11, zipFiles([['install.sh', bootstrap]])]]);
  const inventory = {schema: 1, repository: 'Epiphytic/marmish', publisher_sha: source, source, version,
    candidate: {run_id: '101', run_attempt: '1'}, artifacts: [...archives].map(([id, bytes]) => ({id, name: id === 10 ? 'candidate-101-1' : 'v02-pinned-bootstrap-101-1', digest: `sha256:${inventoryDigest(bytes)}`})),
    assets: releaseNames(version).map(name => ({name, size: Buffer.byteLength(name), sha256: inventoryDigest(Buffer.from(name))})),
    bootstrap_sha256: inventoryDigest(bootstrap), reports: releaseReportNames(version, {run_id:'101',run_attempt:'1'}).map(name => ({name,sha256:'e'.repeat(64)}))};
  const inventoryBytes = canonicalInventory(inventory), hash = inventoryDigest(inventoryBytes);
  const proof = `Marmish-Publication: ${JSON.stringify({schema: 1, source, version, run_id: '101', run_attempt: '1', inventory_sha256: hash})}`;
  const state = {master: source, tag: null, release: null, assets: [], drafts: [], tags: [], writes: [], uploads: [], snapshots: [], hook: () => {}};
  const notFound = () => { const e = new Error('not_found'); e.status = 404; throw e; };
  const asset = pin => ({id: inventory.assets.indexOf(pin) + 1, name: pin.name, state: 'uploaded', size: pin.size,
    digest: `sha256:${pin.sha256}`, browser_download_url: `https://github.com/Epiphytic/marmish/releases/download/v${version}/${pin.name}`});
  const api = async (route, options = {}) => {
    const method = options.method ?? 'GET';
    const r = route.replace('/repos/Epiphytic/marmish', '').split('?')[0];
    state.hook(r, method);
    if (method !== 'GET') state.writes.push({route: r, method, body: options.body});
    if (r === '/actions/runs/101/attempts/1' || r === '/actions/runs/101') return {id: 101, run_attempt: 1, repository: {full_name: inventory.repository},
      head_repository: {full_name: inventory.repository}, path: '.github/workflows/v02-release.yml', head_branch: 'master',
      event: 'push', head_sha: source, status: 'completed', conclusion: 'success'};
    if (r.startsWith('/actions/artifacts/')) { const pin = inventory.artifacts.find(a => String(a.id) === r.split('/').at(-1));
      return {...pin, expired: false, workflow_run: {id: 101, head_sha: source}}; }
    if (r === '/branches/master') return {protected: false};
    if (r === '/rulesets') return [];
    if (r === '/tags') return state.tags;
    if (r === '/git/ref/heads/master') return {object: {type: 'commit', sha: state.master}};
    if (r === `/git/ref/tags/v${version}`) return state.tag ? {object: {type: 'tag', sha: tagObject}} : notFound();
    if (r === `/git/tags/${tagObject}`) return {object: {type: 'commit', sha: state.tag}, tag: `v${version}`, message: `release\n${proof}`};
    if (r === '/git/tags' && method === 'POST') return {sha: tagObject};
    if (r === '/git/refs' && method === 'POST') { state.tag = source; return {}; }
    if (r === '/releases' && method === 'GET') return [...state.drafts, ...(state.release ? [state.release] : [])];
    if (r === '/releases' && method === 'POST') { state.release = {id: 20, ...options.body}; return state.release; }
    if (r === '/releases/20/assets') return state.assets.map(a => ({...a, browser_download_url: state.release.draft
      ? `https://github.com/Epiphytic/marmish/releases/download/untagged-123/${a.name}` : a.browser_download_url}));
    if (r === '/releases/20' && method === 'PATCH') { state.release = {...state.release, ...options.body}; return state.release; }
    if (r === `/git/commits/${source}`) return {tree: {sha: 'f'.repeat(40)}};
    if (r === '/git/blobs' && method === 'POST') return {sha: 'e'.repeat(40)};
    if (r === '/git/trees' && method === 'POST') return {sha: 'd'.repeat(40)};
    if (r === '/git/commits' && method === 'POST') return {sha: bootstrapCommit};
    if (r === '/git/refs/heads/master' && method === 'PATCH') {
      assert.deepEqual(options.body, {sha: bootstrapCommit, force: false}); state.master = bootstrapCommit; return {};
    }
    if (r === `/git/commits/${bootstrapCommit}`) return {parents: [{sha: source}], tree: {sha: 'd'.repeat(40)}};
    if (r === `/compare/${source}...${bootstrapCommit}`) return {status: 'ahead', ahead_by: 1, total_commits: 1,
      files: [{filename: 'scripts/install.sh', status: 'modified'}]};
    if (r === `/git/trees/${'d'.repeat(40)}`) return {truncated: false, tree: [{path: 'scripts/install.sh', mode: '100755', type: 'blob', sha: 'e'.repeat(40)}]};
    if (r === `/git/blobs/${'e'.repeat(40)}`) return {encoding: 'base64', content: bootstrap.toString('base64')};
    throw new Error(`unexpected_fixture_route:${method}:${r}`);
  };
  const adapters = {api, downloadArtifact: async id => archives.get(id), readFile: async file => path.basename(file) === 'bootstrap.sh' ? bootstrap : Buffer.from(path.basename(file)),
    uploadAsset: async (id, name) => { assert.equal(id, 20); state.uploads.push(name); state.assets.push(asset(inventory.assets.find(a => a.name === name))); },
    downloadPublic: async url => url.includes('raw.githubusercontent.com') ? bootstrap : Buffer.from(decodeURIComponent(url.split('/').at(-1))),
    writeState: async value => state.snapshots.push(value)};
  const input = {inventoryBytes, inventorySha256: hash, payloadRoot: '/payload', bootstrapPath: '/bootstrap.sh', publisherSha: source};
  const seed = () => { state.tag = source; state.release = {id: 20, tag_name: `v${version}`, prerelease: true, draft: true, body: proof}; };
  return {inventory, input, adapters, state, seed, asset, source, bootstrapCommit};
}
test('publishes exact fifteen immutable assets and script-only direct-child bootstrap', async () => {
  const f = fixture(); const result = await publishRelease(f.input, f.adapters);
  assert.equal(result.publication_state, 'complete'); assert.equal(result.bootstrap_commit, f.bootstrapCommit);
  assert.equal(f.state.uploads.length, 15); assert.equal(f.state.release.draft, false);
  assert.equal(f.state.writes.filter(w => w.method === 'DELETE').length, 0);
  assert.equal(f.state.writes.find(w => w.route === '/git/trees').body.tree[0].mode, '100755');
  assert.deepEqual(f.state.writes.find(w => w.route === '/git/commits').body.parents, [f.source]);
  assert.equal(f.state.writes.find(w => w.route === '/releases').body.make_latest, 'false');
});
test('hidden higher or equal unrelated draft is rejected by write authority before any write', async () => {
  for (const version of ['v0.2.4', 'v0.2.3']) {
    const f = fixture(); f.state.drafts.push({id: 99, tag_name: version, draft: true, body: 'unrelated'});
    await assert.rejects(publishRelease(f.input, f.adapters), /draft_version_conflict/);
    assert.deepEqual(f.state.writes, []);
  }
});
test('old experimental and unrelated tags/releases do not block native release', async () => {
  const f = fixture(); f.state.tags = [{name: 'v0.1.0-experimental.1'}, {name: 'v0.1.0-experimental.2'}, {name: 'other'}];
  f.state.drafts = [{tag_name: 'v0.1.0-experimental.2', draft: true}];
  assert.equal((await publishRelease(f.input, f.adapters)).publication_state, 'complete');
});
test('matching draft resumes missing assets only; completed retry performs no writes', async () => {
  const f = fixture(); f.seed(); f.state.assets.push(f.asset(f.inventory.assets[0]));
  await publishRelease(f.input, f.adapters);
  assert.equal(f.state.uploads.length, 14);
  f.state.writes = []; f.state.uploads = [];
  f.input.publisherSha = f.bootstrapCommit;
  await publishRelease(f.input, f.adapters);
  assert.deepEqual(f.state.writes, []); assert.deepEqual(f.state.uploads, []);
});
test('partial, duplicate and mismatched draft assets are not deleted or replaced', async () => {
  for (const mutate of [a => { a.state = 'new'; }, a => { a.digest = null; }, a => { a.size++; }]) {
    const f = fixture(); f.seed(); const a = f.asset(f.inventory.assets[0]); mutate(a); f.state.assets.push(a);
    await assert.rejects(publishRelease(f.input, f.adapters), /release_asset_conflict/);
    assert.deepEqual(f.state.writes, []); assert.deepEqual(f.state.uploads, []);
  }
  const f = fixture(); f.seed(); const a = f.asset(f.inventory.assets[0]); f.state.assets.push(a, a);
  await assert.rejects(publishRelease(f.input, f.adapters), /release_asset_conflict/);
});
test('inventory, artifact and payload changes fail without any write', async () => {
  const a = fixture(); a.input.inventorySha256 = '0'.repeat(64);
  await assert.rejects(publishRelease(a.input, a.adapters), /inventory_digest/);
  const b = fixture(); const api = b.adapters.api;
  b.adapters.api = async (...args) => { const result = await api(...args); if (args[0].endsWith('/artifacts/10')) return {...result, digest: 'sha256:' + '0'.repeat(64)}; return result; };
  await assert.rejects(publishRelease(b.input, b.adapters), /artifact_changed/);
  const c = fixture(); c.adapters.readFile = async () => Buffer.from('changed');
  await assert.rejects(publishRelease(c.input, c.adapters), /payload_digest/);
  for (const f of [a,b,c]) assert.deepEqual(f.state.writes, []);
});
test('master race before tag and before release publication stops mutation', async () => {
  const f = fixture(); let reads = 0;
  f.state.hook = (r, method) => { if (r === '/git/ref/heads/master' && method === 'GET' && ++reads === 2) f.state.master = '9'.repeat(40); };
  await assert.rejects(publishRelease(f.input, f.adapters), /master_raced/); assert.deepEqual(f.state.writes, []);
  const g = fixture(); g.state.hook = (r, method) => { if (r === '/releases/20/assets' && method === 'GET' && g.state.assets.length === 15) g.state.master = '9'.repeat(40); };
  await assert.rejects(publishRelease(g.input, g.adapters), /master_raced/);
  assert.equal(g.state.release.draft, true); assert.equal(g.state.writes.some(w => w.route === '/releases/20'), false);
});
test('protection or newer native tag prevents release creation', async () => {
  const f = fixture(), api = f.adapters.api;
  f.adapters.api = async (...args) => args[0].endsWith('/branches/master') ? {protected: true} : api(...args);
  await assert.rejects(publishRelease(f.input, f.adapters), /master_protected/); assert.deepEqual(f.state.writes, []);
  const g = fixture(); g.state.tags = [{name: 'v0.2.10-rc.1'}];
  await assert.rejects(publishRelease(g.input, g.adapters), /newer_tag/); assert.deepEqual(g.state.writes, []);
});
test('REST adapter bounds GET retries, never retries uncertain writes, and omits auth publicly', async () => {
  let count = 0; const calls = [];
  const adapters = githubAdapters('private-token', async (url, options) => {
    calls.push({url, options}); count++;
    return new Response(count < 3 ? 'retry' : '{}', {status: count < 3 ? 503 : 200});
  });
  await adapters.api('/repos/Epiphytic/marmish/releases'); assert.equal(count, 3);
  assert.equal(calls[0].options.headers.Authorization, 'Bearer private-token');
  count = 0;
  await assert.rejects(adapters.api('/repos/Epiphytic/marmish/releases', {method: 'POST', body: {}}), /http/);
  assert.equal(count, 1);
  await assert.rejects(adapters.api('/repos/attacker/repo/releases'), /route/);
  const pub = githubAdapters('private-token', async (_url, options) => {
    assert.equal(options.headers.Authorization, undefined); return new Response('safe');
  });
  assert.equal((await pub.downloadPublic('https://github.com/Epiphytic/marmish/releases/download/v0.2.3/a', 4)).toString(), 'safe');
  await assert.rejects(pub.downloadPublic('https://attacker.example/', 4), /download/);
});

test('ZIP verifier rejects traversal, duplicate names, checksum mutation and truncated archives', () => {
  assert.equal(verifyArtifactZip(zipFiles([['safe', Buffer.from('bytes')]])).get('safe').toString(), 'bytes');
  for (const archive of [zipFiles([['../secret', Buffer.from('x')]]), zipFiles([['same', Buffer.from('a')], ['same', Buffer.from('b')]])])
    assert.throws(() => verifyArtifactZip(archive));
  const mutated = zipFiles([['safe', Buffer.from('bytes')]]); mutated[34] ^= 1;
  assert.throws(() => verifyArtifactZip(mutated), /crc/);
  assert.throws(() => verifyArtifactZip(Buffer.alloc(21)));
});
test('original archive digest mismatch blocks all writes', async () => {
  const f = fixture(); f.adapters.downloadArtifact = async () => zipFiles([['changed', Buffer.from('x')]]);
  await assert.rejects(publishRelease(f.input, f.adapters), /artifact_archive_digest/);
  assert.deepEqual(f.state.writes, []);
});

test('wrong tag source or proof is immutable conflict rather than overwrite', async () => {
  const f = fixture(); f.seed(); f.state.tag = '9'.repeat(40);
  await assert.rejects(publishRelease(f.input, f.adapters), /tag_source_conflict/);
  assert.deepEqual(f.state.writes, []);
  const g = fixture(); g.seed(); const api = g.adapters.api;
  g.adapters.api = async (...args) => {
    const result = await api(...args);
    if (args[0].includes('/git/tags/')) return {...result, message: 'other candidate'};
    return result;
  };
  await assert.rejects(publishRelease(g.input, g.adapters), /tag_provenance_conflict/);
  assert.deepEqual(g.state.writes, []);
});
test('anonymous download must reproduce published asset bytes before bootstrap', async () => {
  const f = fixture(); f.adapters.downloadPublic = async () => Buffer.from('changed');
  await assert.rejects(publishRelease(f.input, f.adapters), /public_digest_mismatch/);
  assert.equal(f.state.release.draft, false);
  assert.equal(f.state.writes.some(w => w.route === '/git/refs/heads/master'), false);
});
test('artifact download redirect strips credentials and rejects untrusted hosts', async () => {
  const calls = [];
  const adapter = githubAdapters('private-token', async (url, options) => {
    calls.push({url, options});
    return calls.length === 1 ? new Response(null, {status: 302, headers: {location: 'https://valid.blob.core.windows.net/artifact'}}) : new Response('zip');
  });
  assert.equal((await adapter.downloadArtifact(1, 10)).toString(), 'zip');
  assert.equal(calls[0].options.headers.Authorization, 'Bearer private-token');
  assert.equal(calls[1].options.headers.Authorization, undefined);
  const bad = githubAdapters('private-token', async () => new Response(null, {status: 302, headers: {location: 'https://attacker.example/'}}));
  await assert.rejects(bad.downloadArtifact(1, 10), /artifact_redirect/);
});

test('latest candidate attempt change refuses before any write', async () => {
  const f = fixture(), api = f.adapters.api;
  f.adapters.api = async (route, options) => {
    const value = await api(route, options);
    return route.endsWith('/actions/runs/101') ? {...value, run_attempt: 2} : value;
  };
  await assert.rejects(publishRelease(f.input, f.adapters), /candidate_attempt_changed/);
  assert.deepEqual(f.state.writes, []);
});
test('only uploaded null digests get at most three final rereads', async () => {
  const f = fixture(), api = f.adapters.api; let reads = 0, waits = 0;
  f.adapters.wait = async () => { waits++; };
  f.adapters.api = async (route, options) => {
    const value = await api(route, options);
    if (route.includes('/releases/20/assets') && f.state.release.draft && f.state.assets.length === 15 && ++reads <= 2)
      return value.map(a => ({...a, digest: null}));
    return value;
  };
  await publishRelease(f.input, f.adapters); assert.equal(reads, 3); assert.equal(waits, 2);
  const g = fixture(), secondApi = g.adapters.api; let secondWaits = 0;
  g.adapters.wait = async () => { secondWaits++; };
  g.adapters.api = async (route, options) => {
    const value = await secondApi(route, options);
    return route.includes('/releases/20/assets') && g.state.assets.length === 15 ? value.map(a => ({...a, digest: null})) : value;
  };
  await assert.rejects(publishRelease(g.input, g.adapters), /release_digest_unavailable/);
  assert.equal(secondWaits, 2); assert.equal(g.state.release.draft, true);
  assert.deepEqual(g.state.snapshots.at(-1).error_category, 'release_digest_unavailable');
  assert.equal(g.state.snapshots.at(-1).failed_phase, 'upload');
});
test('post-write failure records only finite phase and category', async () => {
  const f = fixture(); f.seed(); f.adapters.uploadAsset = async () => { throw new Error('secret server supplied arbitrary response'); };
  await assert.rejects(publishRelease(f.input, f.adapters), /arbitrary/);
  const last = f.state.snapshots.at(-1);
  assert.equal(last.publication_state, 'failed'); assert.equal(last.failed_phase, 'upload');
  assert.equal(last.error_category, 'publication_failed');
  assert.equal(JSON.stringify(last).includes('secret'), false);
});

test('completed bootstrap retry accepts only its exact current publisher SHA', async () => {
  const f = fixture(); await publishRelease(f.input, f.adapters);
  f.state.writes = []; f.input.publisherSha = '9'.repeat(40);
  await assert.rejects(publishRelease(f.input, f.adapters), /publisher_not_source/);
  assert.deepEqual(f.state.writes, []);
  f.input.publisherSha = f.bootstrapCommit;
  assert.equal((await publishRelease(f.input, f.adapters)).publication_state, 'complete');
  assert.deepEqual(f.state.writes, []);
});

test('post-publish asset inventory is refreshed from draft untagged URLs', async () => {
  const f = fixture(), api = f.adapters.api; let sawDraft = false, sawPublic = false;
  f.adapters.api = async (route, options) => {
    const value = await api(route, options);
    if (route.includes('/releases/20/assets') && value.length) {
      if (f.state.release.draft) { sawDraft = true; assert.match(value[0].browser_download_url, /untagged-/); }
      else { sawPublic = true; assert.match(value[0].browser_download_url, /download\/v0\.2\.3\//); }
    }
    return value;
  };
  assert.equal((await publishRelease(f.input, f.adapters)).publication_state, 'complete');
  assert.equal(sawDraft, true); assert.equal(sawPublic, true);
});
test('post-publish refreshed inventory is revalidated before bootstrap cutover', async () => {
  const f = fixture(), api = f.adapters.api;
  f.adapters.api = async (route, options) => {
    const value = await api(route, options);
    return route.includes('/releases/20/assets') && !f.state.release.draft
      ? value.map(a => ({...a, digest: 'sha256:' + '0'.repeat(64)})) : value;
  };
  await assert.rejects(publishRelease(f.input, f.adapters), /release_asset_conflict/);
  assert.equal(f.state.master, f.source);
});
test('raw master CDN mismatch retries with bounded backoff and current-ref checks', async () => {
  const f = fixture(), original = f.adapters.downloadPublic; let reads = 0; const waits = [];
  f.adapters.wait = async milliseconds => waits.push(milliseconds);
  f.adapters.downloadPublic = async (url, limit) => {
    if (url.includes('/master/scripts/') && ++reads < 3) return Buffer.from('cached old script');
    return original(url, limit);
  };
  assert.equal((await publishRelease(f.input, f.adapters)).publication_state, 'complete');
  assert.equal(reads, 3); assert.deepEqual(waits, [5000, 15000]);
  const g = fixture(), other = g.adapters.downloadPublic; let attempts = 0; const exhaustedWaits = [];
  g.adapters.wait = async milliseconds => exhaustedWaits.push(milliseconds);
  g.adapters.downloadPublic = async (url, limit) => url.includes('/master/scripts/')
    ? (++attempts, Buffer.from('cached')) : other(url, limit);
  await assert.rejects(publishRelease(g.input, g.adapters), /public_master_bootstrap_mismatch/);
  assert.equal(attempts, 9); assert.equal(exhaustedWaits.length, 8);
  assert.equal(exhaustedWaits.reduce((a,b) => a+b), 350000);
  assert.equal(g.state.snapshots.at(-1).failed_phase, 'bootstrap');
});
test('large binary uploads and downloads get ten bounded minutes; API JSON stays one minute', () => {
  assert.equal(transferDeadlineMs({raw: true}), 600000);
  assert.equal(transferDeadlineMs({limit: 1024 * 1024 * 1024}), 600000);
  assert.equal(transferDeadlineMs({limit: 1024 * 1024}), 60000);
});

test('0.3 publication preserves guards and static qualification disclosures',async()=>{
  for(const version of ['0.3.0','0.3.1']) {
    const f=fixture(version);await publishRelease(f.input,f.adapters);
    assert.equal(f.state.release.tag_name,`v${version}`);
    const uploads=f.state.uploads.length;await publishRelease({...f.input,publisherSha:f.bootstrapCommit},f.adapters);assert.equal(f.state.uploads.length,uploads);
    assert.equal(f.state.release.prerelease,true);assert.equal(f.state.release.make_latest,'false');
    for(const required of ['stdin/stdout/stderr','rsync','transfer-ID resume','zlib','cancellation',
      'Disconnect does not cancel','exactly matching peer versions','directory confinement','glibc 2.39','macOS 15',
      'ad-hoc signed, not notarized','Keychain','Tor','copy-edge journal assertion failed','two interruption cohorts were not dispatched',
      '3e93fa4 carrying 0.2.3','not these release binaries','marmish install --select 0.2.3','old binaries reject 0.3'])
      assert.ok(f.state.release.body.includes(required),required);
  }
});
test('publisher authority rejects cross-minor newer or equal drafts and malformed native tags before writes',async()=>{
  for(const [version,tag] of [['0.2.3','v0.3.0'],['0.3.0','v0.3.0'],['0.3.0','v0.3.1'],['0.3.0','v0.3.01'],['0.3.0','v0.3.bad']]) {
    const f=fixture(version);f.state.drafts.push({id:99,tag_name:tag,draft:true,body:'unrelated'});
    await assert.rejects(publishRelease(f.input,f.adapters));assert.equal(f.state.writes.length,0);
  }
  for(const tag of ['v0.3.01','v0.3.1+build','v0.2.bogus']) {
    const malformed=fixture('0.3.0');malformed.state.tags.push({name:tag});
    await assert.rejects(publishRelease(malformed.input,malformed.adapters),/invalid_native_tag/);assert.equal(malformed.state.writes.length,0);
  }
  const f=fixture('0.3.0');f.state.tags.push({name:'v0.2.18446744073709551615'});
  await publishRelease(f.input,f.adapters);assert.equal(f.state.release.tag_name,'v0.3.0');
});
