// Tokenless candidate renderer orchestration: built-ins only.
import {execFileSync} from 'node:child_process';
import {promises as fs} from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {readPublicFile} from './publish.mjs';
import {parseInventory, inventoryDigest as sha, HEX64, exactKeys, requireRelease as need} from './release-schema.mjs';
export function rendererEnvironment(env) {
  const clean = {PATH:'/usr/bin:/bin:/usr/sbin:/sbin'};
  for (const key of ['HOME','TMPDIR','LANG']) if (env[key]) clean[key] = env[key];
  return clean;
}
export async function loadInspection(root, expected) {
  need(HEX64.test(expected ?? ''), 'invalid_inspection_digest');
  const bytes = await readPublicFile(path.join(root,'inspection.json'),65536);
  need(sha(bytes) === expected, 'inspection_digest_mismatch');
  const report = JSON.parse(bytes);
  exactKeys(report, ['schema','skip','inventory','renderer_sha256']);
  need(report.schema === 1 && report.skip === false && HEX64.test(report.renderer_sha256), 'invalid_inspection');
  // Canonical inventory is nested inside the hash-bound inspection.
  const {canonicalInventory} = await import('./release-schema.mjs');
  const inventory = parseInventory(canonicalInventory(report.inventory));
  for (const pin of inventory.assets) {
    const bytes = await readPublicFile(path.join(root,'assets',pin.name),pin.size);
    need(bytes.length === pin.size && sha(bytes) === pin.sha256, 'inspection_asset_mismatch');
  }
  const bootstrap = await readPublicFile(path.join(root,'bootstrap','install.sh'),131072);
  need(sha(bootstrap) === inventory.bootstrap_sha256, 'inspection_bootstrap_mismatch');
  const renderer = path.join(root,'renderer','marmish-release-tool');
  need(sha(await readPublicFile(renderer)) === report.renderer_sha256, 'inspection_renderer_mismatch');
  return {report,inventory,bootstrap,renderer};
}
export async function reproduce(root, expected, output, env, execute = execFileSync) {
  const checked = await loadInspection(root,expected);
  await fs.mkdir(output,{mode:0o700});
  await fs.chmod(checked.renderer,0o555);
  const target = path.join(output,'install.sh');
  execute(checked.renderer,['render','--assets-dir',path.join(root,'assets'),'--output',target],
    {env:rendererEnvironment(env),timeout:60000,killSignal:'SIGKILL',maxBuffer:4096,stdio:['ignore','pipe','pipe']});
  const rendered = await readPublicFile(target,131072);
  need(rendered.equals(checked.bootstrap), 'renderer_reproduction_mismatch');
  execute('/bin/sh',['-n',target],{env:rendererEnvironment(env),timeout:10000,maxBuffer:4096});
  await fs.writeFile(path.join(output,'render.json'),JSON.stringify({schema:1,inspection_sha256:expected,bootstrap_sha256:sha(rendered)})+'\n',{flag:'wx'});
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  reproduce(process.env.INSPECTION_ROOT,process.env.INSPECTION_SHA256,process.env.RENDER_ROOT,process.env)
    .catch(error=>{console.error(`marmish renderer: ${error.message}`);process.exitCode=1;});
}
