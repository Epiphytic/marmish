// Bind exact current-run handoff IDs before trusting downloaded data.
import {pathToFileURL} from 'node:url';
import {githubAdapters} from './publish.mjs';
import {REPOSITORY, HEX40, validId, requireRelease as need} from './release-schema.mjs';
export async function verifyHandoffs(env, api) {
  need(env.GITHUB_REPOSITORY === REPOSITORY && HEX40.test(env.GITHUB_SHA ?? '')
    && validId(env.GITHUB_RUN_ID) && validId(env.GITHUB_RUN_ATTEMPT), 'invalid_handoff_context');
  const kinds = [['INPUTS_ARTIFACT_ID','inputs'], ['RENDER_ARTIFACT_ID','render']];
  if (env.INVENTORY_ARTIFACT_ID !== undefined) kinds.push(['INVENTORY_ARTIFACT_ID','inventory']);
  const seen = new Set();
  for (const [field,kind] of kinds) {
    const id = env[field];
    need(validId(id) && !seen.has(id), 'invalid_handoff_id'); seen.add(id);
    const record = await api(`/repos/${REPOSITORY}/actions/artifacts/${id}`);
    need(String(record.id) === id && record.expired === false
      && record.name === `v02-publish-${kind}-${env.GITHUB_RUN_ID}-${env.GITHUB_RUN_ATTEMPT}`
      && String(record.workflow_run?.id) === env.GITHUB_RUN_ID && record.workflow_run?.head_sha === env.GITHUB_SHA
      && record.workflow_run?.head_branch === 'master' && /^sha256:[0-9a-f]{64}$/.test(record.digest), 'handoff_provenance');
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  verifyHandoffs(process.env, githubAdapters(process.env.GH_TOKEN).api)
    .catch(error=>{console.error(`marmish handoff: ${error.message}`);process.exitCode=1;});
}
