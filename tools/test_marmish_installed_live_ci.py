"""Finite live Actions wrappers validate evidence without launching application roles."""
import argparse
import copy
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import marmish_installed_live_ci as live
import marmish_installed_ci as prerequisite
import marmish_installed_runtime as runtime
from marmish_installed_release import ROLES, RELAYS, canonical_bytes
from marmish_installed_rendezvous import pin_digest
from marmish_installed_state import clock_sample, read_private, write_private
from marmish_platform import continuous_time
from marmish_rpc import strict_json
from mdk_smoke import SmokeError
from test_marmish_installed_rendezvous import CTX, sample_pin, sample_records, ready_record


class LiveOperations(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        root.chmod(0o700)
        output = root / 'public'
        output.mkdir(mode=0o700)
        self.pin = dict(sample_pin(), relays=RELAYS)
        self.ids = sample_records(self.pin)
        self.now = self.ids[ROLES[0]]['published_at']
        self.ready = {role: ready_record(self.ids, role) for role in ROLES}
        began = clock_sample()
        self.deadline = began['continuous_ns'] / 1e9 + 1800
        self.state = {'ctx': CTX, 'role': ROLES[0], 'root': str(root), 'output': str(output),
                      'pin_sha256': pin_digest(self.pin), 'overall_deadline': self.deadline + 150}
        write_private(root / 'live-start.json', {'start': began, 'deadline': self.deadline,
                                               'readiness_deadline': self.deadline - 1500})
        self.args = argparse.Namespace(state=root / 'pointer', role=ROLES[0], identities=root / 'identities.json',
                                       peers=root / 'server-peers.json')
        write_private(self.args.identities, self.ids)
        for stage, record in (('identity-ready', self.ids[ROLES[0]]), ('server-ready', self.ready[ROLES[0]])):
            directory = output / stage
            directory.mkdir(mode=0o700)
            write_private(directory / 'record.json', record)
        self.status = {**live.envelope(CTX, ROLES[0], self.state['pin_sha256'], 'adapter-result'),
                       'stage': 'server-ready', 'outcome': 'passed', 'error': None}

    def peers(self, records=None, status=None):
        live.write_record(self.args.peers, self.ready if records is None else records)
        live.write_record(self.args.peers.with_name(self.args.peers.name + '.status.json'),
                          self.status if status is None else status)

    def test_identity_barrier_exact_own_record_required(self):
        with patch.object(live.time, 'time', return_value=self.now):
            self.assertEqual(live.identities(self.args, self.state, self.pin), self.ids)
            changed = copy.deepcopy(self.ids)
            changed[ROLES[0]]['nonce'] = 'f' * 64
            live.write_record(self.args.identities, changed)
            with self.assertRaises(SmokeError):
                live.identities(self.args, self.state, self.pin)

    def test_server_barrier_validates_status_binding_and_persists_immutable_map(self):
        self.peers()
        with patch.object(live.time, 'time', return_value=self.now):
            self.assertEqual(live.server_barrier(self.args, self.state, self.pin, self.ids), self.ready)
            self.assertEqual(live.server_records(self.state, self.pin, self.ids), self.ready)
            live.server_barrier(self.args, self.state, self.pin, self.ids)
        self.assertEqual(read_private(Path(self.state['root']) / 'server-records.json'), canonical_bytes(self.ready))

    def test_bad_adapter_status_never_accepts_peer_map(self):
        for changes in ({'outcome': 'failed', 'error': 'barrier_timeout'}, {'role': ROLES[1]},
                        {'stage': 'identity-ready'}, {'format': True}, {'error': 'raw-secret'},
                        {'outcome': 'failed', 'error': 'raw-secret'}):
            self.peers(status=dict(self.status, **changes))
            with self.subTest(changes=changes), self.assertRaises(SmokeError):
                live.server_barrier(self.args, self.state, self.pin, self.ids)
        self.assertFalse((Path(self.state['root']) / 'server-records.json').exists())

    def test_server_barrier_rejects_late_readiness_before_live_expiry(self):
        self.peers()
        with patch.object(live, 'continuous_time', return_value=self.deadline - 1499), \
                patch.object(live.time, 'time', return_value=self.now), \
                self.assertRaisesRegex(SmokeError, 'deadline'):
            live.server_barrier(self.args, self.state, self.pin, self.ids)
        self.assertFalse((Path(self.state['root']) / 'server-records.json').exists())

    def test_swapped_or_changed_own_server_cannot_pass_barrier(self):
        for changed in ({}, {**self.ready, ROLES[0]: self.ready[ROLES[1]]}):
            self.peers(changed)
            with patch.object(live.time, 'time', return_value=self.now), self.assertRaises(SmokeError):
                live.server_barrier(self.args, self.state, self.pin, self.ids)
        changed = copy.deepcopy(self.ready)
        changed[ROLES[0]]['server_started_at'] -= 1
        self.peers(changed)
        with patch.object(live.time, 'time', return_value=self.now), self.assertRaises(SmokeError):
            live.server_barrier(self.args, self.state, self.pin, self.ids)

    def test_start_server_checks_published_runtime_identity(self):
        with patch.object(runtime, 'launch_server', return_value=self.ready[ROLES[0]]) as launch, \
                patch.object(live.time, 'time', return_value=self.now):
            live.start_server(self.state, self.pin, self.ids)
            launch.assert_called_once_with(self.state, self.ids)
        with patch.object(runtime, 'launch_server', return_value=self.ready[ROLES[1]]), \
                patch.object(live.time, 'time', return_value=self.now), self.assertRaises(SmokeError):
            live.start_server(self.state, self.pin, self.ids)

    def test_unexpected_client_driver_error_still_publishes_three_finite_failed_slots(self):
        with patch.object(runtime, 'run_clients', side_effect=RuntimeError('private exception')):
            with self.assertRaisesRegex(SmokeError, 'client_failed'):
                live.clients(self.state, self.pin, self.ids, self.ready)
        output = Path(self.state['output'])
        raw = read_private(output / 'client-result.json')
        self.assertEqual(raw, read_private(output / 'client-result/record.json'))
        self.assertNotIn(b'private exception', raw)
        report = strict_json(raw)
        runtime.validate_client_report(report)
        for slot in report['slots'].values():
            self.assertEqual(slot['category'], 'internal_failure')
            self.assertEqual(slot['status'], 'failed')
            self.assertIsNone(slot['metrics'])
        live.validate_report(report, self.state, self.pin, ROLES[0], self.ids, self.ready)

    def test_client_record_is_not_replaced_when_driver_wrote_then_failed(self):
        expected = runtime.failure_client_report(self.state, self.ids, self.ready, category='deadline')
        write_private(Path(self.state['output']) / 'client-result.json', expected)
        with patch.object(runtime, 'run_clients', side_effect=RuntimeError('private')):
            with self.assertRaises(SmokeError):
                live.clients(self.state, self.pin, self.ids, self.ready)
        self.assertEqual(read_private(Path(self.state['output']) / 'client-result/record.json'),
                         canonical_bytes(expected))

    def test_successful_clients_receive_original_deadline_and_publish_exact_report(self):
        report = runtime.failure_client_report(self.state, self.ids, self.ready)
        for slot in report['slots'].values():
            size = slot['payload']['stdout']['bytes'] + slot['payload']['stderr']['bytes']
            slot.update(status='received', category=None, exit=7,
                        verification=dict.fromkeys(('probe', 'stdout', 'stderr', 'exit'), True),
                        metrics={'cold_session_status_seconds': 1., 'command_wall_seconds': 10.,
                            'command_first_output_seconds': 2., 'output_drain_seconds': 4.,
                            'effective_download_bytes_per_second': size / 10,
                            'output_drain_bytes_per_second': size / 4,
                            **{stream + '_bytes': slot['payload'][stream]['bytes']
                               for stream in ('stdin', 'stdout', 'stderr')}})
        def run(*args, **kwargs):
            self.assertEqual(kwargs, {'deadline': self.deadline})
            write_private(Path(self.state['output']) / 'client-result.json', report)
            return report
        with patch.object(runtime, 'run_clients', side_effect=run):
            self.assertEqual(live.clients(self.state, self.pin, self.ids, self.ready), report)
        self.assertEqual(read_private(Path(self.state['output']) / 'client-result/record.json'),
                         canonical_bytes(report))

    def test_await_passes_original_deadline_and_rejects_malformed_closed_report(self):
        with patch.object(runtime, 'await_server', return_value={'secret': 'private'}) as wait:
            with self.assertRaises(SmokeError):
                live.await_server(self.state, self.pin, self.ids, self.ready)
            wait.assert_called_once_with(self.state, deadline=self.deadline)

    def test_missing_pin_has_finite_failure_status_and_scrubs_token(self):
        stdout = io.StringIO()
        with patch.object(live, 'context', return_value=CTX), \
                patch.object(live, 'public_directory', return_value=Path(self.state['output'])), \
                patch.dict(os.environ, {'GITHUB_TOKEN': 'never-native'}), patch('sys.stdout', stdout):
            code = live.main(['clients', '--state', str(self.args.state), '--role', ROLES[0]])
            self.assertNotIn('GITHUB_TOKEN', os.environ)
        self.assertEqual(code, 1)
        raw = read_private(Path(self.state['output']) / 'live-clients-result.json')
        self.assertNotIn(b'never-native', raw)
        self.assertIn(b'"error":"artifact_invalid"', raw)
        self.assertNotIn('Traceback', stdout.getvalue())

    def test_cleanup_does_not_load_pin_and_passes_runtime_callback(self):
        self.args.state.touch()
        checks = dict.fromkeys(('frontend_absent', 'mdk_absent', 'workers_absent',
            'caffeinate_absent', 'engine_cleanup_ready', 'uninstall_complete', 'root_removed'), True)
        result = {'outcome': 'complete', 'checks': checks, 'error': None, 'duration_ns': 1}
        with patch.object(live, 'context', return_value=CTX), \
                patch.object(prerequisite, 'public_directory', return_value=Path(self.state['output'])), \
                patch.object(prerequisite, 'load_state', return_value=self.state), \
                patch.object(live, 'load_pin', side_effect=AssertionError('must not resolve pin')), \
                patch.object(runtime, 'stop_supervisor') as stop, \
                patch.object(prerequisite, 'cleanup_state', return_value=result) as cleanup:
            self.assertEqual(live.main(['cleanup', '--state', str(self.args.state), '--role', ROLES[0]]), 0)
        cleanup.assert_called_once_with(self.state, runtime_cleanup=runtime.cleanup_runtime)
        stop.assert_called_once_with(self.state)
        record = strict_json(read_private(Path(self.state['output']) / 'cleanup-result.json'))
        self.assertEqual(record['pin_sha256'], self.state['pin_sha256'])
        self.assertEqual(record['checks'], checks)

    def test_cleanup_before_state_creation_records_not_started(self):
        with patch.object(prerequisite, 'public_directory', return_value=Path(self.state['output'])), \
                patch.object(runtime, 'stop_supervisor') as stop, \
                patch.object(prerequisite, 'cleanup_state') as uninstall:
            self.assertEqual(live.cleanup(self.args, CTX), 1)
        stop.assert_not_called()
        uninstall.assert_not_called()
        record = strict_json(read_private(Path(self.state['output']) / 'cleanup-result.json'))
        self.assertEqual(record['outcome'], 'not-started')
        self.assertEqual(record['duration_ns'], 0)
        self.assertIsNone(record['error'])

    def test_uncertain_supervisor_cleanup_prevents_uninstall_and_reports_incomplete(self):
        self.args.state.touch()
        with patch.object(prerequisite, 'public_directory', return_value=Path(self.state['output'])), \
                patch.object(prerequisite, 'load_state', return_value=self.state), \
                patch.object(runtime, 'stop_supervisor', side_effect=SmokeError('cleanup_incomplete')), \
                patch.object(prerequisite, 'cleanup_state') as uninstall:
            self.assertEqual(live.cleanup(self.args, CTX), 1)
        uninstall.assert_not_called()
        record = strict_json(read_private(Path(self.state['output']) / 'cleanup-result.json'))
        self.assertEqual(record['outcome'], 'incomplete')
        self.assertEqual(record['error'], 'cleanup_incomplete')
        self.assertFalse(any(record['checks'].values()))


if __name__ == '__main__':
    unittest.main()
