// Closed CI-only streaming cohorts. Product deadlines remain separately enforced.
export const STREAMING_COHORTS = Object.freeze([
  'stream-matrix', 'rsync-matrix', 'copy-matrix', 'edge-stream', 'edge-rsync', 'edge-copy',
  'copy-interrupt-put', 'copy-interrupt-get',
]);
export const isStreamingCohort = name => STREAMING_COHORTS.includes(name);
export const STREAMING_BOUNDS = Object.freeze({
  lifetime: 3300, preparation: 900, latestServerStart: 720,
  ready: 180, traffic: 1200, cleanup: 180, uptime: 1800,
  minimumRemaining: 1980, watchdogReserve: 420, artifactCleanup: 300,
});

// Seconds since epoch, like the native Context and Actions job-deadline input.
export function streamingServerBudget(context, serverStart, now = serverStart) {
  const b = STREAMING_BOUNDS;
  if (!isStreamingCohort(context.cohort)
      || ![context.created_at, context.expires_at, serverStart, now].every(Number.isSafeInteger)
      || context.expires_at <= context.created_at
      || context.expires_at - context.created_at > b.lifetime
      || serverStart < context.created_at || serverStart > context.created_at + b.latestServerStart
      || context.expires_at - serverStart < b.minimumRemaining || now < serverStart) {
    throw new Error('streaming_start_budget');
  }
  const readyBy = serverStart + b.ready;
  const trafficBy = Math.min(readyBy + b.traffic, context.expires_at - b.cleanup);
  const stopBy = trafficBy + b.cleanup;
  if (now >= readyBy || stopBy > serverStart + b.uptime) throw new Error('streaming_start_budget');
  return {readyBy, trafficBy, stopBy};
}
