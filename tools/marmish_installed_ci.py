"""Actual-script installed validation prerequisites; no source runtime fallback."""
from __future__ import annotations

import argparse
import hashlib
from pathlib import Path
import signal
import time

from mdk_smoke import SmokeError, require
from marmish_installed import strict_json, target as native_target
from marmish_installed_release import (
    VERSION, ROLE_TARGETS, MAX_MANIFEST, _read_regular, canonical_bytes, load_pin, resolve_release,
    verify_installed,
)
from marmish_installed_rendezvous import (
    context, identity_barrier, installation_tuple, pin_digest, validate_identity,
    validate_timing,
)
from marmish_installed_state import (
    cleanup_state, clock_sample, create_state, elapsed, initialize_identity,
    interpreter_info, load_state, public_directory, register_installed,
    run_bounded, read_private, failure_diagnostic,
)

REPO = Path(__file__).resolve().parent.parent
ERRORS = frozenset({
    'toolchain_source_unowned', 'toolchain_source_cleanup_failed',
    'pin_unavailable', 'pin_invalid', 'release_resolve_failed',
    'installer_refused', 'installer_timeout', 'installer_output_limit',
    'install_verify_failed', 'identity_init_failed', 'barrier_timeout',
    'artifact_invalid', 'artifact_network_failed', 'artifact_listing_unstable',
    'clock_invalid', 'uninstall_failed', 'cleanup_incomplete', 'internal_failure',
    'process_scan_preflight_failed',
})


def write_record(path, value):
    """Public reports contain only explicitly constructed finite schemas."""
    raw = canonical_bytes(value)
    require(len(raw) <= 32768, 'artifact_invalid')
    temporary = path.with_name(path.name + '.pending')
    with temporary.open('xb') as output:
        output.write(raw)
    temporary.chmod(0o600)
    temporary.replace(path)


def read_record(path, limit=32768):
    require(not path.is_symlink() and path.is_file() and
            path.stat().st_size <= limit, 'artifact_invalid')
    return strict_json(path.read_bytes())


def envelope(ctx, role, digest, kind):
    return {'format': 1, 'kind': kind, **ctx,
            'pin_sha256': digest, 'role': role}


def initial_report(ctx, role):
    return {
        **envelope(ctx, role, None, 'installation-result'),
        'outcome': 'failed', 'stage': 'pin', 'error': 'pin_unavailable',
        'installation': None, 'interpreter': None,
        'timing': {'unit': 'ns', 'clock': 'continuous+monotonic', 'stages': {}},
        'cleanup': 'not-started',
    }


def measure(report, stage, action):
    report['stage'] = stage
    start = clock_sample()
    try:
        return action()
    finally:
        duration = elapsed(start, clock_sample())
        report['timing']['stages'][stage] = duration
        require(duration['clock_valid'], 'clock_invalid')


def remaining(state, deadline, cap):
    value = min(cap, state[deadline] - clock_sample()['continuous_ns'] / 1e9)
    require(value > 0, 'installer_timeout' if deadline == 'preparation_deadline'
            else 'barrier_timeout')
    return value


