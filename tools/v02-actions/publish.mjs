// Privileged release coordinator. Node built-ins only; never executes a payload.
import {inflateRawSync} from 'node:zlib';
import {constants, promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {REPOSITORY, HEX40, HEX64, NATIVE_TAG, validVersion, compareVersions, MAX_ASSET, inventoryDigest, parseInventory,
  parsePublicationInputs, requireRelease as need} from './release-schema.mjs';

const nativeTag = NATIVE_TAG;
function compareTags(a, b) {
  need(typeof a === 'string' && typeof b === 'string' && nativeTag.test(a) && nativeTag.test(b)
    && validVersion(a.slice(1)) && validVersion(b.slice(1)), 'invalid_native_tag');
  return compareVersions(a.slice(1), b.slice(1));
}
export async function readPublicFile(file, limit = MAX_ASSET) {
  const handle = await fs.open(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const stat = await handle.stat();
    need(stat.isFile() && stat.size > 0 && stat.size <= limit, 'invalid_payload_file');
    const bytes = Buffer.alloc(stat.size + 1); let count = 0;
    while (count < bytes.length) {
      const next = await handle.read(bytes, count, bytes.length - count, null);
      if (!next.bytesRead) break;
      count += next.bytesRead;
    }
    need(count === stat.size, 'payload_changed');
    return bytes.subarray(0, count);
  } finally { await handle.close(); }
}
async function pages(api, route) {
  const result = [];
  for (let page = 1; page <= 5; page++) {
    const list = await api(`${route}${route.includes('?') ? '&' : '?'}per_page=100&page=${page}`);
    need(Array.isArray(list) && list.length <= 100, 'invalid_api_listing');
    result.push(...list);
    if (list.length < 100) return result;
  }
  throw new Error('incomplete_api_listing');
}
async function optional(api, route) {
  try { return await api(route); } catch (error) { if (error.status === 404) return null; throw error; }
}
function marker(inventory, hash) {
  return `Marmish-Publication: ${JSON.stringify({schema: 1, source: inventory.source, version: inventory.version,
    run_id: inventory.candidate.run_id, run_attempt: inventory.candidate.run_attempt, inventory_sha256: hash})}`;
}
function matchingRelease(release, tag, proof) {
  return release.tag_name === tag && typeof release.body === 'string'
    && release.body.split('\n').filter(line => line.startsWith('Marmish-Publication: ')).length === 1
    && release.body.split('\n').includes(proof) && release.prerelease === true;
}
async function tagTarget(api, base, tag, proof) {
  const ref = await optional(api, `${base}/git/ref/tags/${encodeURIComponent(tag)}`);
  if (!ref) return null;
  need(ref.object?.type === 'tag' && HEX40.test(ref.object.sha), 'invalid_tag_object');
  const object = await api(`${base}/git/tags/${ref.object.sha}`);
  need(object.object?.type === 'commit' && HEX40.test(object.object.sha)
    && object.tag === tag && typeof object.message === 'string'
    && object.message.split('\n').includes(proof), 'tag_provenance_conflict');
  return object.object.sha;
}
async function unchangedMaster(api, base, source) {
  const ref = await api(`${base}/git/ref/heads/master`);
  need(ref.object?.type === 'commit' && ref.object.sha === source, 'master_raced');
}
async function checkProtection(api, base) {
  const branch = await api(`${base}/branches/master`);
  need(branch.protected === false, 'master_protected');
  const rules = await pages(api, `${base}/rulesets`);
  need(rules.length === 0, 'master_rulesets_changed');
}
async function verifyCandidate(api, base, inventory) {
  const c = inventory.candidate;
  const latest = await api(`${base}/actions/runs/${c.run_id}`);
  need(String(latest.id) === c.run_id && String(latest.run_attempt) === c.run_attempt
    && latest.head_sha === inventory.source, 'candidate_attempt_changed');
  const run = await api(`${base}/actions/runs/${c.run_id}/attempts/${c.run_attempt}`);
  need(String(run.id) === c.run_id && String(run.run_attempt) === c.run_attempt
    && run.repository?.full_name === REPOSITORY && run.head_repository?.full_name === REPOSITORY
    && run.path === '.github/workflows/v02-release.yml' && run.head_branch === 'master'
    && ['push', 'workflow_dispatch'].includes(run.event) && run.head_sha === inventory.source
    && run.status === 'completed' && run.conclusion === 'success', 'candidate_changed');
  for (const pin of inventory.artifacts) {
    const artifact = await api(`${base}/actions/artifacts/${pin.id}`);
    need(artifact.id === pin.id && artifact.name === pin.name && artifact.digest === pin.digest
      && artifact.expired === false && String(artifact.workflow_run?.id) === c.run_id
      && artifact.workflow_run?.head_sha === inventory.source, 'artifact_changed');
  }
}
const crcTable = Array.from({length: 256}, (_, index) => {
  let value = index;
  for (let bit = 0; bit < 8; bit++) value = (value >>> 1) ^ ((value & 1) ? 0xedb88320 : 0);
  return value >>> 0;
});
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = (crc >>> 8) ^ crcTable[(crc ^ byte) & 255];
  return (crc ^ 0xffffffff) >>> 0;
}
export function verifyArtifactZip(bytes) {
  need(Buffer.isBuffer(bytes) && bytes.length >= 22 && bytes.length <= 1024 * 1024 * 1024, 'invalid_artifact_zip');
  let end = -1;
  for (let offset = bytes.length - 22; offset >= Math.max(0, bytes.length - 65557); offset--) {
    if (bytes.readUInt32LE(offset) === 0x06054b50 && offset + 22 + bytes.readUInt16LE(offset + 20) === bytes.length) { end = offset; break; }
  }
  need(end >= 0 && bytes.readUInt16LE(end + 4) === 0 && bytes.readUInt16LE(end + 6) === 0, 'invalid_artifact_zip');
  const count = bytes.readUInt16LE(end + 10), start = bytes.readUInt32LE(end + 16), size = bytes.readUInt32LE(end + 12);
  need(count > 0 && count <= 64 && bytes.readUInt16LE(end + 8) === count && start + size === end, 'invalid_artifact_zip');
  const files = new Map(); let cursor = start, total = 0;
  for (let index = 0; index < count; index++) {
    need(cursor + 46 <= end && bytes.readUInt32LE(cursor) === 0x02014b50, 'invalid_artifact_zip');
    const flags = bytes.readUInt16LE(cursor + 8), method = bytes.readUInt16LE(cursor + 10);
    const crc = bytes.readUInt32LE(cursor + 16), compressed = bytes.readUInt32LE(cursor + 20), length = bytes.readUInt32LE(cursor + 24);
    const nameLength = bytes.readUInt16LE(cursor + 28), extra = bytes.readUInt16LE(cursor + 30), comment = bytes.readUInt16LE(cursor + 32);
    const mode = bytes.readUInt32LE(cursor + 38) >>> 16, local = bytes.readUInt32LE(cursor + 42);
    need(cursor + 46 + nameLength + extra + comment <= end && bytes.readUInt16LE(cursor + 34) === 0
      && !(flags & 1) && (method === 0 || method === 8) && length <= MAX_ASSET && compressed <= 1024 * 1024 * 1024
      && ((mode & 0o170000) === 0 || (mode & 0o170000) === 0o100000), 'invalid_artifact_zip');
    const nameBytes = bytes.subarray(cursor + 46, cursor + 46 + nameLength), name = nameBytes.toString('utf8');
    need(/^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$/.test(name) && !files.has(name), 'invalid_artifact_zip_name');
    need(local + 30 <= start && bytes.readUInt32LE(local) === 0x04034b50
      && bytes.readUInt16LE(local + 6) === flags && bytes.readUInt16LE(local + 8) === method, 'invalid_artifact_zip');
    const localName = bytes.readUInt16LE(local + 26), localExtra = bytes.readUInt16LE(local + 28);
    const data = local + 30 + localName + localExtra;
    need(localName === nameLength && bytes.subarray(local + 30, local + 30 + localName).equals(nameBytes)
      && data + compressed <= start, 'invalid_artifact_zip');
    total += length;
    need(total <= 1024 * 1024 * 1024, 'artifact_expansion_limit');
    const packed = bytes.subarray(data, data + compressed);
    let content;
    try { content = method === 0 ? packed : inflateRawSync(packed, {maxOutputLength: Math.max(1, length)}); }
    catch { throw new Error('invalid_artifact_deflate'); }
    need(content.length === length && crc32(content) === crc, 'artifact_crc_mismatch');
    files.set(name, content);
    cursor += 46 + nameLength + extra + comment;
  }
  need(cursor === end, 'invalid_artifact_zip');
  return files;
}
async function verifyOriginalArtifacts(inventory, downloadArtifact) {
  const found = new Set(); let bootstrapFound = false, total = 0;
  for (const pin of inventory.artifacts) {
    const zip = await downloadArtifact(pin.id, 1024 * 1024 * 1024);
    total += zip.length;
    need(total <= 4 * 1024 * 1024 * 1024 && `sha256:${inventoryDigest(zip)}` === pin.digest, 'artifact_archive_digest');
    const files = verifyArtifactZip(zip);
    for (const asset of inventory.assets) {
      if (!files.has(asset.name)) continue;
      const bytes = files.get(asset.name);
      need(!found.has(asset.name) && bytes.length === asset.size && inventoryDigest(bytes) === asset.sha256, 'original_asset_mismatch');
      found.add(asset.name);
    }
    if (pin.name.startsWith('v02-pinned-bootstrap-')) {
      need(!bootstrapFound && files.size === 1 && files.has('install.sh')
        && inventoryDigest(files.get('install.sh')) === inventory.bootstrap_sha256, 'original_bootstrap_mismatch');
      bootstrapFound = true;
    }
  }
  need(found.size === 15 && bootstrapFound, 'original_payload_incomplete');
}
async function verifyPayload(inventory, payloadRoot, bootstrapPath, readFile) {
  for (const pin of inventory.assets) {
    const bytes = await readFile(path.join(payloadRoot, pin.name), pin.size);
    need(bytes.length === pin.size && inventoryDigest(bytes) === pin.sha256, 'payload_digest_mismatch');
  }
  const bootstrap = await readFile(bootstrapPath, 1024 * 1024);
  need(inventoryDigest(bootstrap) === inventory.bootstrap_sha256, 'bootstrap_digest_mismatch');
  return bootstrap;
}
function assetsMatch(list, inventory, partial) {
  need(list.length <= 15, 'unexpected_release_assets');
  const names = new Set();
  for (const asset of list) {
    const pin = inventory.assets.find(pin => pin.name === asset.name);
    need(pin && !names.has(asset.name) && asset.state === 'uploaded' && asset.size === pin.size
      && asset.digest === `sha256:${pin.sha256}`, 'release_asset_conflict');
    need(Number.isSafeInteger(asset.id) && asset.id > 0, 'invalid_release_asset');
    names.add(asset.name);
  }
  if (!partial) need(names.size === 15, 'release_assets_incomplete');
  return names;
}
async function finalAssets(api, route, inventory, wait) {
  for (let attempt = 0; attempt < 3; attempt++) {
    const assets = await pages(api, route);
    const pending = assets.filter(asset => asset.state === 'uploaded' && asset.digest === null);
    // Validate every other property immediately. Only an absent server digest
    // on an uploaded asset may be eventual; a wrong digest is never retried.
    const provisional = assets.map(asset => {
      if (!pending.includes(asset)) return asset;
      const pin = inventory.assets.find(pin => pin.name === asset.name);
      return {...asset, digest: pin ? `sha256:${pin.sha256}` : null};
    });
    assetsMatch(provisional, inventory, false);
    if (pending.length === 0) return assets;
    need(attempt < 2, 'release_digest_unavailable');
    await wait(1000);
  }
  throw new Error('release_digest_unavailable');
}
function failureCategory(error) {
  const categories = new Set(['master_raced', 'master_protected', 'master_rulesets_changed', 'candidate_attempt_changed',
    'candidate_changed', 'artifact_changed', 'artifact_archive_digest', 'payload_digest_mismatch', 'payload_changed',
    'tag_source_conflict', 'tag_provenance_conflict', 'draft_version_conflict', 'release_version_conflict',
    'release_asset_conflict', 'release_assets_incomplete', 'release_digest_unavailable', 'github_request_failed',
    'github_http_error', 'github_response_oversized', 'public_digest_mismatch', 'public_bootstrap_mismatch',
    'public_master_bootstrap_mismatch', 'bootstrap_boundary_conflict']);
  return categories.has(error.message) ? error.message : 'publication_failed';
}
async function verifyPublic(release, inventory, downloadPublic) {
  for (const pin of inventory.assets) {
    const asset = release.find(a => a.name === pin.name);
    need(asset && typeof asset.browser_download_url === 'string', 'missing_public_url');
    const expected = `https://github.com/${REPOSITORY}/releases/download/v${inventory.version}/${pin.name}`;
    need(asset.browser_download_url === expected, 'invalid_public_url');
    const bytes = await downloadPublic(expected, pin.size);
    need(bytes.length === pin.size && inventoryDigest(bytes) === pin.sha256, 'public_digest_mismatch');
  }
}
async function verifyMasterBootstrap(context) {
  const {api, base, commit, bootstrap, downloadPublic, wait} = context;
  const backoff = [5_000, 15_000, 30_000, 60_000, 60_000, 60_000, 60_000, 60_000];
  for (let attempt = 0; attempt <= backoff.length; attempt++) {
    await unchangedMaster(api, base, commit);
    try {
      const bytes = await downloadPublic(`https://raw.githubusercontent.com/${REPOSITORY}/master/scripts/install.sh`, 1024 * 1024);
      if (bytes.equals(bootstrap)) return;
    } catch (error) {
      const transient = error.message === 'github_request_failed' || (error.message === 'github_http_error'
        && (error.status === 404 || error.status === 429 || error.status >= 500));
      if (!transient) throw error;
    }
    need(attempt < backoff.length, 'public_master_bootstrap_mismatch');
    await wait(backoff[attempt]);
  }
}
async function completedBootstrap(api, base, inventory, bootstrap) {
  const ref = await api(`${base}/git/ref/heads/master`);
  const sha = ref.object?.sha;
  need(HEX40.test(sha ?? ''), 'invalid_master');
  if (sha === inventory.source) return null;
  const commit = await api(`${base}/git/commits/${sha}`);
  need(commit.parents?.length === 1 && commit.parents[0].sha === inventory.source, 'master_raced');
  const diff = await api(`${base}/compare/${inventory.source}...${sha}`);
  need(diff.status === 'ahead' && diff.ahead_by === 1 && diff.total_commits === 1
    && diff.files?.length === 1 && diff.files[0].filename === 'scripts/install.sh'
    && diff.files[0].status === 'modified', 'bootstrap_boundary_conflict');
  const tree = await api(`${base}/git/trees/${commit.tree.sha}?recursive=1`);
  need(tree.truncated === false && Array.isArray(tree.tree), 'bootstrap_tree_incomplete');
  const file = tree.tree.filter(item => item.path === 'scripts/install.sh');
  need(file.length === 1 && file[0].mode === '100755' && file[0].type === 'blob', 'bootstrap_mode_conflict');
  const blob = await api(`${base}/git/blobs/${file[0].sha}`);
  need(blob.encoding === 'base64' && Buffer.from(blob.content, 'base64').equals(bootstrap), 'bootstrap_content_conflict');
  return sha;
}
async function createBootstrap(api, base, inventory, bootstrap) {
  const source = await api(`${base}/git/commits/${inventory.source}`);
  need(HEX40.test(source.tree?.sha ?? ''), 'invalid_source_tree');
  const blob = await api(`${base}/git/blobs`, {method: 'POST', body: {content: bootstrap.toString('base64'), encoding: 'base64'}});
  need(HEX40.test(blob.sha ?? ''), 'invalid_bootstrap_blob');
  const tree = await api(`${base}/git/trees`, {method: 'POST', body: {base_tree: source.tree.sha,
    tree: [{path: 'scripts/install.sh', mode: '100755', type: 'blob', sha: blob.sha}]}});
  need(HEX40.test(tree.sha ?? ''), 'invalid_bootstrap_tree');
  const commit = await api(`${base}/git/commits`, {method: 'POST', body: {
    message: `Pin native installer to Marmish ${inventory.version}`, tree: tree.sha, parents: [inventory.source]}});
  need(HEX40.test(commit.sha ?? ''), 'invalid_bootstrap_commit');
  await unchangedMaster(api, base, inventory.source);
  await api(`${base}/git/refs/heads/master`, {method: 'PATCH', body: {sha: commit.sha, force: false}});
  const verified = await completedBootstrap(api, base, inventory, bootstrap);
  need(verified === commit.sha, 'bootstrap_ref_mismatch');
  return commit.sha;
}

