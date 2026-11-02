"""Bounded installed CLI/server acceptance driver; never loads source app roles."""
from __future__ import annotations

import contextlib
import hashlib
import math
import os
import re
from pathlib import Path
import secrets
import selectors
import signal
import subprocess
import sys
import time

from marmish_installed_release import ROLES, ROLE_TARGETS, RELAYS, canonical_bytes
from marmish_installed_state import (
    _clean_linux, _exited, _file_bytes, _finish_child, _new_json, _observe_linux,
    _validate_state, _mac_control_evidence, child_environment, clock_sample, elapsed, read_private,
    run_bounded, write_private,
)
from marmish_platform import continuous_time, process_identity
from marmish_protocol import EngineBridge, b64, load_json, remove_engine_state, save_json
from marmish_rpc import MdkRpc, RpcError, strict_json
from mdk_smoke import SmokeError, require

CLIENT_SLOT_KEYS = {'context', 'invocation', 'probe_invocation', 'client', 'server',
                    'status', 'category', 'payload', 'exit', 'verification', 'metrics'}
SERVER_SLOT_KEYS = {'context', 'invocation', 'probe_invocation', 'client', 'server',
                    'status', 'category', 'proof', 'receipt', 'receipt_sha256'}
METRIC_KEYS = {'cold_session_status_seconds', 'command_wall_seconds',
               'command_first_output_seconds', 'output_drain_seconds',
               'effective_download_bytes_per_second', 'output_drain_bytes_per_second',
               'stdin_bytes', 'stdout_bytes', 'stderr_bytes'}
BASE_KEYS = {'format', 'kind', 'run', 'attempt', 'sha', 'pin_sha256', 'role', 'nonce',
             'installation', 'relays', 'identity_ready_sha256', 'server_ready_sha256', 'slots', 'timing'}
CLIENT_KEYS = BASE_KEYS | {'client'}
SERVER_KEYS = BASE_KEYS | {'server', 'policy_sha256', 'fixtures', 'expected_principals',
                           'reason', 'unexpected_submissions', 'lifetime', 'cleanup'}
CATEGORIES = {'probe_failed', 'command_failed', 'command_timeout', 'output_mismatch',
              'clock_invalid', 'deadline', 'native_failed', 'setup_failed', 'observer_failed',
              'receipt_failed', 'client_failed', 'client_missing', 'cleanup_incomplete',
              'internal_failure'}
CLIENT_STEP_SECONDS = 1080
CLIENT_WATCHDOG_SECONDS = 8
# Reserved-leader escalation (11s), Linux descendant cleanup (5s), then
# client-only recovery/lock proof (12s; Darwin recovers at most two identities).
CLIENT_TEARDOWN_SECONDS = 28
CLIENT_WRITE_SECONDS = 10
DELIVERY_RESERVE_SECONDS = 120


def client_deadline(now, server_deadline, overall_deadline):
    margin = CLIENT_WATCHDOG_SECONDS + CLIENT_TEARDOWN_SECONDS + CLIENT_WRITE_SECONDS
    return min(now + CLIENT_STEP_SECONDS - margin,
               server_deadline - DELIVERY_RESERVE_SECONDS - margin,
               overall_deadline - 150 - margin)


def _client_cleanup(state, previous_records):
    """Recover only this client, leaving the concurrently serving role intact."""
    from marmish_installed_state import _registry_records, _process_absent
    from marmish_transport import acquire_profile_home_lock
    records = [row for row in _registry_records(state, 'child-')
               if row['identity'] not in previous_records]
    require(all(_process_absent(row['identity']) for row in records), 'cleanup_incomplete')
    home = Path(state['root']) / 'client/mdk'
    if sys.platform.startswith('linux'):
        for row in records:
            _clean_linux(state, row['execution_id'])
    elif sys.platform == 'darwin':
        from marmish_transport_macos import (DarwinConfinedDaemon, control_dir,
                                             _open_control, _lock_control, cleanup_control)
        if os.path.lexists(control_dir(home)):
            daemon = DarwinConfinedDaemon(home, '/unused', RELAYS, '0' * 64)
            descriptor = _open_control(home)
            lock = None
            try:
                lock = _lock_control(descriptor)
                daemon._control_fd = descriptor
                daemon._recover()
            finally:
                if lock is not None:
                    os.close(lock)
                os.close(descriptor)
            cleanup_control(home)
    else:
        raise SmokeError('cleanup_incomplete')
    if home.exists():
        with acquire_profile_home_lock(home):
            pass


def digest(value):
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def pair_context(identities, client_role, server_role):
    from marmish_installed_fixture import derive_pair
    client, server = identities[client_role], identities[server_role]
    return derive_pair({key: client[key] for key in ('run', 'attempt', 'sha')},
                       client['pin_sha256'], client, server)


def _record_base(state, identities, kind, servers=None):
    local = identities[state['role']]
    return {'format': 1, 'kind': kind, **state['ctx'], 'pin_sha256': state['pin_sha256'],
            'role': state['role'], 'nonce': local['nonce'], 'installation': local['installation'],
            'relays': list(RELAYS),
            'identity_ready_sha256': {role: digest(identities[role]) for role in ROLES},
            'server_ready_sha256': {role: digest(servers[role]) for role in ROLES} if servers else {},
            'slots': {}, 'timing': None}


def payload_record(stdin, stdout, stderr):
    return {name: {'bytes': len(value), 'sha256': hashlib.sha256(value).hexdigest()}
            for name, value in (('stdin', stdin), ('stdout', stdout), ('stderr', stderr))}


