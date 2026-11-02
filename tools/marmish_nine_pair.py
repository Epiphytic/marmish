"""Reconcile installed native evidence; missing evidence always fails acceptance."""
from __future__ import annotations

import argparse
import hashlib
import math
from pathlib import Path

from marmish_installed_release import RELAYS, ROLES, VERSION, canonical_bytes, load_pin
from marmish_installed_rendezvous import (
    context, identity_barrier, installation_tuple, pin_digest, validate_server_ready,
    validate_timing,
)
from marmish_rpc import strict_json
from mdk_smoke import require

CHECKS = frozenset({
    'frontend_absent', 'mdk_absent', 'workers_absent', 'caffeinate_absent',
    'engine_cleanup_ready', 'uninstall_complete', 'root_removed',
})
METRICS = (
    'cold_session_status_seconds', 'command_wall_seconds',
    'command_first_output_seconds', 'output_drain_seconds',
    'effective_download_bytes_per_second', 'output_drain_bytes_per_second',
    'stdin_bytes', 'stdout_bytes', 'stderr_bytes',
)
METRIC_DEFINITIONS = {
    'cold_session_status_seconds': 'Local cold status invocation through process cleanup; includes confinement, relay and MLS setup.',
    'command_wall_seconds': 'Local command invocation through process cleanup; includes startup, stdin transfer, output retrieval and harness teardown.',
    'command_first_output_seconds': 'Local command start to first stdout or stderr byte observed by the harness.',
    'output_drain_seconds': 'First to last stdout or stderr byte observed by the harness.',
    'effective_download_bytes_per_second': 'Verified stdout plus stderr bytes divided by command_wall_seconds; includes harness teardown.',
    'output_drain_bytes_per_second': 'Verified stdout plus stderr bytes divided by output_drain_seconds; null when the interval is zero.',
    'stdin_bytes': 'Verified stdin payload bytes, excluding protocol framing.',
    'stdout_bytes': 'Verified stdout payload bytes, excluding protocol framing.',
    'stderr_bytes': 'Verified stderr payload bytes, excluding protocol framing.',
}


def digest(value):
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def binding(record, ctx, pin, role, kind):
    expected = {'format': 1, 'kind': kind, **ctx,
                'pin_sha256': pin_digest(pin), 'role': role}
    require(isinstance(record, dict) and type(record.get('format')) is int and
            all(record.get(key) == value for key, value in expected.items()),
            'record_binding_invalid')


def validate_cleanup(record, ctx, pin, role):
    binding(record, ctx, pin, role, 'cleanup-result')
    require(set(record) == {'format', 'kind', 'run', 'attempt', 'sha',
                           'pin_sha256', 'role', 'outcome', 'checks', 'error',
                           'duration_ns'} and record['outcome'] == 'complete' and
            record['error'] is None and type(record['duration_ns']) is int and
            record['duration_ns'] >= 0 and isinstance(record['checks'], dict) and
            set(record['checks']) == CHECKS and
            all(value is True for value in record['checks'].values()),
            'cleanup_not_complete')


def validate_installation(record, ctx, pin, role):
    binding(record, ctx, pin, role, 'installation-result')
    require(set(record) == {'format', 'kind', 'run', 'attempt', 'sha',
                           'pin_sha256', 'role', 'outcome', 'stage', 'error',
                           'installation', 'interpreter', 'timing', 'cleanup'} and
            record['outcome'] == 'passed' and record['error'] is None and
            record['installation'] == installation_tuple(pin, role) and
            record['stage'] == 'barrier' and record['cleanup'] in
            ('incomplete', 'complete'), 'installation_not_complete')
    validate_timing(record['timing'], require_valid=True)
    require(set(record['timing']['stages']) == {
        'install', 'verify', 'identity_server', 'identity_client', 'barrier'},
        'installation_not_complete')


def close_number(value):
    return type(value) in (int, float) and math.isfinite(value) and value >= 0


