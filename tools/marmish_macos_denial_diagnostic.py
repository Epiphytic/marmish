#!/usr/bin/env python3
"""Collect finite candidate denial facts for one owned synthetic macOS crash."""
import argparse
from datetime import datetime, timedelta, timezone
import errno
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import socket
import subprocess
import sys
import tempfile
import time

import marmish_macos_crash_diagnostic as crash
import marmish_macos_probe_diagnostic as base

OUT_CAP = 256 * 1024
ERR_CAP = 4096
MAX_RECORDS = 128
OPERATIONS = frozenset(('file-read-data', 'file-read-metadata', 'file-map-executable',
    'process-exec', 'mach-lookup', 'sysctl-read', 'system-fsctl', 'file-ioctl',
    'file-test-existence', 'process-info-codesignature', 'iokit-open', 'system-info',
    'ipc-posix-shm-open', 'ipc-posix-shm-read-data', 'ipc-posix-shm-write-data',
    'ipc-posix-shm-write-create', 'ipc-posix-shm-unlink'))
NAME = re.compile(r'mp-[0-9a-f]{8}\Z', re.ASCII)
STAMP = re.compile(r'\d{4}-\d{2}-\d{2}[ T]\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?[+-](?:[01][0-9]|2[0-3]):?[0-5][0-9]\Z', re.ASCII)
DENIAL = re.compile(r'Sandbox: (mp-[0-9a-f]{8})\(([1-9][0-9]{0,9})\) deny\(([1-9][0-9]{0,8})\) ([^ ]+)(?: (.+))?\Z', re.ASCII)
RESOURCES = {'/usr/lib/dyld': 'system_dyld',
    '/usr/lib/libSystem.B.dylib': 'system_libsystem', '/dev/null': 'fixture_dev_null',
    '/System/Library/dyld': 'system_dyld_directory',
    '/System/Library/dyld/dyld_shared_cache_arm64e': 'system_shared_cache',
    '/System/Library/dyld/dyld_shared_cache_x86_64': 'system_shared_cache',
    '/System/Volumes/Preboot/Cryptexes/OS': 'system_cryptex_os'}
CRYPTEX = re.compile(r'(?:/System/Volumes/Preboot/Cryptexes/OS|/System/Cryptexes/OS|/private/preboot/Cryptexes/OS)/System/Library/dyld/dyld_shared_cache_(?:arm64e|x86_64)(?:\.[0-9]{1,3})?\Z', re.ASCII)


class CleanupFailed(Exception):
    """Owned process cleanup could not be established; carries no private text."""

    def __init__(self, reason, error, stage, signal_attempted, process):
        if (reason not in ('group_signal_failed', 'leader_wait_timeout', 'leader_wait_failed')
                or stage not in ('waiting', 'exit_observed_unconfirmed', 'exit_confirmed', 'observation_failed')
                or type(signal_attempted) is not bool
                or process not in ('preflight', 'control', 'confined', 'query', 'unspecified')):
            raise ValueError('cleanup_fact_invalid')
        super().__init__()
        self.facts = {'category': 'cleanup_failed', 'reason': reason,
            'errno_class': cleanup_errno(error), 'observation_stage': stage,
            'signal_attempted': signal_attempted, 'process': process}


def cleanup_errno(error):
    if not isinstance(error, OSError):
        return 'none'
    return {errno.EPERM: 'permission_denied', errno.EACCES: 'permission_denied',
            errno.ECHILD: 'no_child', errno.EINVAL: 'invalid'}.get(error.errno, 'other')


def terminate(_signum, _frame):
    # Reuse the owned-group and fixture cleanup path without printing a traceback.
    raise KeyboardInterrupt


def shape_default():
    return {'top_level': 'other', 'leading_text': False, 'trailing_summary': False,
            'keysets': [], 'timestamp_parse': 'not_observed'}


def log_result(category='not_queried', shape=None):
    return {'category': category, 'query_exit_code': None, 'query_signal': None,
            'matched_count': None, 'denials': [], 'format_shape': shape or shape_default()}


