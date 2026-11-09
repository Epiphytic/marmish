// CI tooling only. Never imported by an installed Marmish executable.
import {createHash} from 'node:crypto';
import {constants, promises as fs} from 'node:fs';
import path from 'node:path';
import yauzl from 'yauzl';

import {isStreamingCohort} from './streaming-policy.mjs';

export const ROLES = ['linux-amd64', 'linux-arm64', 'macos-arm64'];
export const RECORD_LIMIT = 16 * 1024 * 1024;
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');
export function requireThat(ok, category) { if (!ok) throw new Error(category); }

export function binding(context, role) {
  requireThat(context.schema === 1 && ROLES.includes(role), 'invalid_context');
  requireThat(/^[A-Za-z0-9][A-Za-z0-9-]{0,38}\/[A-Za-z0-9_.-]{1,100}$/.test(context.repository)
    && !['.', '..'].includes(context.repository.split('/')[1]), 'invalid_repository');
  for (const value of [context.run_id, context.run_attempt]) {
    requireThat(/^[1-9][0-9]{0,15}$/.test(String(value)), 'invalid_run');
  }
  requireThat(/^[0-9a-f]{40}$/.test(context.source_commit), 'invalid_source');
  requireThat(context.cohort === 'matrix' || isStreamingCohort(context.cohort), 'unsupported_cohort');
  return `v02-installed-${context.run_id}-${context.run_attempt}-${context.source_commit.slice(0, 12)}-${context.cohort}`;
}
export function artifactName(prefix, role, kind) {
  requireThat(ROLES.includes(role), 'invalid_role');
  requireThat(['identity-ready', 'server-ready', 'client-result', 'server-result'].includes(kind), 'invalid_kind');
  return `${prefix}-${role}-${kind}`;
}
export function safeBlobUrl(value) {
  const url = new URL(value);
  requireThat(url.protocol === 'https:' && !url.username && !url.password && !url.port
    && (url.hostname.endsWith('.blob.core.windows.net')
      || url.hostname.endsWith('.results.githubusercontent.com')), 'invalid_redirect');
  return url;
}
export function childEnvironment(env) {
  const clean = {PATH: '/usr/bin:/bin:/usr/sbin:/sbin'};
  for (const key of ['HOME', 'TMPDIR', 'LANG']) if (env[key]) clean[key] = env[key];
  return clean;
}
export async function boundedFile(file, limit = RECORD_LIMIT) {
  const handle = await fs.open(file, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
  try {
    const stat = await handle.stat();
    requireThat(stat.isFile() && stat.size <= limit, 'invalid_record_file');
    const bytes = Buffer.alloc(stat.size + 1);
    let read = 0;
    while (read < bytes.length) {
      const next = await handle.read(bytes, read, bytes.length - read, null);
      if (!next.bytesRead) break;
      read += next.bytesRead;
    }
    requireThat(read === stat.size, 'record_changed');
    return bytes.subarray(0, read);
  } finally { await handle.close(); }
}
export async function putImmutable(directory, basename, bytes) {
  requireThat(path.basename(basename) === basename, 'invalid_basename');
  await fs.mkdir(directory, {recursive: true, mode: 0o700});
  const target = path.join(directory, basename);
  try {
    requireThat((await boundedFile(target)).equals(bytes), 'record_conflict');
    return;
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  // Only this single supervisor writes its owned inbox. Exclusive temp + link
  // publishes without replacing an existing record, even if another writer races.
  const staging = await fs.mkdtemp(path.join(path.dirname(directory), '.marmish-record-'));
  const temporary = path.join(staging, basename);
  await fs.writeFile(temporary, bytes, {flag: 'wx', mode: 0o600});
  try {
    await fs.link(temporary, target);
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    requireThat((await boundedFile(target)).equals(bytes), 'record_conflict');
  } finally { await fs.rm(staging, {recursive: true, force: true}); }
}
export async function singleJson(zip, basename) {
  requireThat(zip.length <= RECORD_LIMIT, 'archive_oversized');
  return new Promise((resolve, reject) => {
    yauzl.fromBuffer(zip, {lazyEntries: true, strictFileNames: true, validateEntrySizes: true}, (error, archive) => {
      if (error) { reject(new Error('invalid_archive')); return; }
      let count = 0;
      let bytes;
      let failed = false;
      const fail = () => {
        if (!failed) { failed = true; archive.close(); reject(new Error('invalid_archive')); }
      };
      archive.on('error', fail);
      archive.on('entry', entry => {
        const type = (entry.externalFileAttributes >>> 16) & 0o170000;
        if (++count !== 1 || entry.fileName !== basename || (type !== 0 && type !== 0o100000)
          || entry.uncompressedSize > RECORD_LIMIT || entry.compressedSize > RECORD_LIMIT
          || (entry.generalPurposeBitFlag & 1)) { fail(); return; }
        archive.openReadStream(entry, (error, stream) => {
          if (error) { fail(); return; }
          const chunks = [];
          let size = 0;
          stream.on('error', fail);
          stream.on('data', chunk => {
            size += chunk.length;
            if (size > RECORD_LIMIT) { stream.destroy(); fail(); }
            else chunks.push(chunk);
          });
          stream.on('end', () => {
            if (failed) return;
            bytes = Buffer.concat(chunks);
            if (bytes.length !== entry.uncompressedSize) { fail(); return; }
            archive.readEntry();
          });
        });
      });
      archive.on('end', () => {
        if (failed) return;
        if (count !== 1 || !bytes) { fail(); return; }
        try { JSON.parse(bytes.toString('utf8')); } catch { fail(); return; }
        resolve(bytes);
      });
      archive.readEntry();
    });
  });
}

async function body(response, limit) {
  const declared = response.headers.get('content-length');
  requireThat(declared === null || (/^[0-9]+$/.test(declared) && Number(declared) <= limit), 'response_oversized');
  requireThat(response.body, 'response_body_missing');
  const reader = response.body.getReader();
  const chunks = [];
  let size = 0;
  try {
    for (;;) {
      const {done, value} = await reader.read();
      if (done) break;
      size += value.length;
      requireThat(size <= limit, 'response_oversized');
      chunks.push(value);
    }
  } finally { await reader.cancel(); }
  return Buffer.concat(chunks);
}

function parseArtifactList(bytes) {
  let result;
  const fail = (category, reason) => {
    const total = result?.total_count;
    const count = Array.isArray(result?.artifacts) ? result.artifacts.length : null;
    const error = new Error(category);
    error.artifactList = {reason,
      total_count: Number.isSafeInteger(total) && total >= 0 && total <= 100 ? total : null,
      artifact_count: count !== null && count <= 100 ? count : null};
    if (category === 'artifact_retry') error.retryAfter = 30_000;
    throw error;
  };
  try { result = JSON.parse(bytes); }
  catch { fail('artifact_list_json', 'invalid_json'); }
  if (!result || typeof result !== 'object' || Array.isArray(result)) fail('artifact_list_shape', 'invalid_object');
  if (!Number.isSafeInteger(result.total_count) || result.total_count < 0) fail('artifact_list_total', 'invalid_total');
  if (result.total_count > 100) fail('artifact_list_total', 'total_bound');
  if (!Array.isArray(result.artifacts)) fail('artifact_list_items', 'invalid_items');
  if (result.artifacts.length > 100) fail('artifact_list_items', 'items_bound');
  // Never return or scan a partial snapshot. Only the supervisor retries this
  // typed, bounded inconsistency, under its original deadline and API budget.
  if (result.artifacts.length !== result.total_count) fail('artifact_retry', 'count_mismatch');
  return result.artifacts;
}

export class ArtifactTransport {
  constructor({context, token, budgetFile, fetcher = fetch}) {
    requireThat(typeof token === 'string' && token.length > 0, 'missing_api_token');
    this.context = context; this.token = token; this.budgetFile = budgetFile;
    this.fetcher = fetcher; this.used = null;
  }
  async request(url, api, limit, deadline, signal) {
    signal?.throwIfAborted();
    requireThat(Date.now() < deadline, 'artifact_deadline');
    if (api) {
      if (this.used === null) {
        try { this.used = JSON.parse(await fs.readFile(this.budgetFile, 'utf8')).used; }
        catch (error) { if (error.code !== 'ENOENT') throw error; this.used = 0; }
      }
      requireThat(Number.isInteger(this.used) && this.used >= 0 && this.used < 120, 'artifact_budget');
      this.used++;
      const temporary = `${this.budgetFile}.${process.pid}.tmp`;
      await fs.writeFile(temporary, JSON.stringify({used: this.used}), {flag: 'wx', mode: 0o600});
      await fs.rename(temporary, this.budgetFile);
    }
    const headers = api ? {Authorization: `Bearer ${this.token}`, Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28'} : {};
    let response;
    try {
      response = await this.fetcher(url, {headers, redirect: 'manual',
        signal: AbortSignal.any([...(signal ? [signal] : []),
          AbortSignal.timeout(Math.max(1, Math.min(30_000, deadline - Date.now())))])});
    } catch {
      signal?.throwIfAborted();
      const error = new Error('artifact_retry'); error.retryAfter = 30_000; throw error;
    }
    if ([429, 500, 502, 503, 504].includes(response.status)
      || (response.status === 403 && (response.headers.has('retry-after')
        || response.headers.get('x-ratelimit-remaining') === '0'))) {
      await response.body?.cancel();
      const retry = response.headers.get('retry-after');
      const seconds = /^[0-9]{1,5}$/.test(retry ?? '') ? Math.min(Number(retry), 300) : 30;
      const error = new Error('artifact_retry'); error.retryAfter = seconds * 1000; throw error;
    }
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      await response.body?.cancel();
      return {redirect: response.headers.get('location')};
    }
    requireThat(response.status === 200, 'artifact_http');
    return {bytes: await body(response, limit)};
  }
  async list(deadline, signal) {
    const url = `https://api.github.com/repos/${this.context.repository}/actions/runs/${this.context.run_id}/artifacts?per_page=100`;
    const reply = await this.request(url, true, 2 * 1024 * 1024, deadline, signal);
    requireThat(reply.bytes, 'artifact_api_redirect');
    return parseArtifactList(reply.bytes);
  }
  async download(entries, name, basename, deadline, signal) {
    const matching = entries.filter(entry => entry.name === name);
    requireThat(matching.length <= 1, 'artifact_duplicate');
    if (!matching.length) return null;
    const entry = matching[0];
    requireThat(!entry.expired && Number.isSafeInteger(entry.id) && entry.id > 0
      && Number.isSafeInteger(entry.size_in_bytes) && entry.size_in_bytes > 0 && entry.size_in_bytes <= RECORD_LIMIT
      && /^sha256:[0-9a-f]{64}$/.test(entry.digest), 'artifact_metadata');
    const url = `https://api.github.com/repos/${this.context.repository}/actions/artifacts/${entry.id}/zip`;
    const redirect = await this.request(url, true, RECORD_LIMIT, deadline, signal);
    requireThat(redirect.redirect, 'artifact_redirect_missing');
    const reply = await this.request(safeBlobUrl(redirect.redirect), false, RECORD_LIMIT, deadline, signal);
    requireThat(reply.bytes && `sha256:${digest(reply.bytes)}` === entry.digest, 'artifact_digest');
    return singleJson(reply.bytes, basename);
  }
}