def validate_metrics(metrics, payload):
    require(isinstance(metrics, dict) and set(metrics) == set(METRICS),
            'metrics_invalid')
    for key in METRICS:
        value = metrics[key]
        if key in ('output_drain_seconds', 'output_drain_bytes_per_second') and value is None:
            continue
        require(close_number(value), 'metrics_invalid')
    for stream in ('stdin', 'stdout', 'stderr'):
        require(type(metrics[stream + '_bytes']) is int and
                metrics[stream + '_bytes'] == payload[stream]['bytes'],
                'metrics_invalid')
    wall = metrics['command_wall_seconds']
    first = metrics['command_first_output_seconds']
    drain = metrics['output_drain_seconds']
    require(wall > 0 and first <= wall and
            (drain is None or first + drain <= wall + 1e-6), 'metrics_invalid')
    size = payload['stdout']['bytes'] + payload['stderr']['bytes']
    require(math.isclose(metrics['effective_download_bytes_per_second'], size / wall,
                         rel_tol=1e-9, abs_tol=1e-6), 'metrics_invalid')
    rate = metrics['output_drain_bytes_per_second']
    require((rate is None if drain is None or drain == 0 else
             close_number(rate) and math.isclose(rate, size / drain,
                                                 rel_tol=1e-9, abs_tol=1e-6)),
            'metrics_invalid')