def command_metrics(probe, command, payload):
    """Inclusive local timings, one observation per pair, seconds and bytes."""
    require(probe['timing']['clock_valid'] and command['timing']['clock_valid'], 'clock_invalid')
    cold = probe['timing']['elapsed_ns'] / 1e9
    wall = command['timing']['elapsed_ns'] / 1e9
    first, last = command.get('first_output_ns'), command.get('last_output_ns')
    require(type(first) is int and type(last) is int and 0 <= first <= last <=
            command['timing']['elapsed_ns'], 'clock_invalid')
    drain = (last - first) / 1e9
    total = payload['stdout']['bytes'] + payload['stderr']['bytes']
    return {'cold_session_status_seconds': cold, 'command_wall_seconds': wall,
            'command_first_output_seconds': first / 1e9,
            'output_drain_seconds': drain,
            'effective_download_bytes_per_second': total / wall if wall > 0 else None,
            'output_drain_bytes_per_second': total / drain if drain > 0 else None,
            **{name + '_bytes': payload[name]['bytes'] for name in ('stdin', 'stdout', 'stderr')}}


def _common(state, name):
    args = ['--key-storage', 'files', '-i', str(Path(state['root']) / name)]
    for relay in RELAYS:
        args += ['--relay', relay]
    return args


def _pair_spec(state, identities, servers, server_role):
    from marmish_installed_fixture import command_argv, expected_streams, stdin_bytes
    pair = pair_context(identities, state['role'], server_role)
    path = servers[server_role]['fixtures'][state['role']]['path']
    command = command_argv(path, pair['challenge'], pair['invocation'])
    stdin = stdin_bytes(pair['context'])
    stdout, stderr = expected_streams(path, pair['context'], pair['challenge'], pair['invocation'])
    return pair, command, stdin, stdout, stderr


