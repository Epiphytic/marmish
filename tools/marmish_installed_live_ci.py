"""Finite Actions operations around the credential-free installed live driver."""
import argparse
import os
from pathlib import Path
import time

from marmish_installed_ci import checked_state, envelope, read_record, write_record
from marmish_installed_release import ROLES, canonical_bytes, load_pin
from marmish_installed_rendezvous import context, identity_barrier, pin_digest, validate_server_ready
from marmish_installed_state import (
    cleanup_state, clock_sample, elapsed, load_state, public_directory, read_private, write_private,
)
from marmish_installed_terminal import live_deadline, validate_report
from marmish_platform import continuous_time
from marmish_rpc import strict_json
from mdk_smoke import SmokeError, require

OPERATIONS = ('start-server', 'server-barrier', 'clients', 'await-server', 'cleanup')
ERRORS = frozenset({'artifact_invalid', 'barrier_timeout', 'artifact_network_failed',
                    'artifact_listing_unstable', 'deadline', 'native_failed', 'client_failed',
                    'observer_failed', 'cleanup_incomplete', 'internal_failure', 'terminal_timeout'})


def identities(args, state, pin):
    path = args.identities or Path(state['root']) / 'identity-records.json'
    records = identity_barrier(read_record(path), state['ctx'], pin, now=int(time.time()))
    own = read_record(Path(state['output']) / 'identity-ready/record.json')
    require(canonical_bytes(records[state['role']]) == canonical_bytes(own), 'artifact_invalid')
    return records


def server_records(state, pin, peers):
    require(isinstance(peers, dict) and set(peers) == set(ROLES), 'artifact_invalid')
    records = strict_json(read_private(Path(state['root']) / 'server-records.json', 32768))
    for role in ROLES:
        validate_server_ready(records[role], state['ctx'], pin, role, peers, now=int(time.time()))
    return records


def adapter_status(path, state, stage):
    status = read_record(path.with_name(path.name + '.status.json'))
    expected = envelope(state['ctx'], state['role'], state['pin_sha256'], 'adapter-result')
    require(isinstance(status, dict) and set(status) == set(expected) | {
        'stage', 'outcome', 'error'} and type(status['format']) is int and
        all(status[key] == value for key, value in expected.items()) and
        status['stage'] == stage, 'artifact_invalid')
    if status['outcome'] == 'failed':
        require(status['error'] in ERRORS, 'artifact_invalid')
        raise SmokeError(status['error'])
    require(status['outcome'] == 'passed' and status['error'] is None, 'artifact_invalid')


def start_server(state, pin, peers):
    from marmish_installed_runtime import launch_server
    ready = launch_server(state, peers)
    validate_server_ready(ready, state['ctx'], pin, state['role'], peers, now=int(time.time()))
    live_deadline(state)


def server_barrier(args, state, pin, peers):
    require(args.peers is not None, 'artifact_invalid')
    adapter_status(args.peers, state, 'server-ready')
    require(continuous_time() < live_deadline(state), 'deadline')
    timing = strict_json(read_private(Path(state['root']) / 'live-start.json', 2048))
    require(continuous_time() < timing['readiness_deadline'], 'deadline')
    records = read_record(args.peers)
    require(isinstance(records, dict) and set(records) == set(ROLES), 'artifact_invalid')
    for role in ROLES:
        validate_server_ready(records[role], state['ctx'], pin, role, peers, now=int(time.time()))
    own = read_record(Path(state['output']) / 'server-ready/record.json')
    require(canonical_bytes(records[state['role']]) == canonical_bytes(own), 'artifact_invalid')
    destination = Path(state['root']) / 'server-records.json'
    if os.path.lexists(destination):
        require(read_private(destination, 32768) == canonical_bytes(records), 'artifact_invalid')
    else:
        write_private(destination, records)
    return records


