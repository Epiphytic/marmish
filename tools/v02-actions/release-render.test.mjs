import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {reproduce, loadInspection, rendererEnvironment} from './release-render.mjs';
import {writeInspection} from './release-candidate.mjs';
import {releaseNames, inventoryDigest as sha} from './release-schema.mjs';
async function fixture() {
  const root = await fs.mkdtemp(path.join(os.tmpdir(),'marmish-render-test-'));
  const payload = new Map(releaseNames('0.2.3').map(name=>[name,Buffer.from(name)]));
  const bootstrap = Buffer.from('#!/bin/sh\nexit 0\n'), renderer = Buffer.from('renderer');
  const inventory = {schema:1,repository:'Epiphytic/marmish',publisher_sha:'a'.repeat(40),source:'a'.repeat(40),version:'0.2.3',
    candidate:{run_id:'12',run_attempt:'1'},artifacts:[{id:1,name:'artifact',digest:`sha256:${'b'.repeat(64)}`}],
    assets:[...payload].map(([name,b])=>({name,size:b.length,sha256:sha(b)})),bootstrap_sha256:sha(bootstrap),reports:[{name:'report',sha256:'c'.repeat(64)}]};
  const inputs=path.join(root,'inputs'),hash=await writeInspection({skip:false,inventory,payload,bootstrap,renderer},inputs);
  return {root,inputs,hash,bootstrap};
}
test('renderer environment excludes tokens, action command files and arbitrary inherited state',()=>{
  assert.deepEqual(rendererEnvironment({HOME:'/tmp/home',TMPDIR:'/tmp',LANG:'C',PATH:'/evil',GH_TOKEN:'secret',ACTIONS_RUNTIME_TOKEN:'secret',GITHUB_OUTPUT:'/verdict',GITHUB_ENV:'/env'}),
    {PATH:'/usr/bin:/bin:/usr/sbin:/sbin',HOME:'/tmp/home',TMPDIR:'/tmp',LANG:'C'});
});
test('tokenless renderer validates every pin before execution and requires exact reproduced bytes',async()=>{
  const f=await fixture();const calls=[];
  try {
    await reproduce(f.inputs,f.hash,path.join(f.root,'out'),{HOME:f.root,GH_TOKEN:'sentinel',GITHUB_OUTPUT:'sentinel'},(file,args,options)=>{
      calls.push({file,args,options});
      assert.equal(options.env.GH_TOKEN,undefined);assert.equal(options.env.GITHUB_OUTPUT,undefined);
      if (file.endsWith('marmish-release-tool')) return requireWrite(args[args.length-1],f.bootstrap);
    });
    assert.equal(calls.length,2);assert.equal(calls[1].file,'/bin/sh');assert.equal(calls[1].args[0],'-n');
    const r=JSON.parse(await fs.readFile(path.join(f.root,'out','render.json')));assert.equal(r.inspection_sha256,f.hash);
    await fs.chmod(path.join(f.inputs,'renderer','marmish-release-tool'),0o600);
    await fs.writeFile(path.join(f.inputs,'renderer','marmish-release-tool'),'changed');
    await assert.rejects(loadInspection(f.inputs,f.hash),/renderer_mismatch/);
  } finally {await fs.rm(f.root,{recursive:true,force:true});}
});
// execFileSync injection must remain synchronous like the real subprocess API.
import {writeFileSync} from 'node:fs';
function requireWrite(file,bytes) {writeFileSync(file,bytes,{flag:'wx'});return Buffer.alloc(0);}
test('digest tampering and renderer mismatch cannot produce a trusted render report',async()=>{
  const f=await fixture();
  try {
    await assert.rejects(loadInspection(f.inputs,'0'.repeat(64)),/digest_mismatch/);
    await assert.rejects(reproduce(f.inputs,f.hash,path.join(f.root,'out'),{},(_file,args)=>requireWrite(args.at(-1),Buffer.from('wrong'))),/reproduction_mismatch/);
    await assert.rejects(fs.access(path.join(f.root,'out','render.json')));
  } finally {await fs.rm(f.root,{recursive:true,force:true});}
});
