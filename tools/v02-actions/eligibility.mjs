// Read-only, dependency-free eligibility. Never executes candidate artifacts.
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {promises as fs} from 'node:fs';
import {pathToFileURL} from 'node:url';
import {validateEligibility, validVersion, ELIGIBILITY_REASONS} from './release-schema.mjs';
export {ELIGIBILITY_REASONS} from './release-schema.mjs';

const REPOSITORY = 'Epiphytic/marmish';
const WORKFLOW = '.github/workflows/v02-release.yml';
const SHA = /^[0-9a-f]{40}$/;
const ID = /^[1-9][0-9]{0,15}$/;
const NATIVE_TAG = /^v0\.2\.[0-9]+(?:-rc\.[0-9]+)?$/;
function insist(ok, reason) { if (!ok) throw new Error(reason); }
function parts(version) {
  const match = typeof version === 'string' && /^0\.2\.(0|[1-9][0-9]{0,19})(?:-rc\.([1-9][0-9]{0,19}))?$/.exec(version);
  insist(match && validVersion(version), 'invalid_version');
  return [BigInt(match[1]), match[2] ? BigInt(match[2]) : null];
}
export function compareVersions(a, b) {
  const [ap, ar] = parts(a), [bp, br] = parts(b);
  if (ap !== bp) return ap < bp ? -1 : 1;
  if (ar === br) return 0;
  if (ar === null) return 1;
  if (br === null) return -1;
  return ar < br ? -1 : 1;
}
function packageVersion(text, name) {
  const matches = text.split(/^\[\[?[^\n]+\]\]?\s*$/m).slice(1);
  const found = matches.filter(block => new RegExp(`^name\\s*=\\s*"${name}"\\s*$`, 'm').test(block));
  insist(found.length === 1, 'invalid_version');
  const versions = [...found[0].matchAll(/^version\s*=\s*"([^"\n]+)"\s*$/gm)];
  insist(versions.length === 1, 'invalid_version');
  parts(versions[0][1]);
  return versions[0][1];
}
export async function versionAt(source, git) {
  insist(SHA.test(source), 'invalid_context');
  const manifest = packageVersion(await git(['show', `${source}:crates/marmish-native/Cargo.toml`]), 'marmish-native');
  const locked = packageVersion(await git(['show', `${source}:Cargo.lock`]), 'marmish-native');
  insist(manifest === locked, 'invalid_version');
  return manifest;
}
async function list(api, kind) {
  const entries = [];
  for (let page = 1; page <= 5; page++) {
    const chunk = await api(`/repos/${REPOSITORY}/${kind}?per_page=100&page=${page}`);
    insist(Array.isArray(chunk) && chunk.length <= 100, 'inconsistent_release_state');
    entries.push(...chunk);
    if (chunk.length < 100) return entries;
  }
  throw new Error('incomplete_listing');
}
async function releaseState(api) {
  // The read-job adapter owns one persisted API budget; requests serialize.
  const allTags = await list(api, 'tags');
  const allReleases = await list(api, 'releases');
  const tags = allTags.filter(tag => typeof tag.name === 'string' && NATIVE_TAG.test(tag.name));
  const releases = allReleases.filter(release => typeof release.tag_name === 'string' && NATIVE_TAG.test(release.tag_name));
  const names = new Set();
  for (const tag of tags) {
    parts(tag.name.slice(1));
    insist(!names.has(tag.name) && SHA.test(tag.commit?.sha), 'inconsistent_release_state');
    names.add(tag.name);
  }
  const released = new Set();
  for (const release of releases) {
    parts(release.tag_name.slice(1));
    insist(!released.has(release.tag_name) && release.draft === false && names.has(release.tag_name), 'inconsistent_release_state');
    released.add(release.tag_name);
  }
  return {tags, releases};
}
function report(input, version, decision, reason) {
  return {schema:1, repository:input.repository, workflow_path:WORKFLOW,
    run_id:input.runId, run_attempt:input.runAttempt, event:input.event,
    source:input.source, before:input.event === 'push' ? input.before : null,
    version, caveated:input.caveated, decision, reason};
}
function validInput(input) {
  return input.repository === REPOSITORY && SHA.test(input.source) && input.source !== '0'.repeat(40)
    && ID.test(input.runId) && ID.test(input.runAttempt) && typeof input.caveated === 'boolean'
    && ['push','workflow_dispatch'].includes(input.event)
    && (input.event !== 'push' || input.caveated === false);
}
async function progression(input, adapters, version) {
  const {git} = adapters;
  if (input.event !== 'push') return null;
  insist(input.forced !== true, 'invalid_before');
  insist(SHA.test(input.before) && input.before !== '0'.repeat(40), 'invalid_before');
  insist((await git(['rev-parse','--is-shallow-repository'])).trim() === 'false', 'incomplete_history');
  const ancestors = (await git(['rev-list', input.source])).trim().split('\n');
  insist(ancestors.includes(input.before), 'nonancestor_before');
  const chain = (await git(['rev-list','--first-parent',input.source])).trim().split('\n');
  const index = chain.indexOf(input.before);
  insist(index >= 0, 'before_not_first_parent');
  insist(index <= 1000, 'incomplete_history');
  const commits = chain.slice(0,index).reverse();
  let previous = await versionAt(input.before,git), transitions = 0;
  for (const commit of commits) {
    insist(SHA.test(commit), 'incomplete_history');
    const current = await versionAt(commit,git);
    if (current !== previous) {
      transitions++;
      insist(compareVersions(current,previous) > 0, 'nonadvancing_version');
    }
    previous = current;
  }
  insist(previous === version, 'incomplete_history');
  insist(transitions <= 1, 'multiple_version_transitions');
  return transitions === 0 ? 'unchanged_version' : null;
}
async function selectedState(input, adapters, version, state, recovery) {
  const selected = `v${version}`;
  const selectedTag = state.tags.find(tag => tag.name === selected);
  const selectedReleases = state.releases.filter(release => release.tag_name === selected);
  const versions = [...state.tags.map(tag => tag.name.slice(1)), ...state.releases.map(release => release.tag_name.slice(1))];
  if (recovery && selectedTag) {
    insist(selectedTag.commit.sha === input.source, 'existing_version_conflict');
    insist(versions.filter(value => value !== version).every(value => compareVersions(version,value) > 0), 'nonadvancing_version');
    return null;
  }
  if (selectedTag || selectedReleases.length) {
    insist(selectedTag?.commit.sha === input.source && selectedReleases.length === 1
      && typeof adapters.verifyPublished === 'function'
      && await adapters.verifyPublished({...input,version,...state}) === true, 'existing_version_conflict');
    return 'already_published';
  }
  insist(versions.every(value => compareVersions(version,value) > 0), 'nonadvancing_version');
  return null;
}
export async function evaluateEligibility(input, adapters) {
  return evaluate(input,adapters,false);
}
async function evaluate(input, adapters, recovery) {
  let version = null;
  try {
    insist(validInput(input), 'invalid_context');
    version = await versionAt(input.source,adapters.git);
    // Troubleshooting candidates may build, but never authorize publication:
    // inspectCandidate separately requires caveated=false.
    if (input.caveated) return report(input,version,'build','eligible_version');
    const transition = await progression(input,adapters,version);
    // An unchanged version cannot authorize publication; historical release source
    // differs normally on metadata-only commits, so no release-state proof is needed.
    if (transition) return report(input,version,'skip-by-design',transition);
    const state = await releaseState(adapters.api);
    const selected = await selectedState(input,adapters,version,state,recovery);
    if (selected) return report(input,version,'skip-by-design',selected);
    return report(input,version,'build','eligible_version');
  } catch (error) {
    if (!ELIGIBILITY_REASONS.includes(error.message)) throw error;
    return report(input,version,'ineligible',error.message);
  }
}
export async function verifyEligibility(value, adapters) {
  validateEligibility(value);
  const input = {repository:value.repository,source:value.source,before:value.before,event:value.event,
    runId:value.run_id,runAttempt:value.run_attempt,caveated:value.caveated};
  const recomputed = await evaluate(input,adapters,adapters.allowSelectedTag === true && value.decision === 'build');
  insist(Object.keys(recomputed).every(key => value[key] === recomputed[key]), 'eligibility_recomputation_mismatch');
  return recomputed;
}
async function main() {
  const env = process.env;
  const eventFile = await fs.readFile(env.GITHUB_EVENT_PATH,'utf8');
  insist(Buffer.byteLength(eventFile) <= 1024 * 1024, 'event_size');
  const event = JSON.parse(eventFile);
  insist((env.SOURCE_COMMIT || env.GITHUB_SHA) === env.GITHUB_SHA, 'invalid_context');
  const execute = promisify(execFile);
  const git = async argv => (await execute('git',argv,{encoding:'utf8',timeout:30000,maxBuffer:4*1024*1024})).stdout;
  const api = async resource => {
    insist(resource.startsWith(`/repos/${REPOSITORY}/`), 'invalid_api_route');
    for (let attempt = 0; attempt < 3; attempt++) {
      let response;
      try {
        response = await fetch(`https://api.github.com${resource}`, {redirect:'error',headers:{Accept:'application/vnd.github+json',
          Authorization:`Bearer ${env.GH_TOKEN}`, 'X-GitHub-Api-Version':'2022-11-28'},signal:AbortSignal.timeout(30000)});
      } catch (error) { if (attempt < 2) continue; throw error; }
      if (!response.ok) {
        await response.body?.cancel();
        if (attempt < 2 && (response.status === 429 || response.status >= 500)) continue;
        throw new Error(`github_http_${response.status}`);
      }
      const chunks=[];let size=0;
      for await (const chunk of response.body) {
        size+=chunk.length;
        if (size > 4*1024*1024) { await response.body.cancel().catch(()=>{});throw new Error('api_size'); }
        chunks.push(chunk);
      }
      return JSON.parse(Buffer.concat(chunks).toString('utf8'));
    }
    throw new Error('github_retry_exhausted');
  };
  const value = await evaluateEligibility({repository:env.GITHUB_REPOSITORY,source:env.SOURCE_COMMIT || env.GITHUB_SHA,
    before:env.GITHUB_EVENT_NAME === 'push' ? event.before : null,event:env.GITHUB_EVENT_NAME,
    runId:env.GITHUB_RUN_ID,runAttempt:env.GITHUB_RUN_ATTEMPT,forced:env.GITHUB_EVENT_NAME === 'push' && event.forced === true,
    caveated:env.GITHUB_EVENT_NAME === 'push' ? false : event.inputs?.caveated_release === true || event.inputs?.caveated_release === 'true'}, {git,api});
  await fs.writeFile(env.ELIGIBILITY_REPORT,JSON.stringify(value)+'\n',{mode:0o600});
  if (env.GITHUB_OUTPUT) await fs.appendFile(env.GITHUB_OUTPUT,`decision=${value.decision}\nversion=${value.version ?? ''}\nsource=${SHA.test(value.source) ? value.source : ''}\nreason=${value.reason}\n`);
  if (value.decision === 'ineligible') process.exitCode = 1;
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => {console.error(`release eligibility: ${error.message}`);process.exitCode=1;});
}