def run_client_pair(state, identities, servers, server_role, *, deadline):
    from marmish_installed_state import _registry_records
    previous_records = [row['identity'] for row in _registry_records(state, 'child-')]
    pair, command, stdin, stdout, stderr = _pair_spec(state, identities, servers, server_role)
    slot = {key: pair[key] for key in ('context', 'invocation', 'probe_invocation')}
    slot.update(client=identities[state['role']]['client'], server=identities[server_role]['server'],
                status='failed', category='probe_failed', payload=payload_record(stdin, stdout, stderr),
                exit=None, verification=dict.fromkeys(('probe', 'stdout', 'stderr', 'exit'), False), metrics=None)
    cli = [str(Path(state['prefix']) / 'bin/marmish'), *_common(state, 'client'),
           '-u', slot['server']]
    def invoke(arguments, input_bytes=b''):
        remaining = min(300, deadline - continuous_time())
        require(remaining >= 1, 'deadline')
        return run_bounded(cli + ['--timeout', str(int(remaining))] + arguments, state,
                           timeout=remaining + CLIENT_WATCHDOG_SECONDS, output_limit=256 * 1024,
                           runtime=True, stdin=input_bytes)
    try:
        probe = invoke(['--status', pair['probe_invocation']])
        if probe['timed_out']:
            slot['status'] = 'timed_out'
        require(probe['returncode'] == 125 and probe['stdout'] == b'' and
                probe['stderr'] == b'marmish: not_found\n' and not probe['timed_out'] and
                not probe['output_limited'] and probe['cleanup'] == 'complete', 'probe_failed')
        slot['verification']['probe'] = True
        slot['category'] = 'command_failed'
        result = invoke(['--invocation', pair['invocation'], '--', *command], stdin)
        slot['exit'] = result['returncode']
        slot['verification'].update(stdout=result['stdout'] == stdout,
                                    stderr=result['stderr'] == stderr,
                                    exit=result['returncode'] == 7)
        require(not result['timed_out'], 'command_timeout')
        require(not result['output_limited'] and result['cleanup'] == 'complete', 'command_failed')
        require(all(slot['verification'].values()), 'output_mismatch')
        slot['metrics'] = command_metrics(probe, result, slot['payload'])
        slot.update(status='received', category=None)
    except (RpcError, SmokeError, OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
        category = str(error)
        slot['category'] = category if category in CATEGORIES else 'native_failed'
        if category in ('command_timeout', 'deadline'):
            slot['status'] = 'timed_out'
    if slot['status'] != 'received':
        try:
            _client_cleanup(state, previous_records)
        except (RpcError, SmokeError, OSError, RuntimeError, ValueError, KeyError, TypeError):
            slot.update(status='failed', category='cleanup_incomplete', metrics=None)
    return slot


def run_clients(state, identities, servers, *, deadline=None):
    _validate_state(state)
    started = clock_sample()
    report = _record_base(state, identities, 'client-result', servers)
    report['client'] = identities[state['role']]['client']
    deadline = client_deadline(continuous_time(),
        deadline if deadline is not None else float('inf'), state['overall_deadline'])
    for role in ROLES:
        report['slots'][role] = run_client_pair(state, identities, servers, role,
            deadline=min(deadline, continuous_time() + 300))
        if report['slots'][role]['category'] == 'cleanup_incomplete':
            # Reusing the profile after uncertain cleanup would race its daemon.
            break
    for role in ROLES:
        if role not in report['slots']:
            pair, _argv, stdin, out, err = _pair_spec(state, identities, servers, role)
            report['slots'][role] = {key: pair[key] for key in ('context', 'invocation', 'probe_invocation')}
            report['slots'][role].update(client=report['client'], server=identities[role]['server'],
                status='skipped', category='cleanup_incomplete', payload=payload_record(stdin, out, err),
                exit=None, verification=dict.fromkeys(('probe', 'stdout', 'stderr', 'exit'), False), metrics=None)
    report['timing'] = elapsed(started, clock_sample())
    validate_client_report(report)
    write_private(Path(state['output']) / 'client-result.json', report)
    return report


def failure_client_report(state, identities, servers=None, *, category='native_failed'):
    require(category in CATEGORIES, 'invalid_terminal_report')
    started = clock_sample()
    report = _record_base(state, identities, 'client-result', servers)
    report['client'] = identities[state['role']]['client']
    for role in ROLES:
        pair = pair_context(identities, state['role'], role)
        payload = None
        if servers:
            _pair, _argv, stdin, out, err = _pair_spec(state, identities, servers, role)
            payload = payload_record(stdin, out, err)
        report['slots'][role] = {key: pair[key] for key in
                                  ('context', 'invocation', 'probe_invocation')}
        report['slots'][role].update(client=report['client'], server=identities[role]['server'],
            status='failed', category=category, payload=payload, exit=None, metrics=None,
            verification=dict.fromkeys(('probe', 'stdout', 'stderr', 'exit'), False))
    report['timing'] = elapsed(started, clock_sample())
    validate_client_report(report)
    return report


def command_policy(fixtures, identities):
    return {'version': 1,
            'defaults': {'allow_exec': [fixtures[role]['path'] for role in ROLES]},
            'keys': [{'principal': identities[role]['client'],
                      'restrictions': {'allow_exec': [fixtures[role]['path']]}} for role in ROLES]}


def _engine_policy(state, identities, policy, *, scratch=False):
    root = Path(state['root'])
    installed = (Path(state['prefix']) / 'current').resolve(strict=True)
    return {'version': 1, 'state_root': str(root / ('policy-engine' if scratch else 'server/engine')),
            'worker_executable': str(installed / 'libexec/marmish/marmish-worker'),
            'server': identities[state['role']]['server'],
            'allowed_principals': sorted(identities[role]['client'] for role in ROLES),
            'cwd': str(root / 'cwd'), 'allow_shell_command': False, 'command_policy': policy}


@contextlib.contextmanager
def _registered_bridge(state, executable, policy):
    # EngineBridge intentionally passes only PATH/LANG and this public marker.
    # Capture ownership immediately after its bounded init, before any submit.
    previous = os.environ.get('EPIPHYTIC_EXECUTION_ID')
    os.environ['EPIPHYTIC_EXECUTION_ID'] = state['execution_id']
    try:
        bridge = EngineBridge(executable, policy)
    finally:
        if previous is None:
            os.environ.pop('EPIPHYTIC_EXECUTION_ID', None)
        else:
            os.environ['EPIPHYTIC_EXECUTION_ID'] = previous
    token = secrets.token_hex(16)
    path = Path(state['registry']) / ('child-' + token + '.json')
    try:
        record = {'identity': process_identity(bridge.child.pid), 'runtime': False,
                  'finished': False, 'cleanup': None, 'execution_id': state['execution_id']}
        _new_json(path, record)
    except BaseException:
        bridge.close()
        raise
    try:
        yield bridge
    finally:
        bridge.close()
        record.update(finished=bridge.child.returncode is not None,
                      cleanup='complete' if bridge.child.returncode is not None else 'incomplete',
                      returncode=bridge.child.returncode, timed_out=False, output_limited=False)
        save_json(path, record)


def check_policy(state, identities, fixtures, policy, *, deadline):
    """Exercise the actual installed bridge's negative authorization operations."""
    executable = (Path(state['prefix']) / 'current/libexec/marmish/marmish-engine-bridge').resolve(strict=True)
    engine_policy = _engine_policy(state, identities, policy, scratch=True)
    write_private(Path(state['root']) / 'scratch-policy.json', engine_policy)
    count = 0
    with _registered_bridge(state, executable, engine_policy) as bridge:
        for role in ROLES:
            principal = identities[role]['client']
            endpoint = hashlib.sha256(('policy:' + role).encode()).hexdigest()
            token = bridge.call('token', principal=principal, endpoint=endpoint)['token']
            for other in ROLES:
                if other == role:
                    continue
                require(continuous_time() < deadline, 'deadline')
                try:
                    bridge.call('submit', principal=principal, endpoint=endpoint,
                        invocation=secrets.token_hex(16), token=token,
                        command={'mode': 'exec', 'argv': [b64(os.fsencode(fixtures[other]['path']))]})
                except SmokeError as error:
                    require(str(error) == 'command_denied', 'policy_check_failed')
                else:
                    raise SmokeError('policy_check_failed')
                count += 1
        unlisted = hashlib.sha256(b'marmish-installed-unlisted' + os.urandom(32)).hexdigest()
        require(unlisted not in engine_policy['allowed_principals'], 'policy_check_failed')
        try:
            bridge.call('token', principal=unlisted, endpoint='0' * 64)
        except SmokeError as error:
            require(str(error) == 'unauthorized', 'policy_check_failed')
        else:
            raise SmokeError('policy_check_failed')
        require(count == 6 and not any(Path(row['receipt']).exists() for row in fixtures.values()),
                'policy_check_failed')
        require(bridge.call('cleanup_ready') == {'ready': True}, 'cleanup_incomplete')
        # Guard remains held while deleting, as in remove_engine_state.
        import shutil
        shutil.rmtree(engine_policy['state_root'])
    return {'denied_submissions': count, 'unlisted_denied': True, 'cleanup_ready': True}


def _hex(value, width=64):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{' + str(width) + '}', value) is not None


def _closed(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys), 'invalid_terminal_report')


