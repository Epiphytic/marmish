import test from 'node:test';
import assert from 'node:assert/strict';
import {workflowDiagnostic} from './workflow-diagnostic.mjs';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';

function record(mode, error) {
  const lines = []; workflowDiagnostic(mode, error, line => lines.push(line));
  assert.equal(lines.length, 1);
  assert.ok(Buffer.byteLength(lines[0]) < 1024);
  return JSON.parse(lines[0].slice('marmish installed diagnostic: '.length));
}
test('workflow failures retain closed stage and transport HTTP detail', () => {
  assert.deepEqual(record('gather', Object.assign(new Error('artifact_http'),
    {artifactHttp: {status: 404, target: 'api', url: 'private-sentinel'}})),
  {schema: 1, component: 'workflow', event: 'failed', mode: 'gather',
    category: 'artifact_http', http_status: 404, request_target: 'api'});
  assert.equal(record('verify-context', new Error('tooling_record_digest')).category, 'tooling_record_digest');
});
test('unknown modes and errors cannot publish arbitrary strings or metadata', () => {
  const error = {message: 'private-sentinel', get artifactHttp() { throw new Error('private-sentinel'); }};
  assert.deepEqual(record('private-sentinel', error), {schema: 1, component: 'workflow',
    event: 'failed', mode: 'unknown', category: 'unknown_error'});
});
test('hostile and changing getters are captured once without leaking or changing status', () => {
  let reads = 0;
  const error = {message: 'artifact_http', artifactHttp: {
    get status() { return ++reads === 1 ? 404 : 'private-sentinel'; }, target: 'api'}};
  assert.equal(record('gather', error).http_status, 404); assert.equal(reads, 1);
  for (const bad of [null, {status: 404, target: 'private-sentinel'}, {status: '404', target: 'api'},
    {get status() { throw new Error('private-sentinel'); }}]) {
    assert.deepEqual(Object.keys(record('gather', {message: 'artifact_http', artifactHttp: bad})),
      ['schema', 'component', 'event', 'mode', 'category']);
  }
  assert.equal(record('gather', {get message() { throw new Error('private-sentinel'); }}).category, 'unknown_error');
  assert.doesNotThrow(() => workflowDiagnostic('gather', error, () => { throw new Error('logger'); }));
});
test('real workflow CLI fails nonzero with a closed mode and category', () => {
  const child = spawnSync(process.execPath,
    [fileURLToPath(new URL('./workflow.mjs', import.meta.url)), 'private-sentinel'],
    {encoding: 'utf8', timeout: 10000, maxBuffer: 4096});
  assert.equal(child.status, 1);
  assert.equal(child.stdout, '');
  assert.ok(!child.stderr.includes('private-sentinel'));
  const result = JSON.parse(child.stderr.trim().slice('marmish installed diagnostic: '.length));
  assert.equal(result.category, 'invalid_workflow_mode');
  assert.equal(result.mode, 'unknown');
});
