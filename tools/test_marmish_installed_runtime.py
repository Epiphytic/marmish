"""Driver evidence contracts; synthetic reports never count as live validation."""
import copy
import hashlib
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import Mock, patch

import marmish_installed_runtime as runtime
import marmish_installed_state as state_api
from marmish_installed_fixture import expected_receipt
from marmish_platform import continuous_time
from mdk_smoke import SmokeError
from test_marmish_nine_pair import successful_records


class ReportContracts(unittest.TestCase):
    def setUp(self):
        self.pin, self.records = successful_records()
        self.role = runtime.ROLES[0]
        self.client = self.records[self.role]['client-result']
        self.server = self.records[self.role]['server-result']

    def test_success_reports_and_nested_type_mutations(self):
        runtime.validate_client_report(self.client)
        runtime.validate_server_report(self.server)
        mutations = [
            ('client-result', lambda r: r.update(format=True)),
            ('client-result', lambda r: r.update(attempt='01')),
            ('client-result', lambda r: r.update(client='X' * 64)),
            ('client-result', lambda r: r['timing'].update(clock_valid=1)),
            ('client-result', lambda r: r['installation'].update(extra='x')),
            ('client-result', lambda r: r['slots'][self.role]['metrics'].update(stdin_bytes=True)),
            ('client-result', lambda r: r['slots'][self.role]['metrics'].update(command_wall_seconds=float('nan'))),
            ('client-result', lambda r: r['slots'][self.role]['payload']['stdin'].update(bytes=True)),
            ('client-result', lambda r: r.update(server_ready_sha256={})),
            ('server-result', lambda r: r.update(unexpected_submissions=True)),
            ('server-result', lambda r: r['lifetime'].update(limit_seconds=True)),
            ('server-result', lambda r: r['fixtures'][self.role]['provenance'].update(compiler=None)),
            ('server-result', lambda r: r['slots'][self.role].update(receipt_sha256='a' * 64)),
        ]
        for kind, mutate in mutations:
            value = copy.deepcopy(self.records[self.role][kind])
            mutate(value)
            with self.subTest(kind=kind, mutate=mutate), self.assertRaises(SmokeError):
                getattr(runtime, 'validate_' + kind.split('-')[0] + '_report')(value)

    def test_failure_has_no_metrics_and_success_maps_may_be_absent(self):
        for slot in self.client['slots'].values():
            slot.update(status='failed', category='native_failed', metrics=None)
        self.client['server_ready_sha256'] = {}
        runtime.validate_client_report(self.client)
        self.client['slots'][self.role]['metrics'] = {}
        with self.assertRaises(SmokeError):
            runtime.validate_client_report(self.client)
        for slot in self.server['slots'].values():
            slot.update(status='failed', category='native_failed', proof=None,
                        receipt=None, receipt_sha256=None)
        self.server.update(fixtures={}, policy_sha256=None, server_ready_sha256={})
        runtime.validate_server_report(self.server)

    def test_metrics_use_ns_and_combined_exact_stream_counts(self):
        payload = {'stdin': {'bytes': 8192}, 'stdout': {'bytes': 65536}, 'stderr': {'bytes': 8192}}
        probe = {'timing': {'clock_valid': True, 'elapsed_ns': 2_000_000_000}}
        command = {'timing': {'clock_valid': True, 'elapsed_ns': 10_000_000_000},
                   'first_output_ns': 2_000_000_000, 'last_output_ns': 6_000_000_000}
        metrics = runtime.command_metrics(probe, command, payload)
        self.assertEqual(metrics['cold_session_status_seconds'], 2)
        self.assertEqual(metrics['effective_download_bytes_per_second'], 7372.8)
        self.assertEqual(metrics['output_drain_bytes_per_second'], 18432)
        command['last_output_ns'] = command['first_output_ns']
        self.assertIsNone(runtime.command_metrics(probe, command, payload)['output_drain_bytes_per_second'])
        command['first_output_ns'] = None
        with self.assertRaises(SmokeError):
            runtime.command_metrics(probe, command, payload)

    def test_client_budget_reserves_watchdog_teardown_write_and_delivery(self):
        margin = runtime.CLIENT_WATCHDOG_SECONDS + runtime.CLIENT_TEARDOWN_SECONDS + runtime.CLIENT_WRITE_SECONDS
        self.assertEqual(runtime.client_deadline(0, 10000, 20000) + margin, 1080)
        self.assertEqual(runtime.client_deadline(0, 500, 20000) + margin + 120, 500)
        self.assertEqual(runtime.client_deadline(0, 10000, 500) + margin + 150, 500)


