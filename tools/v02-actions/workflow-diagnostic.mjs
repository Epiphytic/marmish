// CI-only closed diagnostics. Never serialize arbitrary exceptions or payloads.
const MODES = new Set(['validate-candidate', 'resolve', 'verify-context', 'gather',
  'provider-diagnostic-validate', 'provider-diagnostic', 'report-replay']);
const CATEGORIES = new Set(['workflow_cohort', 'workflow_context', 'context_pins',
  'rollup_context', 'bootstrap_digest', 'invalid_role', 'invalid_tooling_record',
  'tooling_provenance', 'tooling_pin', 'tooling_record_digest', 'tool_digest',
  'tool_metadata', 'artifact_retry', 'artifact_deadline', 'artifact_budget',
  'artifact_http', 'artifact_api_redirect', 'artifact_list_json', 'artifact_list_shape',
  'artifact_list_total', 'artifact_list_items', 'artifact_duplicate', 'artifact_metadata',
  'artifact_redirect_missing', 'artifact_digest', 'invalid_redirect', 'invalid_archive',
  'archive_oversized', 'response_oversized', 'response_body_missing', 'invalid_record_file',
  'record_changed', 'missing_api_token', 'invalid_workflow_mode', 'replay_provenance',
  'replay_jobs', 'replay_context', 'replay_source_delta', 'replay_candidate',
  'replay_coordinator', 'replay_head', 'replay_delta', 'replay_run', 'replay_artifacts',
  'replay_context_digest', 'replay_api', 'replay_candidate_jobs', 'replay_context_missing',
  'replay_guard', 'replay_artifact_changed', 'replay_mode']);

export function workflowDiagnostic(mode, error, write = line => console.error(line)) {
  const record = {schema: 1, component: 'workflow', event: 'failed',
    mode: MODES.has(mode) ? mode : 'unknown', category: 'unknown_error'};
  try {
    const message = error?.message;
    if (CATEGORIES.has(message)) record.category = message;
    if (record.category === 'artifact_http') {
      const observation = error?.artifactHttp;
      const status = observation?.status, target = observation?.target;
      if (Number.isInteger(status) && status >= 100 && status <= 599
        && (target === 'api' || target === 'blob')) {
        record.http_status = status; record.request_target = target;
      }
    }
  } catch { /* Hostile metadata cannot replace failure or expose its payload. */ }
  try { write(`marmish installed diagnostic: ${JSON.stringify(record)}`); }
  catch { /* A logging failure must preserve the original nonzero exit. */ }
}
