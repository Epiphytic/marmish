import test from 'node:test';
import assert from 'node:assert/strict';
import {verifyHandoffs} from './release-handoff.mjs';
const env={GITHUB_REPOSITORY:'Epiphytic/marmish',GITHUB_SHA:'a'.repeat(40),GITHUB_RUN_ID:'21',GITHUB_RUN_ATTEMPT:'2',INPUTS_ARTIFACT_ID:'31',RENDER_ARTIFACT_ID:'32',INVENTORY_ARTIFACT_ID:'33'};
const records=['inputs','render','inventory'].map((kind,i)=>({id:31+i,name:`v02-publish-${kind}-21-2`,expired:false,digest:`sha256:${'b'.repeat(64)}`,workflow_run:{id:21,head_sha:env.GITHUB_SHA,head_branch:'master'}}));
test('handoff accepts exact IDs/names/current publisher run and refuses every changed binding',async()=>{
  const api=async route=>structuredClone(records.find(a=>route.endsWith(`/${a.id}`)));
  await verifyHandoffs(env,api);
  for(const mutate of [r=>r.expired=true,r=>r.id++,r=>r.name+='x',r=>r.digest=null,r=>r.workflow_run.id++,r=>r.workflow_run.head_sha='c'.repeat(40),r=>r.workflow_run.head_branch='other']) {
    await assert.rejects(verifyHandoffs(env,async route=>{const r=await api(route);mutate(r);return r;}),/handoff_provenance/);
  }
  await assert.rejects(verifyHandoffs({...env,RENDER_ARTIFACT_ID:'31'},api),/invalid_handoff_id/);
});
