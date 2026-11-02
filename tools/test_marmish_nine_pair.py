"""Strict reconciliation refuses incomplete evidence and misleading metrics."""
import copy
import unittest

import marmish_nine_pair as rollup
from mdk_smoke import SmokeError
from test_marmish_installed_rendezvous import CTX, RELAYS, sample_pin
from test_marmish_installed_rendezvous import ready_record, sample_records


class Measurements(unittest.TestCase):
    def setUp(self):
        self.payload = {'stdin': {'bytes': 8192}, 'stdout': {'bytes': 65536},
                        'stderr': {'bytes': 8192}}
        self.metrics = dict(zip(rollup.METRICS,
            [5.0, 10.0, 2.0, 4.0, 7372.8, 18432.0, 8192, 65536, 8192]))

    def test_rates_use_verified_payload_and_seconds(self):
        rollup.validate_metrics(self.metrics, self.payload)
        for key in ('effective_download_bytes_per_second',
                    'output_drain_bytes_per_second'):
            bad = dict(self.metrics)
            bad[key] *= 1000
            with self.assertRaises(SmokeError):
                rollup.validate_metrics(bad, self.payload)

    def test_no_infinite_or_boolean_measurements(self):
        for invalid in (float('nan'), float('inf'), -1, True, '1'):
            with self.subTest(invalid=invalid):
                bad = dict(self.metrics, command_wall_seconds=invalid)
                with self.assertRaises(SmokeError):
                    rollup.validate_metrics(bad, self.payload)

    def test_missing_drain_has_null_rate(self):
        for duration in (None, 0):
            bad = dict(self.metrics, output_drain_seconds=duration,
                       output_drain_bytes_per_second=None)
            rollup.validate_metrics(bad, self.payload)
            bad['output_drain_bytes_per_second'] = 0
            with self.assertRaises(SmokeError):
                rollup.validate_metrics(bad, self.payload)

    def test_negative_timing_and_byte_mismatch_refused(self):
        for changes in ({'command_first_output_seconds': 11},
                        {'output_drain_seconds': 9}, {'stdout_bytes': 1},
                        {'stdin_bytes': True}, {'extra': 0}):
            with self.subTest(changes=changes):
                with self.assertRaises(SmokeError):
                    rollup.validate_metrics(dict(self.metrics, **changes), self.payload)

    def test_markdown_milliseconds_and_failed_cells_are_not_zero(self):
        cells = [{'client_role': c, 'server_role': s, 'status': 'failed',
                  'metrics': None} for c in rollup.ROLES for s in rollup.ROLES]
        cells[0].update(status='passed', metrics=copy.deepcopy(self.metrics))
        result = {'outcome': 'failed', 'run_url': 'https://github.com/Epiphytic/marmish/actions/runs/1',
                  'application_source': 'a' * 40, 'sha': 'b' * 40,
                  'release': 'test', 'cells': cells, 'relays': ['wss://relay.example/']}
        text = rollup.markdown(result)
        self.assertIn('5000.000 | 10000.000 | 2000.000 | 7372.8', text)
        self.assertEqual(text.count(' | — | — | — | — | — | —'), 8)
        self.assertIn('one command sample', text)


class PartialEvidence(unittest.TestCase):
    def setUp(self):
        self.pin = dict(sample_pin(), relays=RELAYS)

    def test_absent_and_cancelled_jobs_leave_all_nine_failed_cells(self):
        for records in ({}, {role: {} for role in rollup.ROLES}):
            result = rollup.reconcile(CTX, self.pin, records)
            self.assertEqual(result['outcome'], 'failed')
            self.assertEqual(result['sample_count'], 0)
            self.assertEqual(len(result['cells']), 9)
            self.assertEqual({(c['client_role'], c['server_role']) for c in result['cells']},
                             {(c, s) for c in rollup.ROLES for s in rollup.ROLES})
            self.assertTrue(all(c['metrics'] is None for c in result['cells']))

    def test_cleanup_requires_every_final_check_with_strict_boolean_types(self):
        envelope = {'format': 1, 'kind': 'cleanup-result', **CTX,
                    'pin_sha256': rollup.pin_digest(self.pin), 'role': rollup.ROLES[0],
                    'outcome': 'complete', 'checks': {k: True for k in rollup.CHECKS},
                    'error': None, 'duration_ns': 1}
        rollup.validate_cleanup(envelope, CTX, self.pin, rollup.ROLES[0])
        for key in rollup.CHECKS:
            for value in (None, False, 1, 'true'):
                bad = copy.deepcopy(envelope)
                bad['checks'][key] = value
                with self.subTest(key=key, value=value), self.assertRaises(SmokeError):
                    rollup.validate_cleanup(bad, CTX, self.pin, rollup.ROLES[0])
        for changes in ({'format': True}, {'duration_ns': True},
                        {'outcome': 'provisional'}, {'attempt': '2'}, {'extra': 'value'}):
            with self.subTest(changes=changes), self.assertRaises(SmokeError):
                rollup.validate_cleanup(dict(envelope, **changes), CTX, self.pin, rollup.ROLES[0])


