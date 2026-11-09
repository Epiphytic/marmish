import test from 'node:test';
import {readFileSync} from 'node:fs';
import assert from 'node:assert/strict';
import {canonicalInventory, parseInventory, inventoryDigest, releaseNames, validateEligibility,
  parsePublicationInputs, validVersion, releaseReportNames, parseVersion, compareVersions} from './release-schema.mjs';
export function sampleInventory() {
  return {schema: 1, repository: 'Epiphytic/marmish', publisher_sha: 'a'.repeat(40), source: 'a'.repeat(40), version: '0.2.3',
    candidate: {run_id: '101', run_attempt: '1'}, artifacts: [{id: 10, name: 'candidate-101-1', digest: `sha256:${'b'.repeat(64)}`}],
    assets: releaseNames('0.2.3').map(name => ({name, size: Buffer.byteLength(name), sha256: inventoryDigest(Buffer.from(name))})),
    bootstrap_sha256: inventoryDigest(Buffer.from('#!/bin/sh\n')), reports: releaseReportNames('0.2.3', {run_id:'101',run_attempt:'1'}).map(name => ({name,sha256:'c'.repeat(64)}))};
}
test('inventory canonicalizes all fields and roundtrips exact fifteen names', () => {
  const value = sampleInventory();
  const bytes = canonicalInventory(value);
  assert.equal(parseInventory(bytes).assets.length, 15);
  assert.equal(inventoryDigest(bytes).length, 64);
  const reverse = {...value, assets: [...value.assets].reverse()};
  assert.deepEqual(canonicalInventory(reverse), bytes);
  assert.throws(() => parseInventory(Buffer.from(JSON.stringify(value))), /noncanonical/);
});
test('inventory rejects extra fields, missing, duplicate, traversing and oversized assets', () => {
  const mutate = action => { const value = sampleInventory(); action(value); assert.throws(() => canonicalInventory(value)); };
  mutate(v => { v.secret = 'forbidden'; });
  mutate(v => v.assets.pop());
  mutate(v => { v.assets[0] = v.assets[1]; });
  mutate(v => { v.assets[0].name = '../private'; });
  mutate(v => { v.assets[0].size = 257 * 1024 * 1024; });
  mutate(v => { v.artifacts[0].digest = 'missing'; });
  mutate(v => { v.candidate.run_attempt = 1; });
  assert.throws(() => parseInventory(Buffer.alloc(65537)), /size/);
});
test('version grammar matches native uint64 bounds and positive rc', () => {
  assert.equal(validVersion('0.2.18446744073709551615-rc.18446744073709551615'), true);
  for (const value of ['0.2.18446744073709551616', '0.2.1-rc.0', '0.2.01', '0.1.0-experimental.1', null])
    assert.equal(validVersion(value), false);
});
test('eligibility has a closed tri-state reason mapping and event-before binding', () => {
  const value = {schema: 1, repository: 'Epiphytic/marmish', workflow_path: '.github/workflows/v02-release.yml',
    run_id: '1', run_attempt: '1', event: 'push', source: 'a'.repeat(40), before: 'b'.repeat(40),
    version: '0.2.3', caveated: false, decision: 'build', reason: 'eligible_version'};
  assert.equal(validateEligibility(value), value);
  assert.throws(() => validateEligibility({...value, reason: 'unchanged_version'}), /decision/);
  assert.throws(() => validateEligibility({...value, before: null}), /before/);
  assert.throws(() => validateEligibility({...value, decision: 'whatever'}), /eligibility/);
  assert.throws(() => validateEligibility({...value, untrusted: true}), /eligibility/);
});
test('publisher context accepts only exact repository and immutable SHA', () => {
  assert.deepEqual(parsePublicationInputs({GITHUB_REPOSITORY: 'Epiphytic/marmish', GITHUB_SHA: 'a'.repeat(40)}),
    {repository: 'Epiphytic/marmish', publisherSha: 'a'.repeat(40)});
  assert.throws(() => parsePublicationInputs({GITHUB_REPOSITORY: 'fork/marmish', GITHUB_SHA: 'a'.repeat(40)}));
});

test('shared cross-language version vectors use the one strict parser',()=>{
  const vectors=JSON.parse(readFileSync(new URL('../release-version-vectors.json',import.meta.url)));
  for(const value of vectors.valid) {assert.ok(parseVersion(value),value);assert.equal(validVersion(value),true,value);}
  for(const value of [...vectors.invalid,null,1,{},[]]) {assert.equal(parseVersion(value),null,String(value));assert.equal(validVersion(value),false,String(value));}
  assert.equal(compareVersions('0.2.18446744073709551615','0.3.0-rc.1'),-1);
  assert.equal(compareVersions('0.3.0-rc.18446744073709551615','0.3.0'),-1);
});
test('inventories require exact family report identities and roundtrip 0.3 releases',()=>{
  for(const version of ['0.2.3','0.3.0','0.3.1']) {
    const value=sampleInventory();value.version=version;value.assets=releaseNames(version).map(name=>({name,size:1,sha256:'a'.repeat(64)}));
    value.reports=releaseReportNames(version,value.candidate).map(name=>({name,sha256:'c'.repeat(64)}));
    assert.equal(parseInventory(canonicalInventory(value)).reports.length,version.startsWith('0.3.')?10:7);
    for(const change of [v=>v.reports.pop(),v=>v.reports.push(v.reports[0]),v=>v.reports[0].name='other',v=>v.reports[0].sha256='bad']) {
      const altered=structuredClone(value);change(altered);assert.throws(()=>canonicalInventory(altered),/invalid_inventory_report/);
    }
  }
});