class DriverBoundary(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        installer = b'#!/bin/sh\nexit 0\n'
        self.state = state_api.create_state(self.base / 'state.json',
            {'run': '123', 'attempt': '2', 'sha': 'a' * 40}, 'b' * 64, runtime.ROLES[0],
            installer_bytes=installer, installer_sha256=hashlib.sha256(installer).hexdigest())
        self.root = Path(self.state['root'])
        self.addCleanup(lambda: shutil.rmtree(self.root, ignore_errors=True))
        self.pin, self.records = successful_records()
        self.identities = {r: self.records[r]['identity-ready'] for r in runtime.ROLES}
        self.servers = {r: self.records[r]['server-ready'] for r in runtime.ROLES}

    def test_pair_calls_only_cold_status_and_one_exact_byte_command(self):
        role = runtime.ROLES[1]
        pair, argv, stdin, out, err = runtime._pair_spec(self.state, self.identities, self.servers, role)
        responses = [
            {'returncode': 125, 'stdout': b'', 'stderr': b'marmish: not_found\n'},
            {'returncode': 7, 'stdout': out, 'stderr': err},
        ]
        for value in responses:
            value.update(timed_out=False, output_limited=False, cleanup='complete',
                timing={'clock_valid': True, 'elapsed_ns': 10_000_000_000},
                first_output_ns=2_000_000_000, last_output_ns=6_000_000_000)
        with patch.object(runtime, 'run_bounded', side_effect=responses) as run:
            slot = runtime.run_client_pair(self.state, self.identities, self.servers, role,
                                           deadline=continuous_time() + 300)
        self.assertEqual(slot['status'], 'received')
        self.assertEqual(run.call_count, 2)
        self.assertEqual(run.call_args_list[0].args[0][-2:], ['--status', pair['probe_invocation']])
        self.assertEqual(run.call_args_list[1].args[0][-len(argv):], argv)
        self.assertEqual(run.call_args_list[1].kwargs['stdin'], stdin)
        self.assertIn(b'\xff', argv)
        self.assertIn(b'$(id)', argv)
        self.assertIn(b'line\nbreak', argv)

    def test_failed_probe_executes_no_command(self):
        result = dict(returncode=125, stdout=b'', stderr=b'marmish: unauthorized\n',
                      timed_out=False, output_limited=False, cleanup='complete')
        with patch.object(runtime, 'run_bounded', return_value=result) as run:
            slot = runtime.run_client_pair(self.state, self.identities, self.servers,
                runtime.ROLES[0], deadline=continuous_time() + 300)
        self.assertEqual(run.call_count, 1)
        self.assertEqual(slot['category'], 'probe_failed')
        self.assertIsNone(slot['metrics'])

    def test_watchdog_requires_cleanup_and_skips_remaining_pairs_if_unproven(self):
        response = dict(returncode=-9, stdout=b'', stderr=b'', timed_out=True,
                        output_limited=False, cleanup='incomplete')
        with patch.object(runtime, 'run_bounded', return_value=response) as run, patch.object(runtime, '_client_cleanup', side_effect=SmokeError('cleanup_incomplete')) as cleanup:
            report = runtime.run_clients(self.state, self.identities, self.servers)
        self.assertEqual(run.call_count, 1)
        cleanup.assert_called_once()
        self.assertEqual(report['slots'][runtime.ROLES[0]]['category'], 'cleanup_incomplete')
        self.assertTrue(all(report['slots'][role]['status'] == 'skipped' for role in runtime.ROLES[1:]))

    def test_watchdog_recovered_client_can_continue_without_claiming_pair_success(self):
        response = dict(returncode=-9, stdout=b'', stderr=b'', timed_out=True,
                        output_limited=False, cleanup='incomplete')
        with patch.object(runtime, 'run_bounded', return_value=response) as run, patch.object(runtime, '_client_cleanup') as cleanup:
            report = runtime.run_clients(self.state, self.identities, self.servers)
        self.assertEqual((run.call_count, cleanup.call_count), (3, 3))
        self.assertTrue(all(slot['status'] == 'timed_out' for slot in report['slots'].values()))

    def test_client_cleanup_is_scoped_to_new_client_markers_and_proves_profile_lock(self):
        home = self.root / 'client/mdk'
        home.mkdir(parents=True, mode=0o700)
        server_identity, client_identity = {'pid': 100}, {'pid': 101}
        records = [{'identity': server_identity, 'execution_id': 'server-marker'},
                   {'identity': client_identity, 'execution_id': 'client-marker'}]
        lock = Mock(__enter__=Mock(), __exit__=Mock(return_value=False))
        with patch.object(runtime.sys, 'platform', 'linux'), patch.object(state_api, '_registry_records', return_value=records), patch.object(state_api, '_process_absent', return_value=True) as absent, patch.object(runtime, '_clean_linux') as clean, patch('marmish_transport.acquire_profile_home_lock', return_value=lock) as acquire:
            runtime._client_cleanup(self.state, [server_identity])
        absent.assert_called_once_with(client_identity)
        clean.assert_called_once_with(self.state, 'client-marker')
        acquire.assert_called_once_with(home)
        lock.__enter__.assert_called_once()
        lock.__exit__.assert_called_once()

    def test_client_cleanup_rejects_live_frontend_before_native_recovery(self):
        records = [{'identity': {'pid': 101}, 'execution_id': 'client-marker'}]
        with patch.object(state_api, '_registry_records', return_value=records), patch.object(state_api, '_process_absent', return_value=False), patch.object(runtime, '_clean_linux') as clean:
            with self.assertRaisesRegex(SmokeError, 'cleanup_incomplete'):
                runtime._client_cleanup(self.state, [])
        clean.assert_not_called()

    def test_owned_supervisor_graceful_sigint_and_identity_record(self):
        account = 'a' * 64
        source = ('import json,signal,time,sys;'
                  'signal.signal(signal.SIGINT,lambda *_:sys.exit(0));'
                  'print(json.dumps({"ready":True,"account":"' + account + '"}),flush=True);'
                  'time.sleep(30)')
        daemon = runtime.OwnedServer(self.state, [sys.executable, '-I', '-B', '-c', source])
        self.addCleanup(lambda: daemon.close() if daemon.child.returncode is None else None)
        until = continuous_time() + 3
        while not daemon.ready(account):
            self.assertLess(continuous_time(), until)
            daemon.tick()
        daemon.close()
        record = runtime.strict_json(state_api.read_private(daemon.record_path))
        self.assertEqual(record['returncode'], 0)
        self.assertEqual(record['cleanup'], 'complete')
        self.assertTrue(record['finished'])
        self.assertTrue(state_api._process_absent(daemon.identity))

    def test_native_setup_failure_publishes_all_three_failed_slots(self):
        with patch('marmish_installed_fixture.build_fixture', side_effect=SmokeError('build_failed')):
            report = runtime.serve(self.state, self.identities)
        self.assertEqual(report['reason'], 'native-failed')
        self.assertEqual(set(report['slots']), set(runtime.ROLES))
        self.assertTrue(all(slot['status'] == 'failed' and slot['proof'] is None
                            for slot in report['slots'].values()))
        written = runtime.strict_json(state_api.read_private(Path(self.state['output']) / 'server-result.json'))
        self.assertEqual(written, report)
        live = runtime.strict_json(state_api.read_private(self.root / 'live-start.json'))
        self.assertLessEqual(live['deadline'], live['start']['continuous_ns'] / 1e9 + 1800)
        self.assertLessEqual(live['readiness_deadline'], live['start']['continuous_ns'] / 1e9 + 300)

    def test_rpc_failure_is_published_with_finite_native_category(self):
        with patch('marmish_installed_fixture.build_fixture', side_effect=runtime.RpcError('socket_failed')):
            report = runtime.serve(self.state, self.identities)
        self.assertEqual(report['reason'], 'native-failed')
        self.assertTrue(all(slot['category'] == 'native_failed' for slot in report['slots'].values()))

    def test_missing_server_records_produce_three_explicit_failed_client_slots(self):
        report = runtime.failure_client_report(self.state, self.identities)
        self.assertEqual(report['server_ready_sha256'], {})
        self.assertTrue(all(slot['payload'] is None and slot['metrics'] is None
                            for slot in report['slots'].values()))
        runtime.validate_client_report(report)

    def test_busy_native_worker_guard_never_removes_engine_or_claims_absence(self):
        engine = self.root / 'policy-engine'
        engine.mkdir(mode=0o700)
        state_api.write_private(self.root / 'scratch-policy.json', {'state_root': str(engine)})
        executable = Path(self.state['prefix']) / 'current/libexec/marmish/marmish-engine-bridge'
        executable.parent.mkdir(parents=True)
        executable.touch()
        bridge = Mock()
        bridge.call.side_effect = SmokeError('busy')
        context = Mock(__enter__=Mock(return_value=bridge), __exit__=Mock(return_value=False))
        with patch.object(runtime, '_registered_bridge', return_value=context), patch.object(runtime, 'continuous_time', side_effect=[0, 100000]), patch('shutil.rmtree') as remove:
            with self.assertRaisesRegex(SmokeError, 'cleanup_incomplete'):
                runtime.cleanup_runtime(self.state)
        bridge.call.assert_called_once_with('cleanup_ready')
        remove.assert_not_called()
        self.assertTrue(engine.exists())

    def test_lifetime_alarm_uses_serve_deadline_and_masks_report_cleanup(self):
        lifecycle = {}
        events = []
        real_write = runtime.write_private
        def write(path, value):
            if path.name == 'server-result.json':
                events.append('report')
            return real_write(path, value)
        def mask(how, _values):
            events.append('blocked' if how == runtime.signal.SIG_BLOCK else 'restored')
            return set()
        with patch('marmish_installed_fixture.build_fixture', side_effect=SmokeError('deadline')), patch.object(runtime, 'continuous_time', return_value=self.state['overall_deadline']), patch.object(runtime.signal, 'setitimer') as timer, patch.object(runtime.signal, 'pthread_sigmask', side_effect=mask), patch.object(runtime, 'write_private', side_effect=write):
            report = runtime.serve(self.state, self.identities, arm_lifetime=True, lifecycle=lifecycle)
        self.assertEqual(report['reason'], 'lifetime-cap')
        self.assertEqual(lifecycle['cleanup'], 'complete')
        self.assertEqual(events, ['blocked', 'report', 'restored'])
        self.assertEqual(timer.call_args_list[0].args, (runtime.signal.ITIMER_REAL, .001))
        self.assertEqual(timer.call_args_list[1].args, (runtime.signal.ITIMER_REAL, 0))

    def test_waiter_allows_bounded_terminal_report_after_live_cap(self):
        path = Path(self.state['output']) / 'server-result.json'
        state_api.write_private(path, {'finished': True})
        with patch.object(state_api, '_registry_records', return_value=[{'kind': 'supervisor', 'identity': {}}]), \
                patch.object(state_api, '_process_absent', return_value=True), \
                patch.object(runtime, 'continuous_time', return_value=1005), \
                patch.object(runtime, 'validate_server_report', side_effect=lambda value: value), \
                patch.object(runtime, 'stop_supervisor') as stop:
            self.assertEqual(runtime.await_server(self.state, deadline=1000), {'finished': True})
        stop.assert_not_called()
        path.unlink()
        with patch.object(state_api, '_registry_records', return_value=[{'kind': 'supervisor', 'identity': {}}]), \
                patch.object(runtime, 'continuous_time', return_value=1031), \
                patch.object(runtime, 'stop_supervisor') as stop:
            with self.assertRaisesRegex(SmokeError, 'deadline'):
                runtime.await_server(self.state, deadline=1000)
        stop.assert_called_once_with(self.state)

    def test_receipt_reconciliation_uses_native_receipt_directory_and_canonical_digest(self):
        role = runtime.ROLES[0]
        fixtures = {}
        proofs = {}
        (self.root / 'r').mkdir(mode=0o700)
        report = copy.deepcopy(self.records[role]['server-result'])
        for client in runtime.ROLES:
            slot = report['slots'][client]
            receipt = slot['receipt']
            path = self.root / 'r' / (client + '.json')
            # Non-canonical whitespace is accepted only after exact object comparison.
            import json
            path.write_text(json.dumps(receipt, indent=2))
            path.chmod(0o600)
            fixtures[client] = dict(report['fixtures'][client], receipt=str(path))
            proofs[self.identities[client]['client']] = slot['proof']
        class Observer:
            def proof_for(self, peer):
                return proofs[peer]
        clients = {r: self.records[r]['client-result'] for r in runtime.ROLES}
        # The receipts bind the original server-ready executable paths.
        runtime._reconcile_server(self.state, self.identities, fixtures, Observer(), clients, report)
        self.assertTrue(all(s['status'] == 'completed' for s in report['slots'].values()))
        self.assertTrue(all(s['receipt_sha256'] == runtime.digest(s['receipt'])
                            for s in report['slots'].values()))
        state_api.write_private(self.root / 'r' / 'extra.json', {})
        with self.assertRaises(SmokeError):
            runtime._reconcile_server(self.state, self.identities, fixtures, Observer(), clients, report)
        self.assertTrue(all(s['status'] == 'completed' for s in report['slots'].values()))
        (self.root / 'r' / 'extra.json').unlink()
        missing = runtime.ROLES[-1]
        Path(fixtures[missing]['receipt']).unlink()
        report['slots'] = {r: runtime._server_slot(self.identities, r, role) for r in runtime.ROLES}
        runtime._reconcile_server(self.state, self.identities, fixtures, Observer(), clients, report)
        self.assertEqual([s['status'] for s in report['slots'].values()], ['completed', 'completed', 'failed'])


if __name__ == '__main__':
    unittest.main()