def prepare(args, ctx, pin, report):
    installer_raw = _read_regular(REPO / 'scripts/install.sh', MAX_MANIFEST)
    require(hashlib.sha256(installer_raw).hexdigest() == pin['installer_sha256'],
            'install_verify_failed')
    require(native_target() == ROLE_TARGETS[args.role], 'install_verify_failed')
    state = create_state(args.state, ctx, pin_digest(pin), args.role,
                         installer_bytes=installer_raw, installer_sha256=pin['installer_sha256'])
    require(state['installer_sha256'] == pin['installer_sha256'],
            'install_verify_failed')
    result = measure(report, 'install', lambda: run_bounded(
        ['/bin/sh', str(Path(state['root']) / 'install.sh'), '--debug',
         '--version', pin['version'], '--prefix', str(state['prefix'])], state,
        timeout=remaining(state, 'preparation_deadline', 900),
        output_limit=2 * 1024 * 1024, runtime=False))
    require(not result['timed_out'], 'installer_timeout')
    require(not result['output_limited'], 'installer_output_limit')
    require(result['returncode'] == 0 and result['cleanup'] == 'complete',
            'installer_refused')
    require(hashlib.sha256(read_private(Path(state['installer']))).hexdigest() ==
            pin['installer_sha256'], 'install_verify_failed')
    manifest = (args.pin_dir / pin['targets'][args.role]['manifest_name']).read_bytes()
    verified = measure(report, 'verify', lambda: verify_installed(
        Path(state['prefix']), pin, manifest, args.role))
    register_installed(state, verified['root'])
    if getattr(args, 'toolchain_source', None) is not None:
        from marmish_installed_fixture import prepare_dependencies
        try:
            prepare_dependencies(state, deadline=state['preparation_deadline'],
                                 toolchain_source=args.toolchain_source,
                                 consume_source=getattr(args, 'consume_toolchain', False))
        except (ValueError, RuntimeError) as failure:
            if str(failure) in {'toolchain_source_unowned', 'toolchain_source_cleanup_failed'}:
                raise SmokeError(str(failure)) from None
            raise
    report['installation'] = verified['installation']
    report['interpreter'] = interpreter_info(state)
    report.update(outcome='passed', error=None, cleanup='incomplete')
    return state


def checked_state(args, ctx, pin):
    state = load_state(args.state)
    require(state['ctx'] == ctx and state['role'] == args.role and
            state['pin_sha256'] == pin_digest(pin), 'artifact_invalid')
    return state


def identities(args, ctx, pin, report):
    state = checked_state(args, ctx, pin)
    accounts = {}
    for name in ('server', 'client'):
        accounts[name] = measure(report, 'identity_' + name, lambda name=name:
            initialize_identity(state, name, timeout=remaining(
                state, 'preparation_deadline', 300)))
    require(accounts['server'] != accounts['client'], 'identity_init_failed')
    import os
    now = int(time.time())
    record = {
        **envelope(ctx, args.role, pin_digest(pin), 'identity-ready'),
        'target': ROLE_TARGETS[args.role], 'nonce': os.urandom(32).hex(),
        **accounts, 'relays': pin['relays'],
        'installation': installation_tuple(pin, args.role),
        'interpreter': report['interpreter'], 'timing': report['timing'],
        'published_at': now, 'expires_at': now + 4200,
    }
    validate_identity(record, ctx, pin, args.role, now=now)
    ready_dir = Path(state['output']) / 'identity-ready'
    ready_dir.mkdir(mode=0o700)
    write_record(ready_dir / 'record.json', record)
    write_record(Path(state['root']) / 'barrier-start.json', clock_sample())
    report.update(outcome='passed', error=None)


def barrier(args, ctx, pin, report):
    report['stage'] = 'barrier'
    state = checked_state(args, ctx, pin)
    require(args.peers is not None, 'artifact_invalid')
    status = read_record(args.peers.with_name(args.peers.name + '.status.json'))
    require(set(status) == {'format', 'kind', 'run', 'attempt', 'sha',
                           'pin_sha256', 'role', 'stage', 'outcome', 'error'} and
            type(status['format']) is int and
            all(status[key] == value for key, value in envelope(
                ctx, args.role, pin_digest(pin), 'adapter-result').items()) and
            status['stage'] == 'identity-ready', 'artifact_invalid')
    if status['outcome'] == 'failed':
        require(status['error'] in {'barrier_timeout', 'artifact_network_failed',
                                   'artifact_listing_unstable', 'artifact_invalid'},
                'artifact_invalid')
        raise SmokeError(status['error'])
    require(status['outcome'] == 'passed' and status['error'] is None,
            'artifact_invalid')
    remaining(state, 'barrier_deadline', 600)
    peers = identity_barrier(read_record(args.peers), ctx, pin, now=int(time.time()))
    own = read_record(Path(state['output']) / 'identity-ready/record.json')
    require(canonical_bytes(peers[args.role]) == canonical_bytes(own), 'artifact_invalid')
    start = read_record(Path(state['root']) / 'barrier-start.json')
    duration = elapsed(start, clock_sample())
    require(duration['clock_valid'], 'clock_invalid')
    report['timing']['stages']['barrier'] = duration
    report.update(outcome='passed', stage='barrier', error=None)