export async function publishRelease(input, adapters) {
  const {uploadAsset, downloadPublic, downloadArtifact, readFile = readPublicFile, writeState = async () => {},
    wait = milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds))} = adapters;
  let phase = 'validation', mutationsStarted = false;
  const api = (route, options = {}) => {
    if ((options.method ?? 'GET') !== 'GET') mutationsStarted = true;
    return adapters.api(route, options);
  };
  need(HEX64.test(input.inventorySha256 ?? '') && inventoryDigest(input.inventoryBytes) === input.inventorySha256, 'inventory_digest_mismatch');
  const inventory = parseInventory(input.inventoryBytes);
  need(inventory.publisher_sha === inventory.source && HEX40.test(input.publisherSha ?? ''), 'publisher_source_mismatch');
  const base = `/repos/${REPOSITORY}`;
  const state = {schema: 1, source: inventory.source, version: inventory.version, tag: `v${inventory.version}`,
    candidate: inventory.candidate, inventory_sha256: input.inventorySha256,
    bootstrap_sha256: inventory.bootstrap_sha256, assets: inventory.assets,
    publication_state: 'validated', bootstrap_commit: null, manual_source_mirror_pending: true};
  const save = async value => { state.publication_state = value; await writeState({...state}); };
  try {
  await verifyCandidate(api, base, inventory);
  await verifyOriginalArtifacts(inventory, downloadArtifact);
  const bootstrap = await verifyPayload(inventory, input.payloadRoot, input.bootstrapPath, readFile);
  await checkProtection(api, base);
  const priorBootstrap = await completedBootstrap(api, base, inventory, bootstrap);
  need(input.publisherSha === (priorBootstrap ?? inventory.source), 'publisher_not_source');
  const tag = state.tag, proof = marker(inventory, input.inventorySha256);
  const releases = await pages(api, `${base}/releases`);
  const selected = releases.filter(r => r.tag_name === tag);
  need(selected.length <= 1, 'duplicate_release');
  for (const r of releases) {
    if (!nativeTag.test(r.tag_name ?? '')) continue;
    const comparison = compareTags(r.tag_name, tag);
    need(comparison < 0 || (comparison === 0 && matchingRelease(r, tag, proof)), r.draft ? 'draft_version_conflict' : 'release_version_conflict');
  }
  const tags = await pages(api, `${base}/tags`);
  need(tags.every(t => !nativeTag.test(t.name ?? '') || compareTags(t.name, tag) <= 0), 'newer_tag_exists');
  let target = await tagTarget(api, base, tag, proof);
  need(target === null || target === inventory.source, 'tag_source_conflict');
  let release = selected[0];
  if (priorBootstrap) need(release && release.draft === false && target === inventory.source, 'bootstrap_without_publication');
  await verifyCandidate(api, base, inventory);
  if (!target) {
    phase = 'tag';
    need(!release && !priorBootstrap, 'release_without_tag');
    await unchangedMaster(api, base, inventory.source);
    const object = await api(`${base}/git/tags`, {method: 'POST', body: {tag, message: `Marmish ${inventory.version}\n\n${proof}`,
      object: inventory.source, type: 'commit'}});
    need(HEX40.test(object.sha ?? ''), 'invalid_created_tag');
    await api(`${base}/git/refs`, {method: 'POST', body: {ref: `refs/tags/${tag}`, sha: object.sha}});
    target = await tagTarget(api, base, tag, proof);
    need(target === inventory.source, 'tag_creation_mismatch');
    await save('tag_created');
  }
  if (!release) {
    phase = 'draft';
    await unchangedMaster(api, base, inventory.source);
    need(await tagTarget(api, base, tag, proof) === inventory.source, 'tag_source_conflict');
    release = await api(`${base}/releases`, {method: 'POST', body: {tag_name: tag, target_commitish: inventory.source,
      name: `Marmish ${inventory.version}`, draft: true, prerelease: true, make_latest: 'false',
      body: `Experimental Rust streaming release: streaming stdin/stdout/stderr, native file put/get with transfer-ID resume, rsync remote-shell support, streaming zlib compression and explicit cancellation. Disconnect does not cancel commands. Copy requires exactly matching peer versions; upgrade both peers together. Executable policies do not provide directory confinement. Linux AMD64/ARM64 requires glibc 2.39; macOS ARM64 requires macOS 15. macOS is ad-hoc signed, not notarized; interactive Keychain consent recovery remains unverified. Tor is optional and not a release gate. Candidate package, installed-feature, quickstart and provider checks passed on all three platforms. Full streaming qualification has not passed: the copy-edge journal assertion failed, and the two interruption cohorts were not dispatched. Historical performance figures describe source 3e93fa4 carrying 0.2.3 metadata, not these release binaries. Public HTTPS installer verification is separate and not implied by the candidate. Rollback using this new binary: marmish install --select 0.2.3; old binaries reject 0.3 version directories. Do not downgrade profiles after using newer state formats.\n\n${proof}`}});
    need(matchingRelease(release, tag, proof) && release.draft === true, 'invalid_created_release');
    await save('draft_created');
  }
  need(Number.isSafeInteger(release.id) && release.id > 0, 'invalid_release_id');
  let assets = await pages(api, `${base}/releases/${release.id}/assets`);
  const existing = assetsMatch(assets, inventory, release.draft === true);
  if (release.draft) {
    phase = 'upload';
    for (const pin of inventory.assets) {
      if (existing.has(pin.name)) continue;
      const bytes = await readFile(path.join(input.payloadRoot, pin.name), pin.size);
      need(bytes.length === pin.size && inventoryDigest(bytes) === pin.sha256, 'payload_changed');
      mutationsStarted = true;
      await uploadAsset(release.id, pin.name, bytes);
    }
    assets = await finalAssets(api, `${base}/releases/${release.id}/assets`, inventory, wait);
    await save('draft_verified');
    await unchangedMaster(api, base, inventory.source);
    need(await tagTarget(api, base, tag, proof) === inventory.source, 'tag_source_conflict');
    phase = 'publish';
    const published = await api(`${base}/releases/${release.id}`, {method: 'PATCH', body: {draft: false, prerelease: true, make_latest: 'false'}});
    need(matchingRelease(published, tag, proof) && published.draft === false, 'release_publish_mismatch');
    await save('published');
    // Publishing changes draft untagged URLs. Requery and revalidate the public
    // inventory rather than using the pre-publication representation.
    assets = await finalAssets(api, `${base}/releases/${release.id}/assets`, inventory, wait);
  }
  phase = 'public_verification';
  await verifyPublic(assets, inventory, downloadPublic);
  await save('public_assets_verified');
  phase = 'bootstrap';
  state.bootstrap_commit = priorBootstrap ?? await createBootstrap(api, base, inventory, bootstrap);
  const raw = await downloadPublic(`https://raw.githubusercontent.com/${REPOSITORY}/${state.bootstrap_commit}/scripts/install.sh`, bootstrap.length);
  need(raw.equals(bootstrap), 'public_bootstrap_mismatch');
  await verifyMasterBootstrap({api, base, commit: state.bootstrap_commit, bootstrap, downloadPublic, wait});
  await unchangedMaster(api, base, state.bootstrap_commit);
  await save('complete');
  return state;
  } catch (error) {
    if (mutationsStarted) {
      try { await writeState({...state, publication_state: 'failed', failed_phase: phase, error_category: failureCategory(error)}); }
      catch { /* Preserve the publication failure if evidence storage also fails. */ }
    }
    throw error;
  }
}

