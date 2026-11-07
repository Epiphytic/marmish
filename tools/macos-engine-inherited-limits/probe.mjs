// Manual CI diagnostic only. No product code or limit adjustment.
import {execFileSync} from 'node:child_process';

export const PROGRAM = 'set -eu; soft=$(ulimit -Sn); hard=$(ulimit -Hn); for value in "$soft" "$hard"; do case "$value" in unlimited) ;; ""|*[!0-9]*) exit 1;; esac; done; printf "soft=%s\\nhard=%s\\n" "$soft" "$hard"';
const CEILING = 65536n;
function requireThat(condition, category) {
  if (!condition) throw new Error(category);
}
export function limit(value) {
  requireThat(typeof value === 'string', 'limit_invalid');
  if (value === 'unlimited') return 'infinity';
  requireThat(/^(0|[1-9][0-9]{0,19})$/.test(value), 'limit_invalid');
  return value;
}
function greater(a, b) {
  return a === 'infinity' ? b !== 'infinity' : b !== 'infinity' && BigInt(a) > BigInt(b);
}
export function observation(chain, soft, hard) {
  requireThat(chain === 'shell' || chain === 'node24', 'chain_invalid');
  soft = limit(soft); hard = limit(hard);
  requireThat(!greater(soft, hard), 'limit_order');
  return {schema: 1, kind: 'macos-inherited-nofile', chain, soft, hard};
}
export function parseChild(value) {
  requireThat(typeof value === 'string' && Buffer.byteLength(value) <= 4096, 'child_output');
  const fields = value.match(/^soft=([^\n]*)\nhard=([^\n]*)\n$/);
  requireThat(fields !== null, 'child_output');
  return observation('node24', fields[1], fields[2]);
}
function provenance(env, version) {
  const fields = {
    macos_version: [env['INPUT_MACOS-VERSION'], /^[0-9]{1,3}(\.[0-9]{1,3}){1,2}$/],
    image_version: [env.ImageVersion, /^[A-Za-z0-9._-]{1,80}$/],
    image_os: [env.ImageOS, /^[A-Za-z0-9._-]{1,80}$/],
    node_version: [version, /^v24\.[0-9]{1,3}\.[0-9]{1,3}$/],
  };
  return Object.fromEntries(Object.entries(fields).map(([name, [value, pattern]]) => {
    requireThat(typeof value === 'string' && pattern.test(value), 'provenance_invalid');
    return [name, value];
  }));
}
export function compare(env, version, execute = execFileSync) {
  const shell = observation('shell', env['INPUT_SHELL-SOFT'], env['INPUT_SHELL-HARD']);
  const source = provenance(env, version);
  let output;
  try {
    output = execute('/bin/sh', ['-c', PROGRAM], {
      env, stdio: ['ignore', 'pipe', 'pipe'], encoding: 'utf8',
      timeout: 5000, maxBuffer: 4096, killSignal: 'SIGKILL',
    });
  } catch {
    // Never expose child stderr or Node error objects, which include environment paths.
    throw new Error('child_failed');
  }
  const node = parseChild(output);
  return {
    schema: 1, kind: 'macos-inherited-nofile-comparison', qualification: false,
    ceiling: Number(CEILING), observations: [shell, node],
    soft_changed: shell.soft !== node.soft,
    shell_exceeds_ceiling: greater(shell.soft, String(CEILING)),
    node24_exceeds_ceiling: greater(node.soft, String(CEILING)), provenance: source,
  };
}
export function main(env = process.env, version = process.version, execute = execFileSync,
  output = value => console.log(value), failure = value => console.error(value)) {
  try {
    output(JSON.stringify(compare(env, version, execute)));
    return 0;
  } catch (error) {
    const allowed = new Set(['limit_invalid', 'chain_invalid', 'limit_order', 'child_output',
      'provenance_invalid', 'child_failed']);
    failure(`::error::macos-inherited-nofile:${allowed.has(error?.message) ? error.message : 'probe_failed'}`);
    return 1;
  }
}