def cleanup(args, ctx, digest, *, before_cleanup=None, runtime_cleanup=None):
    output = public_directory(args.state)
    record = envelope(ctx, args.role, digest, 'cleanup-result')
    checks = {key: None for key in (
        'frontend_absent', 'mdk_absent', 'workers_absent', 'caffeinate_absent',
        'engine_cleanup_ready', 'uninstall_complete', 'root_removed')}
    start = clock_sample()
    try:
        if not args.state.exists() and not args.state.is_symlink():
            record.update(outcome='not-started', checks=checks, error=None,
                          duration_ns=0)
        else:
            state = load_state(args.state)
            require(state['ctx'] == ctx and state['role'] == args.role,
                    'cleanup_incomplete')
            record['pin_sha256'] = state['pin_sha256']
            if before_cleanup is not None:
                before_cleanup(state)
            record.update(cleanup_state(state, runtime_cleanup=runtime_cleanup))
    except Exception as failure:
        failure_diagnostic(failure, stage='cleanup_load', category='cleanup_incomplete')
        record.update(outcome='incomplete', checks=checks,
                      error='cleanup_incomplete',
                      duration_ns=max(0, clock_sample()['continuous_ns'] -
                                      start['continuous_ns']))
    write_record(output / 'cleanup-result.json', record)
    return 0 if record['outcome'] == 'complete' else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=(
        'resolve', 'prepare', 'identity-ready', 'barrier', 'cleanup'))
    parser.add_argument('--state', type=Path)
    parser.add_argument('--pin-dir', type=Path, required=True)
    parser.add_argument('--role', choices=tuple(ROLE_TARGETS))
    parser.add_argument('--peers', type=Path)
    parser.add_argument('--toolchain-source', type=Path)
    parser.add_argument('--consume-toolchain', action='store_true')
    args = parser.parse_args()
    if args.consume_toolchain and args.toolchain_source is None:
        parser.error('--consume-toolchain requires --toolchain-source')
    ctx = context()
    if args.operation == 'resolve':
        resolve_release(REPO, args.pin_dir, ctx, version=VERSION)
        return 0
    require(args.state is not None and args.role is not None, 'artifact_invalid')
    output = public_directory(args.state)
    if args.operation == 'cleanup':
        return cleanup(args, ctx, None)
    report_path = output / 'installation-result.json'
    report = initial_report(ctx, args.role)
    try:
        pin = load_pin(args.pin_dir, ctx)
    except Exception:
        report['error'] = 'pin_invalid' if args.pin_dir.exists() else 'pin_unavailable'
        write_record(report_path, report)
        return 1
    report['pin_sha256'] = pin_digest(pin)
    try:
        if args.operation != 'prepare':
            previous = read_record(report_path)
            require(set(previous) == set(report) and
                    all(previous[key] == value for key, value in
                        envelope(ctx, args.role, pin_digest(pin),
                                 'installation-result').items()) and
                    previous['outcome'] == 'passed' and previous['error'] is None
                    and previous['installation'] == installation_tuple(pin, args.role),
                    'artifact_invalid')
            validate_timing(previous['timing'], require_valid=True)
            report = previous
        if args.operation == 'prepare':
            prepare(args, ctx, pin, report)
        elif args.operation == 'identity-ready':
            identities(args, ctx, pin, report)
        else:
            barrier(args, ctx, pin, report)
    except (Exception, KeyboardInterrupt) as error:
        category = (str(error) if isinstance(error, SmokeError) else
                    'artifact_invalid' if report['stage'] == 'barrier' else 'internal_failure')
        if category not in ERRORS:
            category = ('identity_init_failed' if report['stage'].startswith('identity_')
                        else 'install_verify_failed' if report['stage'] == 'verify'
                        else 'internal_failure')
        report.update(outcome='failed', error=category)
        # Finite diagnostics distinguish platform failures without exposing argv,
        # command output, private paths or exception messages.
        failure_diagnostic(error, stage=report['stage'], category=category)
    write_record(report_path, report)
    return 0 if report['outcome'] == 'passed' else 1


def interrupted(_signal, _frame):
    raise KeyboardInterrupt


if __name__ == '__main__':
    signal.signal(signal.SIGTERM, interrupted)
    raise SystemExit(main())