def timestamp(value):
    if not isinstance(value, str) or not STAMP.fullmatch(value):
        raise ValueError('timestamp_invalid')
    parsed = datetime.fromisoformat(value)
    return parsed.astimezone(timezone.utc)


def query_window(start, end, elapsed):
    wall = (end - start).total_seconds()
    if not (0 <= wall <= 7 and 0 <= elapsed <= 7 and abs(wall - elapsed) <= 1):
        raise ValueError('clock_window_invalid')
    lower = start.replace(microsecond=0)
    upper = end.replace(microsecond=0) + (timedelta(seconds=1) if end.microsecond else timedelta())
    if (upper - lower).total_seconds() > 9:
        raise ValueError('clock_window_invalid')
    return lower, upper


def query_command(name, pid, window):
    if not NAME.fullmatch(name) or type(pid) is not int or not 1 <= pid < 2 ** 31:
        raise ValueError('identity_invalid')
    predicate = 'process == "kernel" AND eventMessage CONTAINS "Sandbox: ' + name + '(' + str(pid) + ') deny("'
    return ['/usr/bin/log', 'show', '--style', 'json', '--start',
            window[0].strftime('%Y-%m-%d %H:%M:%S+0000'), '--end',
            window[1].strftime('%Y-%m-%d %H:%M:%S+0000'), '--predicate', predicate]


def resource_class(target):
    if target is None:
        return 'none'
    if target in RESOURCES:
        return RESOURCES[target]
    return 'system_cryptex_shared_cache' if CRYPTEX.fullmatch(target) else 'unclassified'


def reject_constant(_):
    raise ValueError('nonfinite_json')


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate_key')
        result[key] = value
    return result


def bounded_depth(text):
    depth, quoted, escaped = 0, False, False
    for char in text:
        if quoted:
            if escaped:
                escaped = False
            elif char == '\\':
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in '[{':
            depth += 1
            if depth > 32:
                raise ValueError('json_depth')
        elif char in ']}':
            depth -= 1


def record_keyset(record):
    if not isinstance(record, dict):
        return 'other'
    if {'timestamp', 'processImagePath', 'eventMessage'} <= record.keys():
        return 'expected'
    return 'summary' if {'count', 'finished'} <= record.keys() else 'other'


def decode_collection(raw):
    """Return private parsed records and a content-free, finite shape."""
    shape = shape_default()
    text = raw.decode('utf-8', errors='strict')
    stripped = text.lstrip()
    shape['top_level'] = ('empty' if not stripped else 'array' if stripped[0] == '['
                          else 'object' if stripped[0] == '{' else 'other')
    shape['leading_text'] = bool(stripped) and stripped[0] not in '[{'
    bounded_depth(text)
    decoder = json.JSONDecoder(object_pairs_hook=unique_object, parse_constant=reject_constant)
    try:
        value, end = decoder.raw_decode(stripped)
    except (ValueError, RecursionError):
        return None, shape
    rest = stripped[end:].strip()
    if rest:
        try:
            trailer = decoder.decode(rest)
            shape['trailing_summary'] = record_keyset(trailer) == 'summary'
        except (ValueError, RecursionError):
            pass
    observations = value if isinstance(value, list) else [value]
    shape['keysets'] = sorted({record_keyset(record) for record in observations})
    observations = observations[:MAX_RECORDS + 1]
    stamps = set()
    for record in observations:
        if isinstance(record, dict) and 'timestamp' in record:
            try:
                value_stamp = record['timestamp']
                if not isinstance(value_stamp, str) or len(value_stamp.encode('utf-8')) > 64:
                    raise ValueError('timestamp_invalid')
                timestamp(value_stamp)
                stamps.add('valid')
            except (ValueError, OverflowError):
                stamps.add('invalid')
    if stamps:
        shape['timestamp_parse'] = next(iter(stamps)) if len(stamps) == 1 else 'mixed'
    return (value if isinstance(value, list) and not rest else None), shape


