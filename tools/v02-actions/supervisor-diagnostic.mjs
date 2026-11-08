// Public CI logs: closed vocabulary only. Never serialize an error or its payload.
import {ROLES} from './transport.mjs';

const PREFIX = 'marmish installed diagnostic: ';
const STAGES = new Set(['setup', 'remote_list', 'remote_download', 'inbox_publish',
  'local_record_read', 'local_record_publish', 'child_terminal', 'bounds', 'cleanup']);
const CATEGORIES = new Set(['invalid_context', 'invalid_repository', 'invalid_run', 'invalid_source',
  'unsupported_cohort', 'invalid_role', 'invalid_kind', 'invalid_redirect', 'invalid_record_file',
  'record_changed', 'invalid_basename', 'record_conflict', 'archive_oversized', 'invalid_archive',
  'response_oversized', 'response_body_missing', 'missing_api_token', 'artifact_deadline',
  'artifact_budget', 'artifact_http', 'artifact_api_redirect', 'artifact_list_bound',
  'artifact_duplicate', 'artifact_metadata', 'artifact_redirect_missing', 'artifact_digest',
  'artifact_upload', 'invalid_deadline', 'invalid_mode', 'supervisor_cancelled',
  'published_record_changed', 'driver_failed', 'terminal_records_missing', 'local_identity_missing',
  'invalid_action_input', 'invalid_action_path', 'action_context_mismatch']);

function category(error) {
  try {
    const message = error?.message;
    return CATEGORIES.has(message) ? message : 'unknown_error';
  }
  catch { return 'unknown_error'; }
}
const role = value => ROLES.includes(value) ? value : null;

export function supervisorDiagnostic(diagnostic = line => console.error(line), localRole = null) {
  let stage = 'setup';
  let peerRole = null;
  let selectedRole = role(localRole);
  let emitted = false;
  return {
    role(value) { selectedRole = role(value); },
    at(value, peer = null) {
      stage = STAGES.has(value) ? value : 'setup';
      peerRole = role(peer);
    },
    failed(error) {
      if (emitted) return;
      emitted = true;
      try {
        const line = PREFIX + JSON.stringify({schema: 1, component: 'supervisor', event: 'failed',
          stage, category: category(error), role: selectedRole, peer_role: peerRole});
        if (Buffer.byteLength(line + '\n') <= 1024) {
          Promise.resolve(diagnostic(line)).catch(() => {});
        }
      } catch { /* Diagnostics never replace failure or prevent owned cleanup. */ }
    },
  };
}
