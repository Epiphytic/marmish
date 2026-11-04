#!/usr/bin/env python3
"""CI-only PTY driver for the real POSIX bootstrap and installed native setup."""
import argparse
import errno
import json
import os
from pathlib import Path
import pty
import pwd
import select
import shutil
import signal
import subprocess
import tempfile
import time


UTILITIES = ('curl', 'uname', 'id', 'mktemp', 'chmod', 'mkdir', 'rm', 'rmdir',
             'mv', 'cat', 'wc', 'tr', 'awk', 'sh', 'sha256sum', 'shasum', 'sysctl')
DIALOG = (
    (b'Role [c]lient/[s]erver/[b]oth [b]: ', b'\n'),
    (b'Explicit relay URL(s), separated by spaces: ', b'wss://relay.invalid/\n'),
    (b'Tor auto/always/off [auto, experimental]: ', b'off\n'),
    (b'Npub(s) allowed to execute here (empty disables inbound): ', b'\n'),
    (b'Trusted outbound server npub(s), independently (empty allowed): ', b'\n'),
)



INTERACTIVE_STAGES = frozenset('target manifest asset_download asset_verify runtime_probe install_validate setup_choices complete'.split())
INTERACTIVE_FAILURES = frozenset('missing_argument invalid_key_storage absolute_path_required invalid_argument missing_utility install_root_forbidden absolute_home_required native_arm64_shell_required unsupported_platform unqualified_release_template missing_checksum_utility temporary_directory missing_asset asset_size asset_read asset_download manifest_digest manifest_grammar asset_digest incompatible_runtime_check_TMPDIR_noexec incompatible_runtime_loader_or_userland runtime_probe_failed runtime_provenance native_install_failed native_setup_failed'.split())
INTERACTIVE_IDENTITY = frozenset('''
ambiguous backend_mismatch command_account command_check_account command_check_prepare
command_key_derivation command_profile_binding command_runtime_binding corrupt exists
helper_oversize helper_pipe identity_mismatch initialization_worker invalid_helper
invalid_helper_request invalid_helper_response invalid_hex invalid_manifest invalid_nostr_key
invalid_profile_name invalid_secret key_readback_mismatch keychain_authorize_ambiguous keychain_authorize_authentication_failed
keychain_authorize_backend_failure keychain_authorize_foreground_required keychain_authorize_helper_failed keychain_authorize_interaction_not_allowed keychain_authorize_interrupted
keychain_authorize_keychain_locked keychain_authorize_malformed keychain_authorize_missing keychain_authorize_uninitialized keychain_authorize_unsupported
keychain_authorize_user_cancelled keychain_authorize_worker keychain_failed keychain_profile_required keychain_verify_ambiguous
keychain_verify_authentication_failed keychain_verify_backend_failure keychain_verify_interaction_not_allowed keychain_verify_keychain_locked keychain_verify_malformed
keychain_verify_missing keychain_verify_uninitialized keychain_verify_user_cancelled locked_or_denied missing
missing_manifest owned_stop_incomplete profile_in_use profile_not_ready public_key
random random_failed runtime_collision runtime_path_too_long storage_binding_mismatch
unavailable unexpected_runtime_file uninitialized unsafe_profile_file unsafe_profile_path
unsafe_runtime_socket
'''.split())
INTERACTIVE_NATIVE = frozenset('storage transport marmot invalid_input local_io timeout'.split()) | frozenset(
    'identity_' + label for label in INTERACTIVE_IDENTITY)
INTERACTIVE_REASONS = frozenset('terminal_failure observation_limit permission_failed io_failed unexpected'.split())


def sanitize_interactive_diagnostics(value):
    # Bounded rebuilding prevents untrusted nested report data from reaching artifacts.
    source = value if type(value) is dict and len(value) <= 6 else {}
    result = {}
    for key, allowed in (('reason', INTERACTIVE_REASONS),
            ('installer_stage', INTERACTIVE_STAGES), ('installer_failure', INTERACTIVE_FAILURES),
            ('native_failure', INTERACTIVE_NATIVE)):
        item = source.get(key)
        vocabulary = allowed if key == 'reason' else allowed | {'absent'}
        result[key] = item if type(item) is str and item in vocabulary else 'diagnostic_invalid'
    for key, low, high in (('prompts_seen', 0, len(DIALOG)), ('exit_code', -128, 255)):
        item = source.get(key)
        if type(item) is int and low <= item <= high:
            result[key] = item
    return result