def payload_digest(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def read_public(path, limit=131072):
    require(not path.is_symlink() and path.is_file() and
            path.stat().st_size <= limit, 'record_invalid')
    return strict_json(path.read_bytes())


def pair_cell(ctx, pin, identities, ready, clients, servers, client_role, server_role):
    from marmish_installed_fixture import (
        derive_pair, expected_receipt, expected_streams, source_sha256, stdin_bytes,
    )
    from marmish_installed_observer import validate_proof

    client, server = clients[client_role], servers[server_role]
    pair = derive_pair(ctx, pin_digest(pin), identities[client_role], identities[server_role])
    received, completed = client['slots'][server_role], server['slots'][client_role]
    for slot in (received, completed):
        require(all(slot[key] == pair[key] for key in (
            'context', 'invocation', 'probe_invocation')) and
            slot['client'] == identities[client_role]['client'] and
            slot['server'] == identities[server_role]['server'], 'pair_binding_invalid')
    require(received['status'] == 'received' and received['category'] is None and
            completed['status'] == 'completed' and completed['category'] is None,
            'pair_not_complete')
    fixture = server['fixtures'][client_role]
    require({key: fixture[key] for key in ('path', 'sha256')} ==
            ready[server_role]['fixtures'][client_role] and
            fixture['provenance']['harness_sha'] == ctx['sha'] and
            fixture['provenance']['source_sha256'] == source_sha256() and
            fixture['provenance']['binary_sha256'] == fixture['sha256'] and
            server['expected_principals'][client_role] == identities[client_role]['client'],
            'fixture_binding_invalid')
    stdout, stderr = expected_streams(fixture['path'], pair['context'],
                                      pair['challenge'], pair['invocation'])
    stdin = stdin_bytes(pair['context'])
    payload = {name: payload_digest(data) for name, data in
               [('stdin', stdin), ('stdout', stdout), ('stderr', stderr)]}
    require(received['payload'] == payload and received['exit'] == 7 and
            type(received['exit']) is int and
            set(received['verification']) == {'probe', 'stdout', 'stderr', 'exit'} and
            all(value is True for value in received['verification'].values()),
            'client_payload_invalid')
    receipt = expected_receipt(client_role, pair['context'], pair['challenge'],
                               pair['invocation'], fixture['path'])
    require(completed['receipt'] == receipt and
            type(completed['receipt'].get('outcome')) is int and
            completed['receipt_sha256'] == digest(receipt), 'receipt_invalid')
    validate_proof(completed['proof'], client=identities[client_role]['client'],
                   invocation=pair['invocation'], probe_invocation=pair['probe_invocation'],
                   stdin=stdin, stdout=stdout, stderr=stderr)
    validate_metrics(received['metrics'], payload)
    return {
        'client_role': client_role, 'server_role': server_role,
        'status': 'passed', 'category': None, 'context': pair['context'],
        'invocation': pair['invocation'], 'client': received['client'],
        'server': received['server'], 'payload': payload,
        'metrics': received['metrics'], 'samples': {'cold_status': 1, 'command': 1},
        'authenticated_proof_sha256': digest(completed['proof']),
        'receipt_sha256': completed['receipt_sha256'],
        'fixture_sha256': fixture['sha256'],
    }


def validate_terminal(record, kind, ctx, pin, role, identities, ready):
    from marmish_installed_runtime import command_policy, validate_client_report, validate_server_report
    binding(record, ctx, pin, role, kind)
    validator = validate_client_report if kind == 'client-result' else validate_server_report
    validator(record)
    identity = identities[role]
    principal = 'client' if kind == 'client-result' else 'server'
    require(record[principal] == identity[principal] and
            record['nonce'] == identity['nonce'] and
            record['installation'] == installation_tuple(pin, role) and
            record['relays'] == pin['relays'] and
            record['identity_ready_sha256'] == {r: digest(identities[r]) for r in ROLES} and
            record['server_ready_sha256'] == {r: digest(ready[r]) for r in ROLES},
            'terminal_binding_invalid')
    if kind == 'server-result':
        proofs = [slot['proof'] for slot in record['slots'].values()
                  if slot['status'] == 'completed']
        for label in ('group', 'session'):
            require(len({proof[label] for proof in proofs}) == len(proofs),
                    'server_session_reused')
        require(record['reason'] == 'all-client-results' and
                record['policy_sha256'] == digest(command_policy(record['fixtures'], identities)) and
                record['expected_principals'] == {r: identities[r]['client'] for r in ROLES} and
                type(record['unexpected_submissions']) is int and
                record['unexpected_submissions'] == 0 and
                record['lifetime']['limit_seconds'] == 1800 and
                type(record['lifetime']['limit_seconds']) is int and
                record['lifetime']['clock_valid'] is True and
                close_number(record['lifetime']['elapsed_seconds']) and
                record['lifetime']['elapsed_seconds'] <= 1800,
                'server_not_complete')


def reconcile(ctx, pin, records):
    """Construct only sanitized evidence, never copy an unvalidated report."""
    installations, identities, ready, clients, servers, cleanups = ({}, {}, {}, {}, {}, {})
    errors = {}
    collections = (('installation-result', installations), ('identity-ready', identities),
                   ('server-ready', ready), ('client-result', clients),
                   ('server-result', servers), ('cleanup-result', cleanups))
    kinds = {kind for kind, _collection in collections}
    if (not isinstance(records, dict) or set(records) != set(ROLES) or
            any(not isinstance(value, dict) or not set(value) <= kinds
                for value in records.values())):
        errors['record-set'] = 'record_set_invalid'
    for kind, collection in collections:
        for role in ROLES:
            try:
                collection[role] = records[role][kind]
            except (KeyError, TypeError):
                errors[f'{role}/{kind}'] = 'record_missing'
    try:
        # Freshness was enforced by the live adapter. Archival reconciliation
        # checks simultaneous validity at publication rather than today's clock.
        snapshot_at = max(record['published_at'] for record in
                          (*identities.values(), *ready.values()))
        require(type(snapshot_at) is int, 'identity_barrier_invalid')
        identity_barrier(identities, ctx, pin, now=snapshot_at)
    except Exception:
        errors['identity-barrier'] = 'identity_barrier_invalid'
    if 'identity-barrier' not in errors:
        for role in ROLES:
            for kind, record, validator in (
                ('installation-result', installations.get(role),
                 lambda r, role=role: validate_installation(r, ctx, pin, role)),
                ('server-ready', ready.get(role),
                 lambda r, role=role: validate_server_ready(r, ctx, pin, role, identities,
                                                           now=snapshot_at)),
                ('cleanup-result', cleanups.get(role),
                 lambda r, role=role: validate_cleanup(r, ctx, pin, role)),
            ):
                try:
                    validator(record)
                except Exception:
                    category = 'record_invalid'
                    if kind == 'installation-result' and isinstance(record, dict):
                        from marmish_installed_ci import ERRORS
                        if isinstance(record.get('error'), str) and record['error'] in ERRORS:
                            category = record['error']
                    errors[f'{role}/{kind}'] = category
        if not any(key.endswith('/server-ready') for key in errors):
            for kind, collection in (('client-result', clients), ('server-result', servers)):
                for role in ROLES:
                    try:
                        validate_terminal(collection.get(role), kind, ctx, pin, role, identities, ready)
                    except Exception:
                        errors[f'{role}/{kind}'] = 'record_invalid'
    cells = []
    for client_role in ROLES:
        for server_role in ROLES:
            cell = {'client_role': client_role, 'server_role': server_role,
                    'status': 'failed', 'category': 'required_evidence_invalid',
                    'metrics': None, 'payload': None,
                    'samples': {'cold_status': 0, 'command': 0}}
            relevant = any(key in ('identity-barrier', 'record-set') or
                           key.startswith(client_role + '/') or
                           key.startswith(server_role + '/') for key in errors)
            if not relevant:
                try:
                    cell = pair_cell(ctx, pin, identities, ready, clients, servers,
                                     client_role, server_role)
                except Exception:
                    cell['category'] = 'pair_evidence_invalid'
                    from marmish_installed_runtime import CATEGORIES
                    for slot in (clients[client_role]['slots'][server_role],
                                 servers[server_role]['slots'][client_role]):
                        category = slot.get('category')
                        if isinstance(category, str) and category in CATEGORIES:
                            cell['category'] = category
                            break
            cells.append(cell)
    passed = not errors and all(cell['status'] == 'passed' for cell in cells)
    return {
        'format': 1, 'kind': 'nine-pair-result', **ctx, 'pin_sha256': pin_digest(pin),
        'outcome': 'passed' if passed else 'failed',
        'run_url': f"https://github.com/Epiphytic/marmish/actions/runs/{ctx['run']}/attempts/{ctx['attempt']}",
        'application_source': pin['source_commit'], 'release': pin['version'],
        'installer_sha256': pin['installer_sha256'], 'relays': pin['relays'],
        'targets': pin['targets'], 'storage': 'disposable-file',
        'clock': 'local-suspend-inclusive', 'cells': cells, 'errors': errors,
        'sample_count': sum(cell['samples']['command'] for cell in cells),
        'metric_definitions': METRIC_DEFINITIONS,
        'harness_commit_url': f"https://github.com/Epiphytic/marmish/commit/{ctx['sha']}",
    }


def markdown(result):
    lines = [
        '# Installed Marmot interoperability measurements', '',
        f"Outcome: **{result['outcome']}**. [Actions run]({result['run_url']}).", '',
        f"Application: `{result['application_source']}`; harness: `{result['sha']}`; "
        f"release: `{result['release']}`.", '',
        'Client → server | Linux AMD64 | Linux ARM64 | macOS ARM64',
        '--- | --- | --- | ---',
    ]
    for client in ROLES:
        cells = [cell for cell in result['cells'] if cell['client_role'] == client]
        lines.append(client + ' | ' + ' | '.join(cell['status'] for cell in cells))
    lines += ['', 'Client → server | Cold status (ms) | Command (ms) | First output (ms) | Effective download (B/s) | Output drain (B/s) | Input/output bytes',
              '--- | ---: | ---: | ---: | ---: | ---: | ---:']
    for cell in result['cells']:
        metrics = cell['metrics']
        if metrics is None:
            values = ['—'] * 6
        else:
            values = [f"{metrics[key] * 1000:.3f}" for key in (
                'cold_session_status_seconds', 'command_wall_seconds',
                'command_first_output_seconds')]
            values.append(f"{metrics['effective_download_bytes_per_second']:.1f}")
            rate = metrics['output_drain_bytes_per_second']
            values.append('—' if rate is None else f"{rate:.1f}")
            values.append(f"{metrics['stdin_bytes']}/{metrics['stdout_bytes'] + metrics['stderr_bytes']}")
        lines.append(f"{cell['client_role']} → {cell['server_role']} | " + ' | '.join(values))
    lines += ['',
        'Each successful cell contains one cold nonexecuting status probe and one command sample. '
        'Durations use local suspend-inclusive clocks; no cross-host clock subtraction. '
        'Cold status includes startup, confinement, relay and MLS setup. Command time includes '
        'startup, stdin transfer, complete output retrieval in an established room and harness teardown. '
        'These timings include process cleanup and are conservative user-operation measurements. '
        'Effective download is verified stdout plus stderr divided by command time, not link capacity. '
        'Payload byte counts exclude protocol, base64 and MLS framing. Failed cells have no performance sample.',
        '', 'Explicit relays: ' + ', '.join('`' + value + '`' for value in result['relays']) + '.', '',
    ]
    return '\n'.join(lines)


def missing_pin_result(ctx):
    return {'format': 1, 'kind': 'nine-pair-result', **ctx,
            'pin_sha256': None, 'outcome': 'failed',
            'run_url': f"https://github.com/Epiphytic/marmish/actions/runs/{ctx['run']}/attempts/{ctx['attempt']}",
            'application_source': None, 'release': VERSION, 'installer_sha256': None,
            'relays': list(RELAYS), 'targets': None, 'storage': 'disposable-file',
            'clock': 'local-suspend-inclusive', 'errors': {'pin': 'pin_unavailable_or_invalid'},
            'sample_count': 0, 'metric_definitions': METRIC_DEFINITIONS,
            'harness_commit_url': f"https://github.com/Epiphytic/marmish/commit/{ctx['sha']}",
            'cells': [
                {'client_role': client, 'server_role': server, 'status': 'failed',
                 'category': 'pin_unavailable_or_invalid', 'metrics': None, 'payload': None,
                 'samples': {'cold_status': 0, 'command': 0}}
                for client in ROLES for server in ROLES]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pin-dir', type=Path, required=True)
    parser.add_argument('--records', type=Path, required=True)
    parser.add_argument('--json', type=Path, required=True)
    parser.add_argument('--markdown', type=Path, required=True)
    args = parser.parse_args()
    ctx = context()
    try:
        pin = load_pin(args.pin_dir, ctx)
    except Exception:
        result = missing_pin_result(ctx)
        args.json.write_bytes(canonical_bytes(result))
        args.markdown.write_text(markdown(result), encoding='utf-8')
        return 1
    records = {}
    expected_directories = {
        f"installed-interop-{ctx['run']}-{ctx['attempt']}-{role}-prerequisite-result"
        for role in ROLES}
    if args.records.is_dir() and any(entry.name not in expected_directories
                                     for entry in args.records.iterdir()):
        records['unexpected-artifact'] = {}
    for role in ROLES:
        root = args.records / f"installed-interop-{ctx['run']}-{ctx['attempt']}-{role}-prerequisite-result"
        records[role] = {}
        for kind in ('installation-result', 'client-result', 'server-result',
                     'cleanup-result', 'identity-ready', 'server-ready'):
            path = root / (kind + '/record.json' if kind.endswith('-ready') else kind + '.json')
            try:
                records[role][kind] = read_public(path)
            except Exception:
                pass
    result = reconcile(ctx, pin, records)
    args.json.write_bytes(canonical_bytes(result))
    args.markdown.write_text(markdown(result), encoding='utf-8')
    return 0 if result['outcome'] == 'passed' else 1


if __name__ == '__main__':
    raise SystemExit(main())