function networkError(code, status) { const error = new Error(code); error.status = status; return error; }
export function transferDeadlineMs({raw = false, limit = 0} = {}) {
  return raw || limit > 16 * 1024 * 1024 ? 600_000 : 60_000;
}
function requestOptions(token, {method, body, raw, auth, limit}) {
  const headers = {'User-Agent': 'marmish-release'};
  if (auth) { headers.Authorization = `Bearer ${token}`; headers['X-GitHub-Api-Version'] = '2022-11-28'; }
  if (body !== undefined) headers['Content-Type'] = raw ? 'application/octet-stream' : 'application/json';
  return {method, redirect: auth ? 'error' : 'follow', signal: AbortSignal.timeout(transferDeadlineMs({raw, limit})), headers,
    body: body === undefined ? undefined : raw ? body : JSON.stringify(body)};
}
async function boundedResponse(response, limit) {
  const chunks = []; let size = 0;
  for await (const chunk of response.body) {
    size += chunk.length;
    need(size <= limit, 'github_response_oversized');
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}
async function githubRequest(token, fetcher, url, {method = 'GET', body, raw = false, limit = 2 * 1024 * 1024, auth = false} = {}) {
    for (let attempt = 0; attempt < 3; attempt++) {
      let response;
      try {
        response = await fetcher(url, requestOptions(token, {method, body, raw, auth, limit}));
      } catch {
        if (method === 'GET' && attempt < 2) continue;
        throw networkError('github_request_failed');
      }
      if (!response.ok) {
        await response.body?.cancel();
        if (method === 'GET' && (response.status === 429 || response.status >= 500) && attempt < 2) continue;
        throw networkError('github_http_error', response.status);
      }
      return boundedResponse(response, limit);
    }
    throw networkError('github_retry_exhausted');
  }

export function githubAdapters(token, fetcher = fetch) {
  need(typeof token === 'string' && token.length > 0, 'missing_github_token');
  const request = (url, options) => githubRequest(token, fetcher, url, options);
  return {
    api: async (route, options = {}) => {
      need(typeof route === 'string' && route.startsWith(`/repos/${REPOSITORY}/`) && !route.includes('://'), 'invalid_api_route');
      const bytes = await request(`https://api.github.com${route}`, {...options, auth: true});
      try { return JSON.parse(bytes); } catch { throw new Error('invalid_github_json'); }
    },
    downloadArtifact: async (id, limit) => {
      need(Number.isSafeInteger(id) && id > 0, 'invalid_artifact_id');
      const response = await fetcher(`https://api.github.com/repos/${REPOSITORY}/actions/artifacts/${id}/zip`, {
        redirect: 'manual', signal: AbortSignal.timeout(60_000), headers: {Authorization: `Bearer ${token}`,
          'User-Agent': 'marmish-release', 'X-GitHub-Api-Version': '2022-11-28'}});
      need(response.status === 302, 'artifact_redirect_failed');
      const url = new URL(response.headers.get('location'));
      need(url.protocol === 'https:' && !url.username && !url.password && !url.port
        && (url.hostname.endsWith('.blob.core.windows.net') || url.hostname.endsWith('.results.githubusercontent.com')), 'invalid_artifact_redirect');
      await response.body?.cancel();
      return request(url.href, {limit});
    },
    uploadAsset: async (id, name, bytes) => {
      need(Number.isSafeInteger(id) && id > 0 && !name.includes('/'), 'invalid_upload');
      return request(`https://uploads.github.com/repos/${REPOSITORY}/releases/${id}/assets?name=${encodeURIComponent(name)}`,
        {method: 'POST', body: bytes, raw: true, auth: true});
    },
    downloadPublic: async (url, limit) => {
      need(url.startsWith(`https://github.com/${REPOSITORY}/releases/download/`) || url.startsWith(`https://raw.githubusercontent.com/${REPOSITORY}/`), 'invalid_public_download');
      return request(url, {limit});
    },
  };
}
export async function publicationMain(argv = process.argv.slice(2), env = process.env) {
  const context = parsePublicationInputs(env);
  const fields = new Map();
  for (let index = 0; index < argv.length; index += 2) {
    need(['--inventory', '--inventory-sha256', '--payload-root', '--bootstrap', '--state-out'].includes(argv[index])
      && argv[index + 1] && !fields.has(argv[index]), 'invalid_publish_arguments');
    fields.set(argv[index], argv[index + 1]);
  }
  need(fields.size === 5, 'missing_publish_arguments');
  const adapters = githubAdapters(env.GH_TOKEN ?? env.GITHUB_TOKEN);
  return publishRelease({inventoryBytes: await readPublicFile(fields.get('--inventory'), 65536),
    inventorySha256: fields.get('--inventory-sha256'), payloadRoot: fields.get('--payload-root'),
    bootstrapPath: fields.get('--bootstrap'), publisherSha: context.publisherSha}, {...adapters,
    writeState: state => fs.writeFile(fields.get('--state-out'), JSON.stringify(state) + '\n', {mode: 0o600})});
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  publicationMain().catch(error => { console.error(`marmish publisher: ${error.message}`); process.exitCode = 1; });
}