def _uint(value, maximum=4_620_000_000_000):
    return type(value) is int and 0 <= value <= maximum


def _validate_timing(value):
    _closed(value, {'elapsed_ns', 'active_ns', 'suspended_ns', 'clock_valid'})
    require(all(_uint(value[key]) for key in ('elapsed_ns', 'active_ns', 'suspended_ns')) and
            type(value['clock_valid']) is bool and value['suspended_ns'] ==
            max(0, value['elapsed_ns'] - value['active_ns']) and
            (not value['clock_valid'] or value['suspended_ns'] <= 5_000_000_000),
            'invalid_terminal_report')


def _validate_base(report, kind, keys):
    _closed(report, keys)
    require(type(report['format']) is int and report['format'] == 1 and
            report['kind'] == kind and report['role'] in ROLES and
            all(isinstance(report[key], str) and re.fullmatch('[1-9][0-9]*', report[key])
                for key in ('run', 'attempt')) and _hex(report['sha'], 40) and
            _hex(report['pin_sha256']) and _hex(report['nonce']), 'invalid_terminal_report')
    _closed(report['slots'], ROLES)
    require(report['relays'] == list(RELAYS), 'invalid_terminal_report')
    for name in ('identity_ready_sha256', 'server_ready_sha256'):
        values = report[name]
        require(isinstance(values, dict) and (set(values) == set(ROLES) or
                name == 'server_ready_sha256' and not values) and
                all(_hex(value) for value in values.values()), 'invalid_terminal_report')
    installation = report['installation']
    _closed(installation, {'target', 'version', 'source_commit', 'archive_sha256',
                           'manifest_sha256', 'installer_sha256'})
    require(installation['target'] == ROLE_TARGETS[report['role']] and
            isinstance(installation['version'], str) and 0 < len(installation['version']) <= 128 and
            _hex(installation['source_commit'], 40) and
            all(_hex(installation[key]) for key in ('archive_sha256', 'manifest_sha256',
                                                    'installer_sha256')),
            'invalid_terminal_report')
    _validate_timing(report['timing'])
    return report


def _validate_slot(slot, keys):
    _closed(slot, keys)
    require(_hex(slot['context']) and _hex(slot['invocation'], 32) and
            _hex(slot['probe_invocation'], 32) and _hex(slot['client']) and _hex(slot['server']) and
            (slot['category'] is None or slot['category'] in CATEGORIES), 'invalid_terminal_report')


def _validate_payload(payload):
    _closed(payload, {'stdin', 'stdout', 'stderr'})
    for stream in payload.values():
        _closed(stream, {'bytes', 'sha256'})
        require(_uint(stream['bytes'], 4 * 1024 * 1024) and _hex(stream['sha256']),
                'invalid_terminal_report')


def _validate_metrics(metrics, payload):
    _closed(metrics, METRIC_KEYS)
    for name, value in metrics.items():
        nullable = name in ('effective_download_bytes_per_second', 'output_drain_bytes_per_second')
        require(value is None and nullable or type(value) in (int, float) and
                math.isfinite(value) and value >= 0, 'invalid_terminal_report')
    for stream in ('stdin', 'stdout', 'stderr'):
        require(type(metrics[stream + '_bytes']) is int and
                metrics[stream + '_bytes'] == payload[stream]['bytes'], 'invalid_terminal_report')
    wall, first, drain = (metrics[key] for key in
        ('command_wall_seconds', 'command_first_output_seconds', 'output_drain_seconds'))
    require(first <= wall and drain <= wall - first + 1e-9, 'invalid_terminal_report')
    total = payload['stdout']['bytes'] + payload['stderr']['bytes']
    for key, duration in [('effective_download_bytes_per_second', wall),
                           ('output_drain_bytes_per_second', drain)]:
        rate = metrics[key]
        require(rate is None if duration == 0 else rate is not None and
                math.isclose(rate, total / duration, rel_tol=1e-9, abs_tol=1e-9),
                'invalid_terminal_report')


def validate_client_report(report):
    _validate_base(report, 'client-result', CLIENT_KEYS)
    require(_hex(report['client']), 'invalid_terminal_report')
    success = False
    for slot in report['slots'].values():
        _validate_slot(slot, CLIENT_SLOT_KEYS)
        require(slot['client'] == report['client'] and
                slot['status'] in ('received', 'failed', 'timed_out', 'skipped') and
                (slot['exit'] is None or type(slot['exit']) is int and -255 <= slot['exit'] <= 255),
                'invalid_terminal_report')
        _closed(slot['verification'], {'probe', 'stdout', 'stderr', 'exit'})
        require(all(type(value) is bool for value in slot['verification'].values()),
                'invalid_terminal_report')
        require(slot['payload'] is not None or slot['status'] != 'received', 'invalid_terminal_report')
        if slot['payload'] is not None:
            _validate_payload(slot['payload'])
        if slot['status'] == 'received':
            success = True
            require(slot['category'] is None and slot['exit'] == 7 and
                    all(slot['verification'].values()), 'invalid_terminal_report')
            _validate_metrics(slot['metrics'], slot['payload'])
        else:
            require(slot['category'] in CATEGORIES and slot['metrics'] is None,
                    'invalid_terminal_report')
    require(not success or set(report['server_ready_sha256']) == set(ROLES), 'invalid_terminal_report')
    return report