def clients(state, pin, peers, servers):
    from marmish_installed_runtime import CATEGORIES, failure_client_report, run_clients
    destination = Path(state['output']) / 'client-result.json'
    try:
        deadline = live_deadline(state)
        report = run_clients(state, peers, servers, deadline=deadline)
    except Exception as error:
        if os.path.lexists(destination):
            report = strict_json(read_private(destination, 65536))
        else:
            category = str(error) if isinstance(error, SmokeError) and str(error) in CATEGORIES else 'internal_failure'
            report = failure_client_report(state, peers, servers, category=category)
            write_private(destination, report)
    validate_report(report, state, pin, state['role'], peers, servers)
    publish_client(state, report)
    require(all(slot['status'] == 'received' for slot in report['slots'].values()), 'client_failed')
    return report


def publish_client(state, report):
    from marmish_installed_runtime import validate_client_report
    validate_client_report(report)
    destination = Path(state['output']) / 'client-result.json'
    if os.path.lexists(destination):
        require(read_private(destination, 65536) == canonical_bytes(report), 'artifact_invalid')
    else:
        write_private(destination, report)
    directory = Path(state['output']) / 'client-result'
    directory.mkdir(mode=0o700, exist_ok=True)
    artifact = directory / 'record.json'
    if os.path.lexists(artifact):
        require(read_private(artifact, 65536) == canonical_bytes(report), 'artifact_invalid')
    else:
        write_private(artifact, report)


def await_server(state, pin, peers, servers):
    from marmish_installed_runtime import await_server as wait, validate_server_report
    from marmish_nine_pair import validate_terminal
    report = wait(state, deadline=live_deadline(state, allow_expired=True))
    validate_server_report(report)
    validate_terminal(report, 'server-result', state['ctx'], pin, state['role'], peers, servers)
    require(all(slot['status'] == 'completed' for slot in report['slots'].values()), 'observer_failed')
    return report


def cleanup(args, ctx):
    from marmish_installed_ci import cleanup as prerequisite_cleanup
    from marmish_installed_runtime import cleanup_runtime, stop_supervisor
    return prerequisite_cleanup(args, ctx, None, before_cleanup=stop_supervisor,
                                runtime_cleanup=cleanup_runtime)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=OPERATIONS)
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--role', choices=ROLES, required=True)
    parser.add_argument('--pin-dir', type=Path)
    parser.add_argument('--identities', type=Path)
    parser.add_argument('--peers', type=Path)
    args = parser.parse_args(argv)
    # Application roles and their children never inherit adapter credentials.
    os.environ.pop('GITHUB_TOKEN', None)
    status, output, state, peers, servers = None, None, None, None, None
    try:
        ctx = context()
        if args.operation == 'cleanup':
            return cleanup(args, ctx)
        output = public_directory(args.state)
        status = {**envelope(ctx, args.role, None, 'live-operation-result'),
                  'operation': args.operation, 'outcome': 'failed', 'error': 'artifact_invalid'}
        require(args.pin_dir is not None, 'artifact_invalid')
        pin = load_pin(args.pin_dir, ctx)
        state = checked_state(args, ctx, pin)
        status['pin_sha256'] = state['pin_sha256']
        from marmish_nine_pair import validate_installation
        validate_installation(read_record(output / 'installation-result.json'), ctx, pin, args.role)
        peers = identities(args, state, pin)
        if args.operation == 'start-server':
            start_server(state, pin, peers)
        elif args.operation == 'server-barrier':
            server_barrier(args, state, pin, peers)
        else:
            servers = server_records(state, pin, peers)
            if args.operation == 'clients':
                clients(state, pin, peers, servers)
            else:
                await_server(state, pin, peers, servers)
        status.update(outcome='passed', error=None)
    except Exception as error:
        if status is not None:
            category = str(error) if isinstance(error, SmokeError) and str(error) in ERRORS else 'artifact_invalid'
            status.update(outcome='failed', error=category)
        if args.operation == 'clients' and state is not None and peers is not None:
            try:
                destination = Path(state['output']) / 'client-result.json'
                if not os.path.lexists(destination):
                    from marmish_installed_runtime import failure_client_report
                    publish_client(state, failure_client_report(state, peers, servers))
            except Exception:
                pass  # Missing or unbound prerequisites remain missing evidence.
        print('{"outcome":"failed","error":"live_operation_failed"}')
    if status is None:
        return 1
    try:
        write_record(output / ('live-' + args.operation + '-result.json'), status)
    except Exception:
        return 1
    return 0 if status['outcome'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
