#!/usr/bin/env python3
"""Validate the real embedded installer and installed runtime in native CI."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import types

REPO = Path(__file__).resolve().parent.parent


# Reviewed finite vocabularies. Offline AST tests detect source drift; never expand
# these from child output or source inspection in the installed runtime.
DIAGNOSTIC_MODULES = frozenset("""
marmish_app marmish_bootstrap marmish_ci marmish_ci_bwrap marmish_e2e marmish_installed
marmish_keys marmish_mdk_build marmish_native_validation marmish_platform marmish_policy
marmish_protocol marmish_rendezvous marmish_rpc marmish_transport marmish_transport_macos
marmish_transport_sandbox mdk_smoke
""".split())

DIAGNOSTIC_ERRORS = frozenset("""
account_mismatch active_worker_migration_accepted active_worker_migration_changed_legacy_keys
active_worker_migration_created_keys active_worker_migration_wrong_error already_native
artifact_api_failed artifact_digest_mismatch artifact_download_failed
artifact_download_transient artifact_network_failed artifact_oversized binary_hash_mismatch
binary_stream_mismatch bootstrap_pipe_write bridge_broken bridge_closed bridge_frame_limit
bridge_io_failure bridge_schema bridge_timeout bwrap_cleanup_ownership
bwrap_conflicting_package_metadata bwrap_hash_mismatch bwrap_invalid_package_metadata
bwrap_invalid_payload bwrap_metadata_oversized bwrap_missing_candidate bwrap_missing_system_tool
bwrap_package_digest_mismatch bwrap_preexisting_override bwrap_provenance_unavailable
bwrap_root_refused bwrap_state_exists bwrap_tar_oversized bwrap_tool_failed
bwrap_tool_output_oversized bwrap_tool_timeout bwrap_unsafe_keyring bwrap_unsupported_platform
bwrap_untrusted_parent bwrap_wrong_elf_architecture call_timeout cancellation_failed
capability_mismatch challenge_quota cleanup_incomplete cli_process_failed client_timeout
conflict_accepted conflict_wrong_error conflicting_remote_identity control_directory_changed
control_gc_limit control_run_record_remaining cross_session_status_failed daemon_error
daemon_exited daemon_hash_mismatch daemon_home_in_use daemon_identity_changed daemon_log_limit
daemon_socket_path_too_long denial_consumed_token denied_job_created denied_job_wrong_error
destination_relay_not_configured detached_worker_did_not_finish detached_worker_not_running
disconnect_recovery_failed dns_origin_required duplicate_execution duplicate_not_existing
duplicate_peer_artifact engine_root_mismatch execution_uncertain expired_peer_metadata
explicit_key_migration_required forbidden_invocation_found forbidden_side_effect frame_limit
frame_too_large global_timeout group_purpose_mismatch hello_mismatch historical_replay_executed
historical_replay_side_effect historical_replay_wrong_error history_gap host_suspended
identities_equal immutable_key_binding internal_failure interrupted invalid_account_setup
invalid_allowlist invalid_artifact_archive invalid_artifact_list invalid_artifact_name
invalid_artifact_redirect invalid_binary invalid_bootstrap invalid_bootstrap_frame
invalid_bwrap_selection invalid_bytes invalid_ci_context invalid_command_policy
invalid_configuration invalid_cursor invalid_daemon_hash invalid_destination
invalid_execution_marker invalid_frame invalid_framing invalid_helper_timeout invalid_history
invalid_identifier invalid_json invalid_key_binding invalid_key_catalog invalid_key_journal
invalid_key_name invalid_key_profile invalid_key_secret invalid_key_selection
invalid_key_storage invalid_legacy_secret invalid_manifest invalid_namespace_ready
invalid_native_ready invalid_native_signature invalid_payload invalid_peer_artifact
invalid_peer_metadata invalid_pending_state invalid_private_record invalid_public_identity
invalid_quarantine_state invalid_relay invalid_relays invalid_response invalid_retention
invalid_room_binding invalid_room_confirmation invalid_rpc_path invalid_run_identity
invalid_run_manifest invalid_secret_import invalid_secret_store invalid_session invalid_socket
invalid_timeout invalid_uid invite_mismatch invite_not_accepted invocation_conflict
job_finished_before_disconnect job_finished_before_frontend_stop key_binding_not_prepared
key_catalog_limit key_catalog_missing key_file_limit key_helper_request_limit
key_helper_required key_helper_response key_helper_response_limit key_helper_start_failed
key_helper_timeout key_migration_required key_name_bound_elsewhere key_name_mismatch
key_package_not_published key_storage_busy key_storage_conflict legacy_key_mismatch
literal_argv_or_exit_mismatch local_io_or_schema_failure locked_frontend_fell_back
locked_frontend_started locked_frontend_wrong_error manifest_bootstrap_mismatch
manifest_helper_mismatch manifest_mismatch manifest_patch_mismatch mdk_error membership_mismatch
message_mismatch message_not_published migration_ciphertext_changed migration_engine_mismatch
migration_frontend_still_running migration_identity_changed migration_identity_mismatch
migration_not_committed migration_not_started migration_quiescence_timeout
migration_retry_changed_reserved_references migration_room_state_changed
migration_snapshot_empty missing_account_state missing_artifact_token missing_command_journal
missing_confinement_tool missing_exit_status missing_invocation_journal
missing_native_confinement missing_or_conflicting_command missing_peer_identity missing_profile
mixed_key_storage mixed_run_boot named_key_requires_native namespace_capabilities_remain
namespace_has_dns namespace_has_external_route namespace_network_mutable namespace_start_failed
namespace_start_timeout native_audit_command_failed native_audit_invalid_process
native_audit_missing_daemon native_audit_missing_worker native_audit_process_limit
native_audit_unsupported native_audit_worker_not_running native_audit_wrong_executable
native_bootstrap_fd_open native_catalog_inventory_mismatch native_cleanup_identity_failed
native_cleanup_incomplete native_cleanup_timeout native_clock_failed native_collection_invalid
native_context_invalid native_context_unavailable native_copy_hash_mismatch native_daemon_exited
native_daemon_start_failed native_daemon_start_timeout
native_endpoint_identity_capability_mismatch native_environment_leak
native_interface_probe_failed native_interface_probe_unavailable native_interrupted
native_key_mismatch native_keys_unverified native_layout_mismatch native_lock_failed
native_lock_prompt_refused native_metadata_invalid native_name_refusal_missing
native_name_refusal_wrong_error native_operation_failed native_operation_timeout
native_output_limit native_private_pipe_required native_process_changed native_profile_missing
native_provider_failed native_reference_inventory_mismatch native_reference_reused
native_report_invalid native_scan_limit native_scan_process_changed native_scan_process_owner
native_scan_secret_invalid native_scan_unsafe_file native_secret_at_rest
native_secret_file_present native_secret_in_log native_secret_in_process native_secret_in_report
native_startup_timeout native_unlock_state_invalid negative_wrong_error noncanonical_daemon_home
nonpristine_key_state nostr_key_not_prepared ordinary_room_side_effect orphan_cleanup_timeout
orphan_executable_mismatch orphan_executable_unverified orphan_identity_mismatch
orphan_identity_unverified output_gap output_limit owned_native_context_required
pairing_prerequisite_timeout peer_credentials_failed peer_not_ready pending_account_setup
pending_key_missing policy_allowed_execution_failed policy_denial_missing
policy_shell_side_effect policy_wrong_error process_identity_failed profile_identity_mismatch
profile_relay_mismatch relay_count_limit relay_lists_incomplete relay_set_mismatch
request_conflict request_too_large response_too_large restricted_ack_mismatch retention_mismatch
retention_not_configurable room_quarantined root_forbidden root_not_supported rpc_io rpc_timeout
schema_mismatch server_migration_required server_start_failed server_start_timeout
session_not_fresh session_not_reused session_quota sleep_assertion_failed socket_timeout
state_limit subprocess_failure transport_failure unexpected_log_file unrecognized_control_entry
unsafe_account_metadata unsafe_bwrap_executable unsafe_bwrap_path unsafe_control_ancestor
unsafe_control_directory unsafe_control_lock unsafe_daemon_socket unsafe_digest_file
unsafe_key_file unsafe_key_helper unsafe_key_lock unsafe_key_path unsafe_manifest
unsafe_native_artifact unsafe_private_path unsafe_private_record unsafe_state_file
unsafe_state_path unsupported_architecture unsupported_operation unsupported_platform
version_schema wrong_binding wrong_inviter wrong_peer_identity wrong_peer_uid wrong_room
""".split())

DIAGNOSTIC_STAGES = frozenset("""
active_worker_migration_refusal binary_streams cancellation cli_process command_policy complete
disconnect_submit durable_retry fresh_session identity_setup literal_arguments
locked_frontend_refusal migration_quiescence native_migration negative_frames
post_migration_native_audit reattach_after_restart server_start setup
unauthorized_and_ordinary_invitations
""".split())

DIAGNOSTIC_ASSERTIONS = frozenset("""
active_worker_refuses_migration_preserving_legacy_keys_and_native_inventory
actual_client_and_server_cli_processes_execute_command_via_marmot
byte_exact_binary_stdin_stdout_stderr_eof
exact_admitted_retry_and_status_first_retry_execute_once_conflict_rejected
file_to_native_migration_preserves_identity_rooms_and_encrypted_state
fresh_independent_mls_group_configurable_retention_same_principal_reattach
frontend_disconnect_does_not_cancel_running_worker
installed_launchers_and_bundled_defaults_preserve_special_character_argv
literal_argv_spaces_unicode_quotes_newline_dollar_backticks_leading_dash_nonutf8
live_additive_default_and_per_key_denials_path_and_shell_refusal
live_cancellation_reports_signal_and_remains_queryable
live_ordinary_room_and_unallowlisted_identity_invites_remain_unaccepted
live_stale_challenge_wrong_group_and_chat_payload_never_execute
locked_native_provider_refuses_both_frontends_without_file_fallback
native_daemon_capabilities_and_exact_server_client_identities
native_reference_inventory_catalog_and_secret_at_rest_process_checks
native_restart_capability_identity_preserved
persisted_mls_room_and_command_output_recovered_after_frontend_restart
policy_denials_create_no_job_and_preserve_token_for_allowed_execution
post_migration_native_command_and_process_audit
previous_boot_valid_control_frame_rejected_after_restart
running_native_daemon_worker_have_no_secret_arguments_environment_or_bootstrap_stdin
separate_binary_stderr_and_nonzero_exit unnamed_auto_native_profile_and_named_binding_refusals
""".split())

def failure_diagnostic(result):
    def known(value, allowed, fallback):
        return value if isinstance(value, str) and value in allowed else fallback

    assertions = result.get('assertions')
    if not isinstance(assertions, list) or len(assertions) > 64:
        assertions = []
    return {'format': 1,
            'stage': known(result.get('stage'), DIAGNOSTIC_STAGES, 'unknown'),
            'error': known(result.get('error'), DIAGNOSTIC_ERRORS, 'unrecognized_error'),
            'assertions': list(dict.fromkeys(item for item in assertions
                if isinstance(item, str) and item in DIAGNOSTIC_ASSERTIONS)),
            'local_cleanup': known(result.get('local_cleanup'),
                                   {'complete', 'incomplete'}, 'unknown')}


def installer():
    text = (REPO / 'scripts/install.sh').read_text()
    code = text.split('# BEGIN MARMISH INSTALLER PYTHON\n', 1)[1].split(
        '# END MARMISH INSTALLER PYTHON', 1)[0]
    module = types.ModuleType('marmish_distribution_installer')
    exec(compile(code, 'scripts/install.sh', 'exec'), module.__dict__)
    return module


def live_acceptance(command, manifest, archive_sha256, evidence):
    binding = {'distribution_source_commit': manifest['source_commit'],
               'distribution_archive_sha256': archive_sha256,
               'target': manifest['target']}

    def failed(category, diagnostic=None):
        result = {'outcome': 'failed', 'error': category, **binding}
        if diagnostic is not None:
            result['diagnostic'] = diagnostic
        evidence.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
        raise ValueError('installed_live_acceptance_failed') from None

    try:
        completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   text=True, timeout=960)
    except subprocess.TimeoutExpired:
        failed('installed_live_timeout')
    except (OSError, UnicodeError):
        failed('installed_live_execution_failed')
    try:
        result = json.loads(completed.stdout)
    except (ValueError, TypeError):
        failed('installed_live_invalid_json')
    if not isinstance(result, dict):
        failed('installed_live_invalid_result')
    if completed.returncode != 0 or result.get('outcome') != 'passed':
        failed('installed_live_command_failed', failure_diagnostic(result))
    result.update(binding)
    evidence.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
    return result


def acceptance(args):
    with args.archive.open('rb') as source:
        archive_sha256 = hashlib.file_digest(source, 'sha256').hexdigest()
    module = installer()
    prefix = args.prefix.resolve()
    launchers = prefix / 'bin'
    manifest = module.install_archive(args.archive, prefix, launchers, args.version,
                                      module.native_target())
    root = prefix / 'versions' / ('marmish-' + args.version + '-' + manifest['target'])
    subprocess.run([sys.executable, str(REPO / 'tools/marmish_release.py'), 'smoke',
                    '--root', str(root)], check=True)
    # A second installation must preserve the immutable version and work again.
    module.install_archive(args.archive, prefix, launchers, args.version, manifest['target'])
    for name in ('marmish', 'marmishd'):
        subprocess.run([str(launchers / name), '--help'], check=True,
                       stdout=subprocess.DEVNULL)
    if args.live:
        if not args.relay:
            raise ValueError('explicit_live_relays_required')
        native = root / 'libexec/marmish'
        command = [sys.executable, str(REPO / 'tools/marmish_e2e.py'), '--live',
                   '--wnd', str(native / 'wnd'), '--wnd-sha256',
                   hashlib.sha256((native / 'wnd').read_bytes()).hexdigest(),
                   '--wnd-manifest', str(native / 'wnd.manifest.json'),
                   '--bridge', str(native / 'marmish-engine-bridge'),
                   '--worker', str(native / 'marmish-worker'),
                   '--installed-root', str(root)]
        for relay in args.relay:
            command += ['--relay', relay]
        live_acceptance(command, manifest, archive_sha256, args.prefix.parent / 'installed-e2e.json')
    module.uninstall(prefix, launchers)
    if any((launchers / name).exists() for name in ('marmish', 'marmishd')):
        raise ValueError('uninstall_left_launcher')
    if not (root / 'release.json').is_file():
        raise ValueError('uninstall_removed_version')
    print(json.dumps({'outcome': 'passed', 'target': manifest['target'],
                      'source_commit': manifest['source_commit'],
                      'distribution_archive_sha256': archive_sha256, 'installed_live': args.live,
                      'checks': ['real_installer', 'installed_help', 'staged_provenance',
                                 'idempotence', 'marker_owned_uninstall_retains_versions']}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--live', action='store_true')
    parser.add_argument('--relay', action='append')
    acceptance(parser.parse_args())


if __name__ == '__main__':
    main()