def interactive_snapshot(error, output, prompts_seen, status):
    reason = 'unexpected'
    if type(error) is RuntimeError:
        reason = {('quickstart_terminal_failure',): 'terminal_failure',
            ('quickstart_observation_limit',): 'observation_limit'}.get(error.args, 'unexpected')
    elif isinstance(error, PermissionError):
        reason = 'permission_failed'
    elif isinstance(error, OSError):
        reason = 'io_failed'
    result = dict(reason=reason, prompts_seen=prompts_seen,
        installer_stage='absent', installer_failure='absent', native_failure='absent')
    if type(status) is int:
        try:
            result['exit_code'] = os.waitstatus_to_exitcode(status)
        except (ValueError, TypeError):
            pass
    # Only complete LF/CRLF lines; raw terminal bytes never leave this function.
    for line in bytes(output[:2 * 1024 * 1024]).split(b'\n')[:-1]:
        if line.endswith(b'\r'):
            line = line[:-1]
        if len(line) > 1024:
            continue
        text = line.decode('ascii', errors='replace')
        if text.startswith('marmish installer debug: stage='):
            value = text[len('marmish installer debug: stage='):]
            result['installer_stage'] = value if value in INTERACTIVE_STAGES else 'diagnostic_invalid'
        elif text.startswith('marmish installer: '):
            token, separator, stage = text[len('marmish installer: '):].partition(' (stage=')
            result['installer_failure'] = token if separator and stage.endswith(')') and token in INTERACTIVE_FAILURES else 'diagnostic_invalid'
            stage = stage[:-1] if separator and stage.endswith(')') else ''
            result['installer_stage'] = stage if stage in INTERACTIVE_STAGES else 'diagnostic_invalid'
        elif text.startswith('marmish: '):
            value = text[len('marmish: '):]
            category = {'local I/O failed':'local_io', 'operation deadline exceeded':'timeout'}.get(value)
            if value.startswith('identity: '):
                candidate = 'identity_' + value[len('identity: '):]
                category = candidate if candidate in INTERACTIVE_NATIVE else None
            else:
                for prefix, family in (('storage: ', 'storage'), ('transport: ', 'transport'),
                        ('marmot: ', 'marmot'), ('invalid input: ', 'invalid_input')):
                    if value.startswith(prefix) and value[len(prefix):] and all(32 <= ord(c) <= 126 for c in value):
                        category = family
            result['native_failure'] = category or 'diagnostic_invalid'
    return sanitize_interactive_diagnostics(result)


def capture_interactive_failure(target, error, output, prompts_seen, status):
    if type(target) is not dict or target:
        return
    try:
        snapshot = interactive_snapshot(error, output, prompts_seen, status)
    except Exception:
        snapshot = sanitize_interactive_diagnostics(None)
    target.update(snapshot)

