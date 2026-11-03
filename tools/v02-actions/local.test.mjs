import test from 'node:test';
import assert from 'node:assert/strict';
import {promises as fs} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {collectLocal, localBudget, localSequence, runOwned} from './local.mjs';
import {ROLES, RECORD_LIMIT} from './transport.mjs';

test('local budget binds the closed cohort and original context expiry', () => {
  const c = {schema: 1, cohort: 'autoexit', created_at: 1000, expires_at: 4300};
  assert.equal(localBudget(c, 'autoexit', ROLES[0], 4_660_000, 1_001_000), 4_300_000);
  for (const changed of [{...c, expires_at: 4301}, {...c, cohort: 'matrix'}, {...c, created_at: 1002}]) {
    assert.throws(() => localBudget(changed, 'autoexit', ROLES[0], 4_660_000, 1_001_000));
  }
  assert.throws(() => localBudget(c, 'autoexit', ROLES[0], 4_659_999, 1_001_000));
  assert.throws(() => localBudget(c, 'autoexit', ROLES[0], 4_660_000, 4_300_000));
});

test('local sequence never launches cohort after failed prepare and shares deadline', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-local-sequence-'));
  try {
    const now = Math.floor(Date.now() / 1000);
    const contextFile = path.join(root, 'context.json');
    await fs.writeFile(contextFile, JSON.stringify({schema: 1, cohort: 'uptime', created_at: now, expires_at: now + 3300}));
    const options = {contextFile, cohort: 'uptime', role: ROLES[0], jobDeadline: (now + 3660) * 1000,
      tool: path.join(root, 'tool'), workspace: path.join(root, 'work'),
      assets: path.join(root, 'assets'), bootstrap: path.join(root, 'install.sh')};
    const calls = [];
    await assert.rejects(localSequence(options, undefined, async (_, args) => {
      calls.push(args); throw new Error('failed_prepare');
    }));
    assert.equal(calls.length, 1); assert.equal(calls[0][0], 'prepare');
    const deadlines = [];
    await localSequence(options, undefined, async (_, args, owned) => {
      deadlines.push(owned.deadline);
      assert.equal(args[1], '--cohort'); assert.equal(args[2], 'uptime');
    });
    assert.deepEqual(deadlines, [(now + 3300) * 1000, (now + 3300) * 1000]);
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('owned child requires zero close and rejects nonzero and missing executable', async () => {
  const options = {cwd: os.tmpdir(), env: process.env, deadline: Date.now() + 5000, graceMs: 30, closeMs: 500};
  await runOwned('/bin/sh', ['-c', 'exit 0'], options);
  await assert.rejects(runOwned('/bin/sh', ['-c', 'exit 7'], options), /local_child_failed/);
  await assert.rejects(runOwned('/nonexistent/marmish-tool', [], options), /local_child_failed/);
});

test('deadline kills and observes an owned TERM-ignoring process group', async () => {
  const start = Date.now();
  await assert.rejects(runOwned('/bin/sh', ['-c', 'trap "" TERM; while :; do sleep 1; done'],
    {cwd: os.tmpdir(), env: process.env, deadline: start + 150, graceMs: 50, closeMs: 1000}), /local_child_failed/);
  assert.ok(Date.now() - start < 3000);
});

test('output overflow keeps draining and fails while cancellation cannot pass', async () => {
  await assert.rejects(runOwned('/bin/sh', ['-c', 'while :; do printf 0123456789; done'],
    {cwd: os.tmpdir(), env: process.env, deadline: Date.now() + 3000, graceMs: 30, closeMs: 500, outputLimit: 128}), /local_child_failed/);
  const abort = new AbortController(); abort.abort();
  await assert.rejects(runOwned('/bin/sh', ['-c', 'exit 0'],
    {cwd: os.tmpdir(), env: process.env, deadline: Date.now() + 3000, signal: abort.signal}), /local_cancelled/);
});

test('local rollup collects exactly six regular records and refuses extras and links', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-local-records-'));
  try {
    const records = path.join(root, 'roles'); await fs.mkdir(records);
    for (const role of ROLES) {
      const dir = path.join(records, role); await fs.mkdir(dir);
      for (const suffix of ['identity-ready', 'benchmark-result']) await fs.writeFile(path.join(dir, `${role}-${suffix}.json`), '{}\n');
    }
    const flat = path.join(root, 'flat');
    await collectLocal(records, flat, 'benchmark'); assert.equal((await fs.readdir(flat)).length, 6);
    const extra = path.join(records, ROLES[0], 'extra.json'); await fs.writeFile(extra, '{}');
    await assert.rejects(collectLocal(records, path.join(root, 'extra'), 'benchmark'), /local_record_set/);
    await fs.unlink(extra);
    const file = path.join(records, ROLES[0], `${ROLES[0]}-benchmark-result.json`);
    await fs.unlink(file); await fs.symlink(path.join(flat, `${ROLES[0]}-benchmark-result.json`), file);
    await assert.rejects(collectLocal(records, path.join(root, 'link'), 'benchmark'));
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('owned child rejects clean-exit residue and kills TERM-ignoring descendants', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-local-residue-'));
  try {
    for (const timed of [false, true]) {
      const pidFile = path.join(root, timed ? 'timed' : 'clean');
      const script = `(/bin/sh -c 'trap "" TERM; echo $$ > "$1"; exec sleep 30' child "$1") </dev/null >/dev/null 2>&1 &\nwhile [ ! -s "$1" ]; do sleep 0.01; done\n${timed ? 'wait' : 'exit 0'}`;
      await assert.rejects(runOwned('/bin/sh', ['-c', script, 'owner', pidFile],
        {cwd: root, env: process.env, deadline: Date.now() + (timed ? 150 : 3000), graceMs: 30, closeMs: 500}), /local_child_failed/);
      const pid = Number(await fs.readFile(pidFile, 'utf8'));
      // A killed orphan can briefly remain a zombie under a container PID 1.
      if (process.platform === 'linux') {
        const status = await fs.readFile(`/proc/${pid}/stat`, 'utf8').catch(() => 'gone');
        assert.ok(status === 'gone' || status.split(' ')[2] === 'Z', status);
      } else {
        await new Promise(resolve => setTimeout(resolve, 100));
        assert.throws(() => process.kill(pid, 0));
      }
    }
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('pipe-holding descendants remain bounded, including a setsid escape', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-local-pipes-'));
  try {
    for (const escaped of [false, ...(process.platform === 'linux' ? [true] : [])]) {
      const pidFile = path.join(root, escaped ? 'escaped' : 'ordinary');
      let pid;
      try {
        const start = Date.now();
        const command = `${escaped ? 'setsid ' : ''}/bin/sh -c 'trap "" TERM; echo $$ > "$1"; exec sleep 30' child "$1" &\nwhile [ ! -s "$1" ]; do sleep 0.01; done\nexit 0`;
        await assert.rejects(runOwned('/bin/sh', ['-c', command, 'owner', pidFile],
          {cwd: root, env: process.env, deadline: start + 150, graceMs: 30, closeMs: 100}), /local_child_failed/);
        assert.ok(Date.now() - start < 3000);
      } finally {
        pid = Number(await fs.readFile(pidFile, 'utf8'));
        try { process.kill(pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
      }
    }
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('mid-run abort stays red and production missing executable is prompt', async () => {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 100);
  try {
    await assert.rejects(runOwned('/bin/sh', ['-c', 'trap "" TERM; exec sleep 30'],
      {cwd: os.tmpdir(), env: process.env, deadline: Date.now() + 3000,
        graceMs: 30, closeMs: 500, signal: controller.signal}), /local_child_failed/);
  } finally { clearTimeout(timer); }
  const start = Date.now();
  await assert.rejects(runOwned('/nonexistent/marmish-tool', [],
    {cwd: os.tmpdir(), env: process.env, deadline: start + 3000}), /local_child_failed/);
  assert.ok(Date.now() - start < 1000);
});

test('owned child environment excludes Actions credentials', async () => {
  await runOwned(process.execPath, ['-e', `for (const name of ['GITHUB_TOKEN','ACTIONS_RUNTIME_TOKEN','ACTIONS_ID_TOKEN_REQUEST_TOKEN']) if (name in process.env) process.exit(7);`],
    {cwd: os.tmpdir(), deadline: Date.now() + 3000,
      env: {...process.env, GITHUB_TOKEN: 'test-secret', ACTIONS_RUNTIME_TOKEN: 'test-secret', ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'test-secret'}});
});

test('collector refuses missing or symlinked roles, existing destinations and oversized records', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'marmish-local-collect-'));
  try {
    const records = path.join(root, 'roles'); await fs.mkdir(records);
    for (const role of ROLES) {
      const directory = path.join(records, role); await fs.mkdir(directory);
      for (const suffix of ['identity-ready', 'uptime-result']) await fs.writeFile(path.join(directory, `${role}-${suffix}.json`), '{}');
    }
    const existing = path.join(root, 'existing'); await fs.mkdir(existing);
    await assert.rejects(collectLocal(records, existing, 'uptime'));
    const rolePath = path.join(records, ROLES[0]), saved = path.join(root, 'saved');
    await fs.rename(rolePath, saved);
    await assert.rejects(collectLocal(records, path.join(root, 'missing'), 'uptime'), /local_roles/);
    await fs.symlink(saved, rolePath);
    await assert.rejects(collectLocal(records, path.join(root, 'link'), 'uptime'), /local_role_directory/);
    await fs.unlink(rolePath); await fs.rename(saved, rolePath);
    await fs.writeFile(path.join(rolePath, `${ROLES[0]}-uptime-result.json`), Buffer.alloc(RECORD_LIMIT + 1));
    await assert.rejects(collectLocal(records, path.join(root, 'oversize'), 'uptime'));
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});