def parse_record(record, name, pid, window):
    for key, cap in (('timestamp', 64), ('processImagePath', 4096), ('eventMessage', 8192)):
        value = record.get(key) if isinstance(record, dict) else None
        if not isinstance(value, str) or len(value.encode('utf-8')) > cap:
            raise ValueError('record_invalid')
    if record['processImagePath'] != '/kernel' or not window[0] <= timestamp(record['timestamp']) <= window[1]:
        raise ValueError('record_unattributed')
    message = record['eventMessage']
    if any(ord(char) < 32 or ord(char) > 126 for char in message):
        raise ValueError('message_invalid')
    match = DENIAL.fullmatch(message)
    if match is None or match[1] != name or match[2] != str(pid) or int(match[3]) == 0:
        raise ValueError('message_unattributed')
    return (match[4] if match[4] in OPERATIONS else 'unknown', resource_class(match[5]))


def parse_denials(raw, name, pid, window):
    result = log_result('format_unknown')
    if len(raw) > OUT_CAP:
        result['category'] = 'query_output_limit'
        return result
    try:
        records, result['format_shape'] = decode_collection(raw)
        if records is None:
            return result
        if len(records) > MAX_RECORDS:
            result['category'] = 'record_limit'
            return result
        pairs = {parse_record(record, name, pid, window) for record in records}
    except (ValueError, UnicodeError, OverflowError, RecursionError):
        return result
    if len(pairs) > 8:
        result['category'] = 'matching_limit'
        return result
    result['denials'] = [{'operation': operation, 'resource_class': target}
                         for operation, target in sorted(pairs)]
    result['matched_count'] = len(pairs)
    result['category'] = ('operation_unknown' if any(operation == 'unknown' for operation, _ in pairs)
                          else 'owned_denial_found' if pairs else 'no_owned_denial')
    return result


def observe(child):
    return os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)


def observed_exit(child, status):
    if status.si_pid != child.pid or type(status.si_status) is not int:
        raise ValueError('observation_identity_invalid')
    if status.si_code == os.CLD_EXITED and 0 <= status.si_status <= 255:
        return status.si_status
    if status.si_code in (os.CLD_KILLED, os.CLD_DUMPED) and 1 <= status.si_status < signal.NSIG:
        return -status.si_status
    raise ValueError('observation_status_invalid')


def observation_key(status):
    return status.si_pid, status.si_code, status.si_status