def interactive(arguments, environment, timeout=240, *, failure_diagnostics=None):
    try:
        child, master = pty.fork()
    except Exception as error:
        capture_interactive_failure(failure_diagnostics, error, b'', 0, None)
        raise
    if child == 0:
        os.execve('/bin/sh', ['/bin/sh', *arguments], environment)
    output = bytearray()
    next_prompt = 0
    status = None
    deadline = time.monotonic() + timeout
    try:
        while status is None:
            if time.monotonic() >= deadline or len(output) > 2 * 1024 * 1024:
                raise RuntimeError('quickstart_observation_limit')
            readable, _, _ = select.select([master], [], [], 0.1)
            if readable:
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b''
                output.extend(data)
            while next_prompt < len(DIALOG) and DIALOG[next_prompt][0] in output:
                os.write(master, DIALOG[next_prompt][1])
                next_prompt += 1
            observed, current = os.waitpid(child, os.WNOHANG)
            if observed == child:
                status = current
                # Reap only our child, then drain its already-buffered terminal output.
                while select.select([master], [], [], 0)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not data:
                        break
                    output.extend(data)
                    if len(output) > 2 * 1024 * 1024:
                        raise RuntimeError('quickstart_observation_limit')
        if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0 or next_prompt != len(DIALOG):
            raise RuntimeError('quickstart_terminal_failure')
        return bytes(output)
    except Exception as error:
        capture_interactive_failure(failure_diagnostics, error, output, next_prompt, status)
        raise
    finally:
        if status is None:
            # The PID/session was created here, never loaded from a file or global search.
            try:
                os.killpg(child, signal.SIGTERM)
            except ProcessLookupError:
                pass
            until = time.monotonic() + 2
            while time.monotonic() < until:
                observed, _ = os.waitpid(child, os.WNOHANG)
                if observed == child:
                    break
                time.sleep(0.05)
            else:
                try:
                    os.killpg(child, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(child, 0)
        os.close(master)


def reports(output):
    found = []
    for line in output.decode('utf-8').splitlines():
        if line.startswith('{'):
            found.append(json.loads(line))
    return found


def run(bootstrap, assets, report):
    account = pwd.getpwuid(os.geteuid())
    root = Path(tempfile.mkdtemp(prefix='.marmish-quickstart-check-', dir=account.pw_dir))
    root.chmod(0o700)
    completed = False
    result = {'schema': 1, 'kind': 'installed_quickstart_pty', 'ok': False,
              'key_storage': 'file', 'network_activation': False,
              'python_in_runtime_path': False, 'owned_state_removed': False}
    try:
        utilities = root / 'utilities'
        utilities.mkdir(mode=0o700)
        for name in UTILITIES:
            executable = shutil.which(name)
            if executable:
                (utilities / name).symlink_to(executable)
        environment = {'HOME': account.pw_dir, 'USER': account.pw_name,
                       'LOGNAME': account.pw_name, 'PATH': str(utilities),
                       'LC_ALL': 'C', 'TMPDIR': str(root)}
        state, prefix, bins = (root / name for name in ('state', 'prefix', 'bin'))
        begin = time.monotonic()
        output = interactive([str(bootstrap), '--quickstart', '--key-storage', 'file',
            '--debug', '--asset-dir', str(assets), '--state-dir', str(state),
            '--prefix', str(prefix), '--bin-dir', str(bins)], environment)
        initial = [value for value in reports(output) if 'identity_created' in value]
        if len(initial) != 1:
            raise RuntimeError('quickstart_missing_setup_report')
        initial = initial[0]
        if not (initial['identity_created'] and initial['owner_opened']
                and initial['role'] == 'both' and not initial['inbound_enabled']
                and initial['backend'] == 'file'
                and all(initial[key] for key in ('engine_stop_complete',
                    'mdk_stop_complete', 'runtime_cleanup_complete'))):
            raise RuntimeError('quickstart_setup_contract')
        repeated = subprocess.run([str(bins / 'marmish'), '--state-dir', str(state),
            '--key-storage', 'file', 'setup', '--no-grants'], env=environment,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=45, check=True)
        repeated = json.loads(repeated.stdout)
        if not (repeated['npub'] == initial['npub'] and not repeated['owner_opened']
                and not repeated['identity_created'] and not repeated['config_changed']
                and all(repeated[key] for key in ('engine_stop_complete',
                    'mdk_stop_complete', 'runtime_cleanup_complete'))):
            raise RuntimeError('quickstart_repeat_contract')
        result.update(ok=True, role='both', inbound_enabled=False,
            prompts_completed=len(DIALOG), identity_created=True,
            repeat_same_identity=True, repeat_owner_opened=False,
            stop_complete=True, elapsed_ms=round((time.monotonic() - begin) * 1000))
        completed = True
    except Exception:
        # Never publish raw terminal output, provider errors, keys or private state.
        result['failure'] = 'quickstart_validation_failed'
    finally:
        if completed:
            shutil.rmtree(root)
            result['owned_state_removed'] = not root.exists()
        report.write_text(json.dumps(result, indent=2) + '\n')
    return 0 if result['ok'] and result['owned_state_removed'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bootstrap', required=True, type=Path)
    parser.add_argument('--assets-dir', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    options = parser.parse_args()
    raise SystemExit(run(options.bootstrap.resolve(), options.assets_dir.resolve(),
                         options.report.resolve()))