def validate_server_report(report):
    _validate_base(report, 'server-result', SERVER_KEYS)
    require(_hex(report['server']) and (report['policy_sha256'] is None or
            _hex(report['policy_sha256'])) and report['reason'] in ('all-client-results',
            'lifetime-cap', 'setup-failed', 'observer-failed', 'native-failed') and
            _uint(report['unexpected_submissions'], 4096) and report['cleanup'] == 'provisional',
            'invalid_terminal_report')
    _closed(report['expected_principals'], ROLES)
    require(all(_hex(value) for value in report['expected_principals'].values()) and
            len(set(report['expected_principals'].values())) == 3, 'invalid_terminal_report')
    _closed(report['lifetime'], {'limit_seconds', 'elapsed_seconds', 'clock_valid'})
    lifetime = report['lifetime']
    require(type(lifetime['limit_seconds']) is int and lifetime['limit_seconds'] == 1800 and
            type(lifetime['elapsed_seconds']) in (int, float) and
            math.isfinite(lifetime['elapsed_seconds']) and lifetime['elapsed_seconds'] >= 0 and
            type(lifetime['clock_valid']) is bool, 'invalid_terminal_report')
    require(isinstance(report['fixtures'], dict) and set(report['fixtures']) in (set(), set(ROLES)),
            'invalid_terminal_report')
    for fixture in report['fixtures'].values():
        _closed(fixture, {'path', 'sha256', 'provenance'})
        provenance = fixture['provenance']
        _closed(provenance, {'harness_sha', 'source_sha256', 'compiler', 'binary_sha256'})
        require(isinstance(fixture['path'], str) and len(fixture['path']) < 256 and
                _hex(fixture['sha256']) and _hex(provenance['harness_sha'], 40) and
                _hex(provenance['source_sha256']) and _hex(provenance['binary_sha256']) and
                isinstance(provenance['compiler'], str) and 0 < len(provenance['compiler']) <= 1024,
                'invalid_terminal_report')
    success = False
    for role, slot in report['slots'].items():
        _validate_slot(slot, SERVER_SLOT_KEYS)
        require(slot['server'] == report['server'] and
                slot['client'] == report['expected_principals'][role] and
                slot['status'] in ('completed', 'failed'), 'invalid_terminal_report')
        if slot['status'] == 'completed':
            success = True
            require(slot['category'] is None and isinstance(slot['proof'], dict) and
                    isinstance(slot['receipt'], dict) and
                    slot['receipt_sha256'] == digest(slot['receipt']), 'invalid_terminal_report')
        else:
            require(slot['category'] in CATEGORIES and slot['proof'] is None and
                    slot['receipt'] is None and slot['receipt_sha256'] is None,
                    'invalid_terminal_report')
    require(not success or set(report['fixtures']) == set(ROLES) and
            set(report['server_ready_sha256']) == set(ROLES) and _hex(report['policy_sha256']),
            'invalid_terminal_report')
    return report


class OwnedServer:
    """Nonblocking daemon reader with reserved-leader cleanup from phase A."""
    def __init__(self, state, argv):
        self.state = state
        self.argv = argv
        self.control_verified = False
        self.token = secrets.token_hex(16)
        self.marker = state['execution_id'] + ':' + self.token
        env = child_environment(state, runtime=True)
        env['EPIPHYTIC_EXECUTION_ID'] = self.marker
        self.child = subprocess.Popen(argv, cwd=state['home'], env=env,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            close_fds=True, start_new_session=True)
        try:
            self.identity = process_identity(self.child.pid)
        except BaseException:
            os.killpg(self.child.pid, signal.SIGKILL)
            self.child.wait(timeout=2)
            self.child.stdout.close()
            self.child.stderr.close()
            raise
        self.record = {'identity': self.identity, 'runtime': True, 'finished': False,
                       'cleanup': None, 'execution_id': self.marker}
        self.record_path = Path(state['registry']) / ('child-' + self.token + '.json')
        _new_json(self.record_path, self.record)
        self.selector = selectors.DefaultSelector()
        self.buffers = {'stdout': bytearray(), 'stderr': bytearray()}
        for name in self.buffers:
            pipe = getattr(self.child, name)
            os.set_blocking(pipe.fileno(), False)
            self.selector.register(pipe, selectors.EVENT_READ, name)
        self.last_scan = continuous_time()

    def tick(self, *, stopping=False):
        require(stopping or not _exited(self.child), 'native_failed')
        for key, _events in self.selector.select(.05):
            data = os.read(key.fd, 16384)
            if not data:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()
                continue
            require(sum(map(len, self.buffers.values())) + len(data) <= 128 * 1024,
                    'native_failed')
            self.buffers[key.data].extend(data)
        if sys.platform == 'darwin':
            self.control_verified = _mac_control_evidence(self.state, self.argv) or self.control_verified
        if sys.platform.startswith('linux') and continuous_time() - self.last_scan >= .5:
            _observe_linux(self.state, self.marker)
            self.last_scan = continuous_time()

    def ready(self, account):
        if b'\n' not in self.buffers['stdout']:
            return False
        row = strict_json(bytes(self.buffers['stdout']).split(b'\n', 1)[0])
        require(row == {'ready': True, 'account': account} and type(row['ready']) is bool,
                'native_failed')
        return True

    def close(self):
        cleanup = 'incomplete'
        try:
            from marmish_installed_state import _signal_group
            _signal_group(self.child, self.identity, signal.SIGINT)
            until = continuous_time() + 5
            while not _exited(self.child) and continuous_time() < until:
                try:
                    self.tick(stopping=True)
                except (RpcError, SmokeError, OSError, RuntimeError, ValueError):
                    break
            _finish_child(self.child, self.identity, interrupted=False, runtime=True)
            if sys.platform.startswith('linux'):
                _clean_linux(self.state, self.marker)
            cleanup = 'complete'
        finally:
            for pipe in (self.child.stdout, self.child.stderr):
                pipe.close()
            self.selector.close()
            self.record.update(finished=self.child.returncode is not None, cleanup=cleanup,
                returncode=self.child.returncode, timed_out=False, output_limited=False)
            if sys.platform == 'darwin':
                self.record['control_verified'] = self.control_verified
            save_json(self.record_path, self.record)
        require(cleanup == 'complete', 'cleanup_incomplete')


