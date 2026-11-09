// Shared closed release records. No third-party imports: safe in the write job.
import {createHash} from 'node:crypto';
export const TARGETS = ['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin'];
export const REPOSITORY = 'Epiphytic/marmish';
export const HEX40 = /^[0-9a-f]{40}$/;
export const HEX64 = /^[0-9a-f]{64}$/;
export const VERSION = /^0\.([23])\.(0|[1-9][0-9]{0,19})(?:-rc\.([1-9][0-9]{0,19}))?$/;
// Discovery is deliberately broader than parsing: malformed native tags fail closed.
export const NATIVE_TAG = /^v0\.[23]\./;
export function parseVersion(value) {
  const match = typeof value === 'string' && VERSION.exec(value);
  if (!match || match[0] !== value) return null;
  const patch = BigInt(match[2]), rc = match[3] === undefined ? null : BigInt(match[3]);
  if (patch > 18446744073709551615n || (rc !== null && rc > 18446744073709551615n)) return null;
  return {minor: Number(match[1]), patch, rc};
}
export function validVersion(value) { return parseVersion(value) !== null; }
export function compareVersions(a, b) {
  const x = parseVersion(a), y = parseVersion(b);
  requireRelease(x && y, 'invalid_version');
  for (const field of ['minor', 'patch']) if (x[field] !== y[field]) return x[field] < y[field] ? -1 : 1;
  if (x.rc === y.rc) return 0;
  if (x.rc === null) return 1;
  if (y.rc === null) return -1;
  return x.rc < y.rc ? -1 : 1;
}
export function releaseReportNames(version, candidate) {
  const parsed = parseVersion(version);
  requireRelease(parsed && validId(candidate.run_id) && validId(candidate.run_attempt), 'invalid_inventory_reports');
  const suffix = `${candidate.run_id}-${candidate.run_attempt}`;
  return [`v02-eligibility-${suffix}`, ...TARGETS.flatMap(target => {
    const quickstart = `v02-quickstart-${target}-${suffix}`;
    return [quickstart, ...(parsed.minor === 3 ? [`${quickstart}-default-relays`] : []), `v02-provider-${target}-${suffix}`];
  })].sort();
}
export const MAX_ASSET = 256 * 1024 * 1024;
export function requireRelease(ok, code) { if (!ok) throw new Error(code); }
export function exactKeys(value, names, code = 'invalid_release_record') {
  requireRelease(value && typeof value === 'object' && !Array.isArray(value)
    && Object.keys(value).sort().join('\0') === [...names].sort().join('\0'), code);
}
export function validId(value) { return typeof value === 'string' && /^[1-9][0-9]{0,15}$/.test(value); }
export function inventoryDigest(bytes) { return createHash('sha256').update(bytes).digest('hex'); }
export function releaseNames(version) {
  requireRelease(validVersion(version), 'invalid_release_version');
  return TARGETS.flatMap(target => ['marmish', 'marmishd', 'marmish-worker', 'THIRD-PARTY-LICENSES']
    .map(prefix => `${prefix}-${version}-${target}`).concat(`marmish-${version}-${target}.manifest`)).sort();
}
function namedDigests(values, fields, limit, code) {
  requireRelease(Array.isArray(values) && values.length > 0 && values.length <= limit, code);
  const seen = new Set();
  for (const value of values) {
    exactKeys(value, fields, code);
    requireRelease(typeof value.name === 'string' && /^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$/.test(value.name)
      && !seen.has(value.name), code);
    seen.add(value.name);
  }
}
export function canonicalInventory(value) {
  exactKeys(value, ['schema', 'repository', 'publisher_sha', 'source', 'version', 'candidate', 'artifacts', 'assets', 'bootstrap_sha256', 'reports']);
  requireRelease(value.schema === 1 && value.repository === REPOSITORY && HEX40.test(value.publisher_sha)
    && HEX40.test(value.source) && validVersion(value.version) && HEX64.test(value.bootstrap_sha256), 'invalid_inventory_binding');
  exactKeys(value.candidate, ['run_id', 'run_attempt']);
  requireRelease(validId(value.candidate.run_id) && validId(value.candidate.run_attempt), 'invalid_inventory_candidate');
  namedDigests(value.assets, ['name', 'size', 'sha256'], 15, 'invalid_inventory_assets');
  requireRelease(value.assets.length === 15 && value.assets.map(a => a.name).sort().join('\0') === releaseNames(value.version).join('\0'), 'invalid_inventory_assets');
  for (const a of value.assets) requireRelease(Number.isSafeInteger(a.size) && a.size > 0 && a.size <= MAX_ASSET && HEX64.test(a.sha256), 'invalid_inventory_asset');
  namedDigests(value.artifacts, ['id', 'name', 'digest'], 20, 'invalid_inventory_artifacts');
  const ids = new Set();
  for (const a of value.artifacts) {
    requireRelease(Number.isSafeInteger(a.id) && a.id > 0 && !ids.has(a.id) && /^sha256:[0-9a-f]{64}$/.test(a.digest), 'invalid_inventory_artifact');
    ids.add(a.id);
  }
  namedDigests(value.reports, ['name', 'sha256'], 20, 'invalid_inventory_reports');
  requireRelease(value.reports.map(r => r.name).sort().join('\0') === releaseReportNames(value.version, value.candidate).join('\0'), 'invalid_inventory_reports');
  for (const r of value.reports) requireRelease(HEX64.test(r.sha256), 'invalid_inventory_report');
  const byName = (a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0;
  return Buffer.from(JSON.stringify({schema: 1, repository: value.repository, publisher_sha: value.publisher_sha,
    source: value.source, version: value.version, candidate: {run_id: value.candidate.run_id, run_attempt: value.candidate.run_attempt},
    artifacts: [...value.artifacts].sort(byName).map(a => ({id: a.id, name: a.name, digest: a.digest})),
    assets: [...value.assets].sort(byName).map(a => ({name: a.name, size: a.size, sha256: a.sha256})),
    bootstrap_sha256: value.bootstrap_sha256, reports: [...value.reports].sort(byName).map(r => ({name: r.name, sha256: r.sha256}))}) + '\n');
}
export function parseInventory(bytes) {
  requireRelease(Buffer.isBuffer(bytes) && bytes.length > 0 && bytes.length <= 65536, 'invalid_inventory_size');
  let value;
  try { value = JSON.parse(bytes.toString('utf8')); } catch { throw new Error('invalid_inventory_json'); }
  requireRelease(canonicalInventory(value).equals(bytes), 'noncanonical_inventory');
  return value;
}
export const ELIGIBILITY_REASONS = ['eligible_version', 'unchanged_version', 'already_published', 'invalid_context',
  'caveated_candidate', 'invalid_version', 'incomplete_history', 'invalid_before', 'nonancestor_before',
  'before_not_first_parent', 'multiple_version_transitions', 'nonadvancing_version', 'existing_version_conflict',
  'incomplete_listing', 'inconsistent_release_state'];
export function validateEligibility(value) {
  exactKeys(value, ['schema', 'repository', 'workflow_path', 'run_id', 'run_attempt', 'event', 'source', 'before', 'version', 'caveated', 'decision', 'reason'], 'invalid_eligibility');
  requireRelease(value.schema === 1 && value.repository === REPOSITORY && value.workflow_path === '.github/workflows/v02-release.yml'
    && validId(value.run_id) && validId(value.run_attempt) && ['push', 'workflow_dispatch'].includes(value.event)
    && HEX40.test(value.source) && (value.before === null || HEX40.test(value.before))
    && (validVersion(value.version) || (value.version === null && value.decision === 'ineligible' && ['invalid_version', 'invalid_context'].includes(value.reason)))
    && typeof value.caveated === 'boolean' && ['build', 'skip-by-design', 'ineligible'].includes(value.decision)
    && ELIGIBILITY_REASONS.includes(value.reason), 'invalid_eligibility');
  requireRelease((value.event === 'push') === (value.before !== null), 'invalid_eligibility_before');
  const decisions = {eligible_version: 'build', unchanged_version: 'skip-by-design', already_published: 'skip-by-design'};
  requireRelease(value.decision === (decisions[value.reason] ?? 'ineligible'), 'invalid_eligibility_decision');
  return value;
}
export function parsePublicationInputs(env) {
  requireRelease(env.GITHUB_REPOSITORY === REPOSITORY && HEX40.test(env.GITHUB_SHA ?? ''), 'invalid_publisher_context');
  return {repository: REPOSITORY, publisherSha: env.GITHUB_SHA};
}