def run_private(command, root, timeout=5, stdout_cap=OUT_CAP, stderr_cap=ERR_CAP,
                capture_stdout=True, *, process='unspecified'):
    """Keep the leader unreaped until its owned process group has been killed."""
    result = {'exit_code': None, 'signal': None, 'category': 'completed',
              'pid': None, 'start': None, 'end': None, 'elapsed': None,
              'stdout': b'', 'stderr': b''}
    child = None
    can_signal = True
    expected_exit = None
    observation_stage = 'waiting'
    with tempfile.TemporaryFile(dir=root) as out, tempfile.TemporaryFile(dir=root) as err:
        try:
            result['start'] = datetime.now(timezone.utc)
            began = time.monotonic()
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL,
                stdout=out if capture_stdout else subprocess.DEVNULL, stderr=err,
                close_fds=True, env=base.ENV, start_new_session=True)
            result['pid'] = child.pid
            while True:
                if os.fstat(out.fileno()).st_size > stdout_cap or os.fstat(err.fileno()).st_size > stderr_cap:
                    result['category'] = 'output_limit'
                    break
                if time.monotonic() - began >= timeout:
                    result['category'] = 'timeout'
                    break
                try:
                    status = observe(child)
                    if status is not None:
                        observation_stage = 'exit_observed_unconfirmed'
                        # A returned exit is not proof WNOWAIT was honoured.
                        # Withhold group signaling until the same zombie can
                        # be observed a second time without consuming it.
                        can_signal = False
                        expected_exit = observed_exit(child, status)
                        repeated = observe(child)
                        if repeated is None or observation_key(repeated) != observation_key(status):
                            raise ValueError('observation_not_repeatable')
                        observed_exit(child, repeated)
                        can_signal = True
                        observation_stage = 'exit_confirmed'
                except OSError as error:
                    observation_stage = 'observation_failed'
                    if error.errno == errno.ECHILD:
                        can_signal = False
                    result['category'] = 'observation_failed'
                    break
                except (AttributeError, NotImplementedError, ValueError):
                    observation_stage = 'observation_failed'
                    result['category'] = 'observation_failed'
                    break
                if status is not None:
                    break
                time.sleep(0.025)
        except OSError:
            result['category'] = 'launch_failed'
        finally:
            if child is not None:
                # Never poll/wait before this signal; PID cannot be reused while unreaped.
                signal_error = None
                signal_attempted = False
                if can_signal:
                    try:
                        signal_attempted = True
                        os.killpg(child.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    except OSError as error:
                        signal_error = error
                try:
                    code = child.wait(timeout=2)
                except subprocess.TimeoutExpired as error:
                    raise CleanupFailed('leader_wait_timeout', error, observation_stage,
                                        signal_attempted, process) from None
                except (OSError, subprocess.SubprocessError) as error:
                    raise CleanupFailed('leader_wait_failed', error, observation_stage,
                                        signal_attempted, process) from None
                if signal_error is not None:
                    raise CleanupFailed('group_signal_failed', signal_error, observation_stage,
                                        signal_attempted, process) from None
                if not can_signal or expected_exit is not None and code != expected_exit:
                    # Popen's ECHILD fallback can manufacture zero. Never
                    # promote that value, or an observation mismatch, to pass.
                    result['category'] = 'observation_failed'
                else:
                    result['exit_code'] = code
                    result['signal'] = -code if code < 0 else None
            if result['start'] is not None:
                result['end'] = datetime.now(timezone.utc)
                result['elapsed'] = time.monotonic() - began
            if os.fstat(out.fileno()).st_size > stdout_cap or os.fstat(err.fileno()).st_size > stderr_cap:
                result['category'] = 'output_limit'
            out.seek(0)
            err.seek(0)
            result['stdout'] = out.read(stdout_cap + 1)
            result['stderr'] = err.read(stderr_cap + 1)
    return result


def observation_supported(root):
    if not all(hasattr(os, name) for name in ('waitid', 'WNOWAIT', 'WEXITED', 'WNOHANG', 'P_PID')):
        return False
    try:
        result = run_private(['/usr/bin/false'], root, timeout=2, capture_stdout=False, process='preflight')
        return result['category'] == 'completed' and result['exit_code'] == 1
    except (OSError, ValueError, subprocess.SubprocessError):
        return False


def public_status(case_id, private):
    return {'id': case_id, 'exit_code': private['exit_code'], 'signal': private['signal'],
            'timeout': private['category'] == 'timeout', 'category': private['category']}


def query_logs(root, name, pid, window):
    try:
        private = run_private(query_command(name, pid, window), root, process='query')
    except (OSError, ValueError, subprocess.SubprocessError):
        return log_result('query_failed')
    result = parse_denials(private['stdout'], name, pid, window)
    result['query_exit_code'] = private['exit_code']
    result['query_signal'] = private['signal']
    permission = any(token in private['stderr'].lower() for token in
        (b'permission denied', b'operation not permitted', b'insufficient privileges'))
    category = ('query_unavailable' if private['exit_code'] not in (None, 0) and permission else
                'query_output_limit' if private['category'] == 'output_limit' else
                'query_timeout' if private['category'] == 'timeout' else
                'query_failed' if private['category'] != 'completed' or private['exit_code'] != 0 else None)
    if category:
        result.update(category=category, denials=[], matched_count=None)
    return result


def collect(root, report):
    if not observation_supported(root):
        report['failure'] = {'category': 'query_observation_unsupported'}
        return
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        daemon, profile = base.make_fixture(root, listener.getsockname())
        name = 'mp-' + hashlib.sha256(root.name.encode()).hexdigest()[:8]
        probe = daemon._directory / name
        report['profile_sha256'] = hashlib.sha256(profile.read_bytes()).hexdigest()
        report['probe_source_sha256'] = hashlib.sha256(base.SOURCE.read_bytes()).hexdigest()
        report['failure'] = base.build_probe(probe)
        if report['failure'] is not None:
            return
        identity = crash.read_owned_uuid(probe)
        control = run_private([str(probe), 'read', '/dev/null'], root, capture_stdout=False, process='control')
        report['cases'] = [public_status('read_control', control)]
        if not base.succeeded(control):
            report['log']['category'] = 'control_invalid'
            return
        directory = Path.home() / 'Library/Logs/DiagnosticReports'
        before, _ = crash.snapshot(directory, name)
        confined = run_private(daemon._command(probe, 'read', '/dev/null'), root, capture_stdout=False, process='confined')
        report['cases'].append(public_status('read_confined', confined))
        if confined['category'] != 'completed' or confined['signal'] != signal.SIGABRT:
            report['log']['category'] = 'not_sigabrt'
            return
        context = crash.wait_context(directory, name, before, confined['pid'], probe,
                                     base.replacements_for(root), identity)
        matched = (context.get('category') == 'matched' and context.get('executable') == 'probe'
                   and context.get('path_shape', {}).get('uuid_image_match') is True)
        report['crash_attribution'] = context.get('attribution') if matched else 'unavailable'
        if not matched:
            report['log']['category'] = 'owned_crash_unavailable'
            return
        try:
            window = query_window(confined['start'], confined['end'], confined['elapsed'])
        except ValueError:
            report['log']['category'] = 'clock_window_invalid'
            return
        report['log'] = query_logs(root, name, confined['pid'], window)


def diagnose(output):
    report = {'schema': 1, 'driver': 'marmish-owned-denial-v1',
              'platform': platform.system(), 'architecture': platform.machine(),
              'python_minor': str(sys.version_info[0]) + '.' + str(sys.version_info[1])
                  if sys.version_info[0] == 3 and 11 <= sys.version_info[1] <= 20 else 'other',
              'profile_sha256': None, 'probe_source_sha256': None, 'cases': [],
              'crash_attribution': 'unavailable', 'log': log_result(), 'failure': None, 'ok': False}
    refusal = ('python_too_old' if sys.version_info < (3, 11) else
               'darwin_required' if platform.system() != 'Darwin' else
               'ordinary_user_required' if os.geteuid() == 0 else None)
    if refusal:
        report['failure'] = {'category': refusal}
    else:
        owned = None
        try:
            owned = tempfile.TemporaryDirectory(prefix='marmish-denial-', dir='/private/tmp')
            root = Path(owned.name).resolve()
            root.chmod(0o700)
            if root == output.resolve() or root in output.resolve().parents:
                raise ValueError('report_outside_fixture_required')
            collect(root, report)
        except CleanupFailed as error:
            report['failure'] = error.facts
        except (OSError, ValueError, AssertionError, ImportError, subprocess.SubprocessError, KeyboardInterrupt):
            report['failure'] = {'category': 'fixture_failed'}
        finally:
            if owned is not None:
                try:
                    owned.cleanup()
                except OSError as error:
                    report['fixture_cleanup'] = {'reason': 'fixture_cleanup_failed',
                                                 'errno_class': cleanup_errno(error)}
                    if report['failure'] is None:
                        report['failure'] = {'category': 'cleanup_failed'}
        if report['failure'] is not None:
            report['ok'] = False
    base.write_report(output, report)
    return 0 if report['ok'] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args(argv)
    previous = signal.signal(signal.SIGTERM, terminate)
    try:
        return diagnose(args.output.absolute())
    except (OSError, ValueError):
        print('marmish denial diagnostic: report_or_cleanup_failed', file=sys.stderr)
        return 1
    finally:
        signal.signal(signal.SIGTERM, previous)


if __name__ == '__main__':
    sys.exit(main())
