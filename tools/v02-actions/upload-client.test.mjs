import test from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {BoundedArtifactClient} from './upload-client.mjs';
function worker() {
  const child = new EventEmitter();
  child.stdout = new EventEmitter(); child.stderr = new EventEmitter();
  child.signals = [];
  child.kill = signal => { child.signals.push(signal); };
  return child;
}
function client(child, ready, settings = {}) {
  return new BoundedArtifactClient({forker: (_file, _args, options) => {
    assert.equal(options.env.GITHUB_TOKEN, undefined);
    assert.equal(options.env.LD_PRELOAD, undefined);
    setImmediate(() => ready(child)); return child;
  }, timeoutMs: 1000, forceMs: 5, observeMs: 20, ...settings});
}
test('upload joins close after exit and late IPC delivery', async () => {
  const child = worker();
  assert.deepEqual(await client(child, c => {
    c.emit('exit', 0, null); c.emit('message', {id: 42}); c.emit('close', 0, null);
  }).uploadArtifact('n', ['/file'], '/'), {id: 42});
});
test('bad IPC and excessive output terminate owned upload child', async () => {
  for (const ready of [c => c.emit('message', {id: 1, extra: true}),
    c => c.stdout.emit('data', Buffer.alloc(65537))]) {
    const child = worker();
    await assert.rejects(client(child, ready).uploadArtifact('n', ['/file'], '/'), /artifact_upload/);
    assert.deepEqual(child.signals, ['SIGTERM', 'SIGKILL']);
  }
});
test('upload timeout and mid-flight cancellation retain TERM/KILL observation', async () => {
  const timed = worker();
  await assert.rejects(client(timed, () => {}, {timeoutMs: 2}).uploadArtifact('n', ['/file'], '/'), /artifact_upload/);
  assert.deepEqual(timed.signals, ['SIGTERM', 'SIGKILL']);
  const cancelled = worker(); const controller = new AbortController();
  await assert.rejects(client(cancelled, () => controller.abort()).uploadArtifact('n', ['/file'], '/',
    {signal: controller.signal}), /artifact_upload/);
  assert.deepEqual(cancelled.signals, ['SIGTERM', 'SIGKILL']);
});