def _running_rpc(state):
    home = Path(state['root']) / 'server/mdk'
    options = {}
    if sys.platform == 'darwin':
        from marmish_transport_macos import read_running_identity
        options = {'rpc_home': str(home), 'rpc_socket': str(home / 's'),
                   'peer_identity': read_running_identity(home)}
    return MdkRpc(home / 's', timeout=2, **options)


def _server_slot(identities, client_role, server_role):
    pair = pair_context(identities, client_role, server_role)
    return {key: pair[key] for key in ('context', 'invocation', 'probe_invocation')} | {
        'client': identities[client_role]['client'], 'server': identities[server_role]['server'],
        'status': 'failed', 'category': 'client_missing', 'proof': None,
        'receipt': None, 'receipt_sha256': None}


def _read_clients(state, identities):
    inbox = Path(state['output']) / 'client-inbox'
    results = {}
    for role in ROLES:
        path = inbox / (role + '.json')
        if os.path.lexists(path):
            report = validate_client_report(strict_json(read_private(path, 65536)))
            require(report['role'] == role and report['client'] == identities[role]['client'] and
                    report['identity_ready_sha256'] == {key: digest(identities[key]) for key in ROLES} and
                    all(report[key] == state['ctx'][key] for key in ('run', 'attempt', 'sha')) and
                    report['pin_sha256'] == state['pin_sha256'], 'invalid_terminal_report')
            results[role] = report
    return results


