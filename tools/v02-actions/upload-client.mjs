import {fork} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {childEnvironment} from './transport.mjs';

export class BoundedArtifactClient {
  constructor({forker = fork, timeoutMs = 30_000, forceMs = 1000, observeMs = 5000} = {}) {
    this.forker = forker; this.timeoutMs = timeoutMs; this.forceMs = forceMs; this.observeMs = observeMs;
  }
  async uploadArtifact(name, files, directory, {signal} = {}) {
    signal?.throwIfAborted();
    const env = childEnvironment(process.env);
    for (const key of ['ACTIONS_RUNTIME_TOKEN', 'ACTIONS_RESULTS_URL', 'GITHUB_RETENTION_DAYS', 'GITHUB_WORKSPACE']) {
      if (process.env[key]) env[key] = process.env[key];
    }
    return new Promise((resolve, reject) => {
      const child = this.forker(fileURLToPath(new URL('./upload.mjs', import.meta.url)), [name, files[0], directory],
        {env, stdio: ['ignore', 'pipe', 'pipe', 'ipc']});
      let id;
      let bytes = 0;
      let failed = false;
      let settled = false;
      let force;
      let observe;
      let deadline;
      const finish = (ok) => {
        if (settled) return;
        settled = true;
        clearTimeout(deadline); clearTimeout(force); clearTimeout(observe);
        signal?.removeEventListener('abort', terminate);
        if (ok) resolve({id}); else reject(new Error('artifact_upload'));
      };
      const terminate = () => {
        if (failed || settled) return;
        failed = true;
        child.kill('SIGTERM');
        force = setTimeout(() => child.kill('SIGKILL'), this.forceMs);
        observe = setTimeout(() => finish(false), this.observeMs);
      };
      const output = chunk => { bytes += chunk.length; if (bytes > 65536) terminate(); };
      child.stdout.on('data', output); child.stderr.on('data', output);
      child.on('message', message => {
        if (id !== undefined || !message || Object.keys(message).join() !== 'id'
          || !Number.isSafeInteger(message.id) || message.id <= 0) { terminate(); return; }
        id = message.id;
      });
      child.once('error', () => finish(false));
      child.once('close', (code, signal) => finish(!failed && code === 0 && signal === null && id !== undefined));
      deadline = setTimeout(terminate, this.timeoutMs);
      signal?.addEventListener('abort', terminate, {once: true});
      if (signal?.aborted) terminate();
    });
  }
}