def successful_records():
    """Synthetic local reports, never used as native validation evidence."""
    from marmish_installed_fixture import (
        derive_pair, expected_receipt, expected_streams, source_sha256, stdin_bytes,
    )
    from test_marmish_installed_observer import observer, transcript
    pin = dict(sample_pin(), relays=RELAYS)
    identities = sample_records(pin)
    ready = {role: ready_record(identities, role) for role in rollup.ROLES}
    identity_digests = {r: rollup.digest(v) for r, v in identities.items()}
    ready_digests = {r: rollup.digest(v) for r, v in ready.items()}
    template = observer()
    template.scan(transcript())
    template = template.proof()
    timing = {'elapsed_ns': 30_000_000_000, 'active_ns': 30_000_000_000,
              'suspended_ns': 0, 'clock_valid': True}
    records = {}
    for role in rollup.ROLES:
        common = {'format': 1, **CTX, 'pin_sha256': rollup.pin_digest(pin),
                  'role': role, 'nonce': identities[role]['nonce'],
                  'installation': rollup.installation_tuple(pin, role), 'relays': RELAYS,
                  'identity_ready_sha256': identity_digests,
                  'server_ready_sha256': ready_digests, 'timing': copy.deepcopy(timing)}
        install = {key: common[key] for key in ('format', 'run', 'attempt', 'sha', 'pin_sha256', 'role')}
        install.update(kind='installation-result', outcome='passed', stage='barrier', error=None,
                       installation=common['installation'], interpreter=identities[role]['interpreter'],
                       cleanup='incomplete', timing=copy.deepcopy(identities[role]['timing']))
        install['timing']['stages']['barrier'] = dict(timing)
        cleanup = {key: common[key] for key in ('format', 'run', 'attempt', 'sha', 'pin_sha256', 'role')}
        cleanup.update(kind='cleanup-result', outcome='complete', error=None, duration_ns=10,
                       checks={key: True for key in rollup.CHECKS})
        records[role] = {
            'identity-ready': identities[role], 'server-ready': ready[role],
            'installation-result': install, 'cleanup-result': cleanup,
            'client-result': dict(common, kind='client-result', client=identities[role]['client'], slots={}),
            'server-result': dict(common, kind='server-result', server=identities[role]['server'],
                slots={}, policy_sha256='a' * 64, fixtures={},
                expected_principals={r: identities[r]['client'] for r in rollup.ROLES},
                reason='all-client-results', unexpected_submissions=0, cleanup='provisional',
                lifetime={'limit_seconds': 1800, 'elapsed_seconds': 30.0, 'clock_valid': True}),
        }
    for ci, client in enumerate(rollup.ROLES):
        for si, server in enumerate(rollup.ROLES):
            pair = derive_pair(CTX, rollup.pin_digest(pin), identities[client], identities[server])
            fixture = dict(ready[server]['fixtures'][client], provenance={
                'harness_sha': CTX['sha'], 'source_sha256': source_sha256(),
                'compiler': 'rustc test', 'binary_sha256': 'f' * 64})
            records[server]['server-result']['fixtures'][client] = fixture
            stdout, stderr = expected_streams(fixture['path'], pair['context'],
                                              pair['challenge'], pair['invocation'])
            stdin = stdin_bytes(pair['context'])
            payload = {stream: rollup.payload_digest(data) for stream, data in
                       [('stdin', stdin), ('stdout', stdout), ('stderr', stderr)]}
            slot = {key: pair[key] for key in ('context', 'invocation', 'probe_invocation')}
            slot.update(client=identities[client]['client'], server=identities[server]['server'])
            size = len(stdout) + len(stderr)
            metrics = dict(zip(rollup.METRICS,
                [5.0, 10.0, 2.0, 4.0, size / 10, size / 4, len(stdin), len(stdout), len(stderr)]))
            records[client]['client-result']['slots'][server] = dict(slot,
                status='received', category=None, payload=payload, exit=7, metrics=metrics,
                verification=dict.fromkeys(('probe', 'stdout', 'stderr', 'exit'), True))
            proof = copy.deepcopy(template)
            proof.update(principal=slot['client'], invocation=pair['invocation'],
                         group=f'{ci * 3 + si + 1:064x}', session=f'{ci * 3 + si + 1:032x}')
            proof['probe']['invocation'] = pair['probe_invocation']
            proof['input'].update(payload['stdin'])
            for write in proof['input']['writes']:
                offset = write['offset']
                write['sha256'] = rollup.payload_digest(stdin[offset:offset + write['bytes']])['sha256']
            for stream in ('stdout', 'stderr'):
                proof[stream] = payload[stream]
                proof['terminal'][stream + '_cursor'] = payload[stream]['bytes']
            receipt = expected_receipt(client, pair['context'], pair['challenge'],
                                       pair['invocation'], fixture['path'])
            records[server]['server-result']['slots'][client] = dict(slot,
                status='completed', category=None, proof=proof, receipt=receipt,
                receipt_sha256=rollup.digest(receipt))
    from marmish_installed_runtime import command_policy
    for role in rollup.ROLES:
        report = records[role]['server-result']
        report['policy_sha256'] = rollup.digest(command_policy(report['fixtures'], identities))
    return pin, records