def _reconcile_server(state, identities, fixtures, observer, clients, report):
    from marmish_installed_fixture import expected_receipt
    root = Path(state['root'])
    expected_paths = {Path(fixtures[role]['receipt']) for role in ROLES}
    actual = set((root / 'r').iterdir()) if (root / 'r').exists() else set()
    for role in ROLES:
        slot = report['slots'][role]
        try:
            require(role in clients and clients[role]['slots'][state['role']]['status'] == 'received',
                    'client_failed')
            pair = pair_context(identities, role, state['role'])
            path = Path(fixtures[role]['receipt'])
            raw = read_private(path, 8192)
            receipt = strict_json(raw)
            require(receipt == expected_receipt(role, pair['context'], pair['challenge'],
                        pair['invocation'], fixtures[role]['path']), 'receipt_failed')
            proof = observer.proof_for(identities[role]['client'])
            slot.update(status='completed', category=None, proof=proof,
                        receipt=receipt, receipt_sha256=digest(receipt))
            require(slot['receipt_sha256'] == digest(receipt), 'receipt_failed')
        except (RpcError, SmokeError, OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
            category = str(error)
            slot['category'] = category if category in CATEGORIES else 'observer_failed'
    # Unknown receipts fail the report without erasing independently verified cells.
    require(actual <= expected_paths, 'receipt_failed')


def _observer_specs(identities, server_role, fixtures):
    from marmish_installed_fixture import command_argv, expected_streams, stdin_bytes
    expected = {}
    for role in ROLES:
        pair = pair_context(identities, role, server_role)
        path = fixtures[role]['path']
        out, err = expected_streams(path, pair['context'], pair['challenge'], pair['invocation'])
        expected[identities[role]['client']] = {
            'invocation': pair['invocation'], 'probe_invocation': pair['probe_invocation'],
            'command': {'mode': 'exec', 'argv': [b64(arg) for arg in command_argv(
                path, pair['challenge'], pair['invocation'])]},
            'stdin': stdin_bytes(pair['context']), 'stdout': out, 'stderr': err}
    return expected


def _publish_server_ready(state, identities, fixtures, identity_digests):
    now = int(time.time())
    ready = dict(identities[state['role']], kind='server-ready', ready=True,
        identity_ready_sha256=identity_digests,
        fixtures={role: {key: fixtures[role][key] for key in ('path', 'sha256')} for role in ROLES},
        server_started_at=now, published_at=now, expires_at=now + 3600)
    ready_dir = Path(state['output']) / 'server-ready'
    ready_dir.mkdir(mode=0o700)
    write_private(ready_dir / 'record.json', ready)
    (Path(state['output']) / 'client-inbox').mkdir(mode=0o700, exist_ok=True)


def serve(state, identities, *, arm_lifetime=False, lifecycle=None):
    """Foreground supervisor; launcher/Actions run it with scrubbed credentials."""
    from marmish_installed_fixture import build_fixture
    from marmish_installed_observer import DaemonObserver
    _validate_state(state)
    started = clock_sample()
    began = started['continuous_ns'] / 1e9
    deadline = min(began + 1800, state['overall_deadline'] - 150)
    readiness_deadline = min(began + 300, deadline - 1200)
    write_private(Path(state['root']) / 'live-start.json', {
        'start': started, 'deadline': deadline, 'readiness_deadline': readiness_deadline})
    report = _record_base(state, identities, 'server-result')
    report.update(server=identities[state['role']]['server'], policy_sha256=None, fixtures={},
        expected_principals={role: identities[role]['client'] for role in ROLES},
        slots={role: _server_slot(identities, role, state['role']) for role in ROLES},
        reason='setup-failed', unexpected_submissions=0,
        lifetime={'limit_seconds': 1800, 'elapsed_seconds': 0, 'clock_valid': True},
        cleanup='provisional')
    daemon = None
    observer = None
    fixtures = {}
    if lifecycle is not None:
        lifecycle['cleanup'] = 'incomplete'
    if arm_lifetime:
        signal.setitimer(signal.ITIMER_REAL, max(.001, deadline - continuous_time() + 1))
    try:
        root = Path(state['root'])
        (root / 'cwd').mkdir(mode=0o700)
        for role in ROLES:
            fixtures[role] = build_fixture(state, role,
                pair_context(identities, role, state['role']), deadline=readiness_deadline)
        policy = command_policy(fixtures, identities)
        policy_path = root / 'command-policy.json'
        write_private(policy_path, policy)
        check_policy(state, identities, fixtures, policy, deadline=readiness_deadline)
        report['policy_sha256'] = digest(policy)
        report['fixtures'] = {role: {key: fixtures[role][key] for key in ('path', 'sha256', 'provenance')}
                              for role in ROLES}
        write_private(root / 'runtime-policy.json', _engine_policy(state, identities, policy))
        args = [str(Path(state['prefix']) / 'bin/marmishd'), *_common(state, 'server'),
                '--cwd', str(root / 'cwd'), '--command-policy', str(policy_path)]
        for role in ROLES:
            args += ['--allow', identities[role]['client']]
        daemon = OwnedServer(state, args)
        while not daemon.ready(report['server']):
            require(continuous_time() < readiness_deadline, 'deadline')
            daemon.tick()
        observer = DaemonObserver(_running_rpc(state), account=report['server'],
            expected=_observer_specs(identities, state['role'], fixtures),
            profile_path=root / 'server/profile.json')
        observer.tick()
        _publish_server_ready(state, identities, fixtures, report['identity_ready_sha256'])
        # Server-ready records are delivered by the same bounded adapter as clients.
        while continuous_time() < deadline:
            daemon.tick()
            observer.tick()
            clients = _read_clients(state, identities)
            if len(clients) == 3:
                observer.tick(final=True)
                report['reason'] = 'all-client-results'
                _reconcile_server(state, identities, fixtures, observer, clients, report)
                break
            time.sleep(.25)
        else:
            report['reason'] = 'lifetime-cap'
        server_records = Path(state['root']) / 'server-records.json'
        if server_records.exists():
            all_servers = strict_json(read_private(server_records))
            report['server_ready_sha256'] = {role: digest(all_servers[role]) for role in ROLES}
    except (RpcError, SmokeError, OSError, RuntimeError, ValueError, KeyError, TypeError) as error:
        report['reason'] = ('lifetime-cap' if str(error) == 'deadline' and continuous_time() >= deadline
                            else 'observer-failed' if observer is not None else 'native-failed')
        category = str(error)
        category = category if category in CATEGORIES else 'native_failed'
        for slot in report['slots'].values():
            if slot['status'] != 'completed':
                slot['category'] = category
    finally:
        previous_mask = None
        if arm_lifetime:
            previous_mask = signal.pthread_sigmask(signal.SIG_BLOCK,
                {signal.SIGINT, signal.SIGTERM, signal.SIGALRM})
            signal.setitimer(signal.ITIMER_REAL, 0)
        try:
            frontend_cleanup = 'complete'
            if daemon is not None:
                try:
                    daemon.close()
                except (RpcError, SmokeError, OSError, RuntimeError, ValueError):
                    frontend_cleanup = 'incomplete'
                    report['reason'] = 'native-failed'
                    for slot in report['slots'].values():
                        slot.update(status='failed', category='cleanup_incomplete',
                            proof=None, receipt=None, receipt_sha256=None)
            if not report['server_ready_sha256']:
                with contextlib.suppress(RpcError, SmokeError, OSError, RuntimeError, ValueError, KeyError, TypeError):
                    records = strict_json(read_private(Path(state['root']) / 'server-records.json'))
                    report['server_ready_sha256'] = {role: digest(records[role]) for role in ROLES}
            if observer is not None:
                report['unexpected_submissions'] = sum(pair.unexpected_submissions
                    for pair in observer.pairs.values())
            report['timing'] = elapsed(started, clock_sample())
            report['lifetime'].update(elapsed_seconds=report['timing']['elapsed_ns'] / 1e9,
                                      clock_valid=report['timing']['clock_valid'])
            validate_server_report(report)
            write_private(Path(state['output']) / 'server-result.json', report)
            if lifecycle is not None:
                lifecycle['cleanup'] = frontend_cleanup
        finally:
            if previous_mask is not None:
                signal.pthread_sigmask(signal.SIG_SETMASK, previous_mask)
    return report


def cleanup_runtime(state):
    """After frontend shutdown, fence both real and negative-test engine state."""
    import shutil
    root = Path(state['root'])
    until = min(continuous_time() + 130, state['overall_deadline'] - 35)
    for name, engine in [('scratch-policy.json', 'policy-engine'),
                          ('runtime-policy.json', 'server/engine')]:
        policy_path = root / name
        if not policy_path.exists():
            require(not (root / engine).exists(), 'cleanup_incomplete')
            continue
        policy = strict_json(read_private(policy_path))
        if not (root / engine).exists():
            continue
        executable = (Path(state['prefix']) / 'current/libexec/marmish/marmish-engine-bridge').resolve(strict=True)
        while True:
            try:
                with _registered_bridge(state, executable, policy) as bridge:
                    # Pinned release f609b6's Engine::into_cleanup performs
                    # native PID/start/boot identity absence checks twice,
                    # around acquisition of every job/IO lock. The consuming
                    # bridge guard keeps those locks through state removal.
                    require(bridge.call('cleanup_ready') == {'ready': True}, 'cleanup_incomplete')
                    # Keep the opaque ownership guard held through actual removal.
                    shutil.rmtree(policy['state_root'])
                break
            except SmokeError as error:
                require(str(error) in ('busy', 'already_open') and continuous_time() < until,
                        'cleanup_incomplete')
                time.sleep(.5)
    return {'workers_absent': True, 'engine_cleanup_ready': True}


def launch_server(state, identities):
    """Start one tracked credential-free supervisor, await its bounded readiness."""
    _validate_state(state)
    source = Path(__file__).resolve()
    peers = Path(state['root']) / 'identity-records.json'
    write_private(peers, identities)
    token = secrets.token_hex(16)
    env = child_environment(state, runtime=True)
    marker = state['execution_id'] + ':' + token
    env['EPIPHYTIC_EXECUTION_ID'] = marker
    argv = [state['python'], '-B', str(source), '--serve', '--state', state['state_file'],
            '--identities', str(peers), '--supervisor-token', token]
    child = subprocess.Popen(argv, cwd=state['home'], env=env, stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, close_fds=True, start_new_session=True)
    identity = process_identity(child.pid)
    record_path = Path(state['registry']) / ('child-' + token + '.json')
    _new_json(record_path, {'identity': identity, 'runtime': False, 'finished': False, 'kind': 'supervisor',
                            'cleanup': None, 'execution_id': marker})
    ready = Path(state['output']) / 'server-ready/record.json'
    result = Path(state['output']) / 'server-result.json'
    until = min(continuous_time() + 305, state['overall_deadline'] - 1200)
    while continuous_time() < until:
        if ready.exists():
            require(not _exited(child), 'native_failed')
            return strict_json(read_private(ready, 8192))
        if result.exists() or _exited(child):
            _finish_child(child, identity, interrupted=False, runtime=True)
            raise SmokeError('native_failed')
        time.sleep(.1)
    _finish_child(child, identity, interrupted=True, runtime=True)
    raise SmokeError('deadline')


def stop_supervisor(state):
    """Stop only the exact registered supervisor; no launcher is a safe no-op."""
    from marmish_installed_state import _process_absent, _registry_records, _pidfd_signal
    _validate_state(state)
    supervisors = [row for row in _registry_records(state, 'child-')
                   if row.get('kind') == 'supervisor']
    require(len(supervisors) <= 1, 'invalid_process')
    if not supervisors:
        return
    identity = supervisors[0]['identity']
    # No PGID signals across steps: its original unreaped leader is unavailable.
    for number, grace in [(signal.SIGINT, 8), (signal.SIGTERM, 2), (signal.SIGKILL, 2)]:
        if _process_absent(identity):
            return
        if sys.platform.startswith('linux'):
            _pidfd_signal(identity, number)
        else:
            require(process_identity(identity['pid']) == identity, 'process_identity_changed')
            os.kill(identity['pid'], number)
        until = continuous_time() + grace
        while not _process_absent(identity) and continuous_time() < until:
            time.sleep(.05)
    require(_process_absent(identity), 'cleanup_incomplete')


def await_server(state, *, deadline):
    """Across workflow steps, prove the recorded supervisor identity has exited."""
    from marmish_installed_state import _process_absent, _registry_records
    _validate_state(state)
    supervisors = [row for row in _registry_records(state, 'child-')
                   if row.get('kind') == 'supervisor']
    require(len(supervisors) == 1, 'invalid_process')
    identity = supervisors[0]['identity']
    path = Path(state['output']) / 'server-result.json'
    # The live cap stops acceptance; bounded close/report writing follows it.
    shutdown_deadline = min(deadline + 30, state['overall_deadline'] - 120)
    while continuous_time() < shutdown_deadline:
        if path.exists() and _process_absent(identity):
            return validate_server_report(strict_json(read_private(path, 262144)))
        time.sleep(.1)
    stop_supervisor(state)
    require(path.exists(), 'deadline')
    return validate_server_report(strict_json(read_private(path, 262144)))


def main(argv=None):
    import argparse
    from marmish_installed_state import load_state
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--serve', action='store_true', required=True)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--identities', type=Path, required=True)
    parser.add_argument('--supervisor-token', required=True)
    args = parser.parse_args(argv)
    state = load_state(args.state)
    require(len(args.supervisor_token) == 32 and all(c in '0123456789abcdef'
            for c in args.supervisor_token), 'invalid_process')
    record_path = Path(state['registry']) / ('child-' + args.supervisor_token + '.json')
    # Parent records ownership before this child begins acceptance operations.
    until = continuous_time() + 2
    while not record_path.exists() and continuous_time() < until:
        time.sleep(.01)
    record = strict_json(read_private(record_path))
    require(record['identity'] == process_identity(os.getpid()), 'process_identity_changed')
    identities = strict_json(read_private(args.identities))
    def interrupted(_number, _frame):
        raise SmokeError('deadline')
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    signal.signal(signal.SIGALRM, interrupted)
    lifecycle = {'cleanup': 'incomplete'}
    try:
        serve(state, identities, arm_lifetime=True, lifecycle=lifecycle)
    finally:
        signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM, signal.SIGALRM})
        signal.setitimer(signal.ITIMER_REAL, 0)
        record.update(finished=True, cleanup=lifecycle['cleanup'])
        save_json(record_path, record)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