class NinePairReconciliation(unittest.TestCase):
    def setUp(self):
        self.pin, self.records = successful_records()

    def test_exact_cartesian_product_passes(self):
        result = rollup.reconcile(CTX, self.pin, self.records)
        self.assertEqual(result['outcome'], 'passed', result['errors'])
        self.assertEqual(result['sample_count'], 9)
        self.assertTrue(result['harness_commit_url'].endswith('/' + CTX['sha']))
        self.assertEqual(set(result['metric_definitions']), set(rollup.METRICS))
        self.assertIn('teardown', result['metric_definitions']['command_wall_seconds'])
        for cell in result['cells']:
            self.assertEqual(cell['fixture_sha256'], self.records[
                cell['server_role']]['server-result']['fixtures'][cell['client_role']]['sha256'])

    def test_different_clients_cannot_share_server_group_or_session(self):
        role = rollup.ROLES[0]
        for label in ('group', 'session'):
            bad = copy.deepcopy(self.records)
            slots = bad[role]['server-result']['slots']
            slots[rollup.ROLES[1]]['proof'][label] = slots[rollup.ROLES[0]]['proof'][label]
            result = rollup.reconcile(CTX, self.pin, bad)
            self.assertEqual(result['outcome'], 'failed')
            self.assertIn(role + '/server-result', result['errors'])

    def test_delete_every_required_record(self):
        for role in rollup.ROLES:
            for kind in self.records[role]:
                bad = copy.deepcopy(self.records)
                del bad[role][kind]
                with self.subTest(role=role, kind=kind):
                    result = rollup.reconcile(CTX, self.pin, bad)
                    self.assertEqual(result['outcome'], 'failed')
                    self.assertEqual(len(result['cells']), 9)

    def test_duplicate_platform_or_record_kind_refused(self):
        for role, kind in (('fourth-platform', 'client-result'), (rollup.ROLES[0], 'fourth-result')):
            bad = copy.deepcopy(self.records)
            bad.setdefault(role, {})[kind] = {}
            result = rollup.reconcile(CTX, self.pin, bad)
            self.assertEqual(result['outcome'], 'failed')
            self.assertEqual(result['sample_count'], 0)

    def test_tampered_binding_payload_receipt_and_fourth_command(self):
        role, peer = rollup.ROLES[:2]
        for mutate in (
            lambda r: r[role]['client-result'].__setitem__('attempt', '2'),
            lambda r: r[role]['client-result']['slots'][peer].__setitem__('client', '0' * 64),
            lambda r: r[role]['client-result']['slots'][peer].__setitem__('invocation', '0' * 32),
            lambda r: r[role]['client-result']['slots'][peer]['payload']['stdout'].__setitem__('bytes', 1),
            lambda r: r[peer]['server-result'].__setitem__('unexpected_submissions', 1),
            lambda r: r[peer]['server-result']['slots'][role]['receipt'].__setitem__('outcome', 0),
            lambda r: r[role]['cleanup-result']['checks'].__setitem__('root_removed', False),
            lambda r: r[peer]['server-result']['fixtures'][role]['provenance'].__setitem__('source_sha256', '0' * 64),
        ):
            bad = copy.deepcopy(self.records)
            mutate(bad)
            self.assertEqual(rollup.reconcile(CTX, self.pin, bad)['outcome'], 'failed')


if __name__ == '__main__':
    unittest.main()
