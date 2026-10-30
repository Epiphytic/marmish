"""Offline harness contract checks; never connect to a provider or relay."""
import tempfile
import signal
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

from marmish_e2e import Harness
from marmish_protocol import b64, save_json
from mdk_smoke import SmokeError


class NativeHarnessTests(unittest.TestCase):
    def harness(self, root, storage='auto', migrate=False):
        args = SimpleNamespace(timeout=900, key_storage=storage, migrate_acceptance=migrate,
            keychain_helper=Path('/private/helper'), key_catalog=root / 'catalog.json',
            key_name_prefix='owned', relay=['wss://relay.example/'], wnd=Path('/private/wnd'),
            wnd_sha256='a' * 64, wnd_manifest=None, worker=Path('/private/worker'),
            bridge=Path('/private/bridge'))
        with patch('marmish_e2e.active_context', return_value=MagicMock()):
            return Harness(args, root, {'assertions': []})

    def test_native_context_required(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            args = SimpleNamespace(timeout=900, key_storage='auto', migrate_acceptance=False)
            with patch('marmish_e2e.active_context', return_value=None):
                with self.assertRaisesRegex(SmokeError, '^owned_native_context_required$'):
                    Harness(args, root, {})

    def test_profile_cli_storage_and_names_agree(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self.harness(Path(directory))
            self.assertEqual(harness.profile_args('client').key_name, 'owned-client')
            argv = harness.common_args('client')
            self.assertEqual(argv[argv.index('--key-storage') + 1], 'auto')
            self.assertEqual(argv[argv.index('--key-name') + 1], 'owned-client')
            self.assertIn('--keychain-helper', argv)
            harness.storage = 'files'
            self.assertIsNone(harness.profile_args('client').key_name)
            self.assertNotIn('--key-name', harness.common_args('client'))

    def test_migration_starts_files_and_supplies_server_guard_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            harness = self.harness(root, migrate=True)
            self.assertEqual(harness.storage, 'files')
            save_json(root / 'command-policy.json', {'version': 1})
            harness.storage = 'native'
            harness.client_identity = 'b' * 64
            server = harness.migration_args('server')
            self.assertTrue(server.server_mode and server.migrate_keys_to_native)
            self.assertEqual(server.allow, ['b' * 64])
            self.assertEqual(server.loaded_command_policy, {'version': 1})
            self.assertFalse(harness.migration_args('client').server_mode)

    def test_locked_restarts_require_both_refusals(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            harness = self.harness(root)
            for name in ('server', 'client'):
                save_json(root / name / 'profile.json', {'keys': {'backend': 'native'}})
            with patch('marmish_e2e.Profile', side_effect=SmokeError('locked_or_denied')) as profile:
                harness.check_locked_restarts()
                self.assertEqual(profile.call_count, 2)
            with patch('marmish_e2e.Profile', side_effect=SmokeError('unavailable')):
                with self.assertRaisesRegex(SmokeError, '^locked_frontend_wrong_error$'):
                    harness.check_locked_restarts()

    def test_native_cleanup_uses_bound_master_without_daemon(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            harness = self.harness(root)
            save_json(root / 'server/profile.json', {'account': 'b' * 64,
                                                   'keys': {'backend': 'native'}})
            master = SimpleNamespace(root_id='c' * 64,
                reference={'version': 1, 'purpose': 'sqlcipher', 'id': 'd' * 64}, secret=b'x' * 32)
            with patch('marmish_e2e.KeySession') as session, \
                    patch('marmish_e2e.remove_engine_state') as remove, \
                    patch('marmish_e2e.Profile') as profile:
                keys = session.return_value.__enter__.return_value
                keys.prepare_master.return_value = master
                harness.remove_engine({'version': 1, 'state_root': str(root / 'server/engine')})
                policy = remove.call_args.args[1]
                self.assertEqual(policy['version'], 2)
                self.assertEqual(policy['storage'], {'mode': 'native', 'root_id': master.root_id,
                    'reference': master.reference, 'master': b64(master.secret)})
                keys.prepare_nostr.assert_called_once_with(existing_account='b' * 64)
                profile.assert_not_called()

    def test_running_audit_requires_live_worker_and_loads_owned_references(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self.harness(Path(directory))
            harness.server = SimpleNamespace(pid=123)
            reference = {'version': 1, 'purpose': 'sqlcipher', 'id': 'd' * 64}
            harness.native.references.return_value = [reference]
            with patch('marmish_e2e.NativeHelper') as helper, \
                    patch('marmish_e2e.audit_tree', return_value={'workers': 1}) as audit:
                helper.return_value.load.return_value = b'x' * 32
                harness.audit_running_native_worker()
                helper.return_value.load.assert_called_once_with(reference)
                audit.assert_called_once_with(123, [b'x' * 32], require_worker=True)
                self.assertEqual(harness.report['native_process_audit'], {'workers': 1})

    def test_native_identity_check_includes_server_and_client(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self.harness(Path(directory))
            harness.server_identity, harness.client_identity = 'a' * 64, 'b' * 64
            server, profile = MagicMock(), MagicMock()
            for rpc, identity in ((server, harness.server_identity),
                                  (profile.rpc, harness.client_identity)):
                rpc.capabilities.return_value = {'version': 1, 'secret_store': 'MarmishStdin',
                                                 'public_key': identity}
                rpc.accounts.return_value = [identity]
            with patch.object(harness, 'server_rpc', return_value=server):
                harness.check_native_identities(profile)
                server.accounts.return_value = ['c' * 64]
                with self.assertRaisesRegex(SmokeError, '^native_endpoint_identity_capability_mismatch$'):
                    harness.check_native_identities(profile)

    def test_running_audit_also_inspects_client_daemon_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self.harness(Path(directory))
            harness.server = SimpleNamespace(pid=123)
            profile = SimpleNamespace(transport=SimpleNamespace(process=SimpleNamespace(pid=456)))
            harness.native.references.return_value = []
            with patch('marmish_e2e.NativeHelper'), \
                    patch('marmish_e2e.audit_tree', return_value={'daemons': 1}) as audit:
                harness.audit_running_native_worker(profile)
                self.assertEqual(audit.call_args_list[1].args, (456, []))
                self.assertEqual(harness.report['native_client_process_audit'], {'daemons': 1})

    def test_cleanup_failure_retains_provider_and_local_state(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            harness = self.harness(root)
            with patch.object(harness, 'stop_server', side_effect=SmokeError('owned_stop_failed')):
                harness.cleanup()
            harness.native.retain_private_state.assert_called_once_with()
            self.assertTrue(root.exists())
            self.assertEqual(harness.report['local_cleanup'], 'incomplete')
            self.assertEqual(harness.report['outcome'], 'failed')

    def _migration_preflight(self, root):
        harness = self.harness(root, migrate=True)
        harness.server_identity, harness.client_identity = 'a' * 64, 'b' * 64
        harness.migration_reserved = {'master': {'root_id': 'c' * 64,
            'reference': {'version': 1, 'purpose': 'sqlcipher', 'id': 'd' * 64}}}
        engine = root / 'server/engine'
        engine.mkdir(parents=True, mode=0o700)
        (root / 'server').chmod(0o700)
        (engine / 'engine.key').write_bytes(b'MRMSKEY1' + b'x' * 32)
        (engine / 'engine.key').chmod(0o600)
        save_json(root / 'command-policy.json', {'version': 1})
        return harness

    def test_migration_preflight_retries_busy_authenticates_and_releases_without_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self._migration_preflight(Path(directory))
            guard = MagicMock()
            with patch('marmish_e2e.EngineBridge', side_effect=[SmokeError('busy'), guard]) as bridge:
                harness.quiesce_migration()
                self.assertEqual(bridge.call_count, 2)
                policy = bridge.call_args.args[1]
                self.assertEqual(policy['storage']['mode'], 'migrate_native')
                self.assertEqual(policy['storage']['master'], b64(b'x' * 32))
                self.assertEqual(policy['storage']['reference'], harness.migration_reserved['master']['reference'])
                guard.call.assert_not_called()
                guard.close.assert_called_once_with()

    def test_migration_preflight_preserves_nonbusy_failure_and_refuses_live_frontend(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self._migration_preflight(Path(directory))
            with patch('marmish_e2e.EngineBridge', side_effect=SmokeError('storage_failure')) as bridge:
                with self.assertRaisesRegex(SmokeError, '^storage_failure$'):
                    harness.quiesce_migration()
                bridge.assert_called_once()
            harness.server = MagicMock()
            with patch('marmish_e2e.EngineBridge') as bridge:
                with self.assertRaisesRegex(SmokeError, '^migration_frontend_still_running$'):
                    harness.quiesce_migration()
                bridge.assert_not_called()

    def test_migration_preflight_deadline_restores_outer_alarm(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self._migration_preflight(Path(directory))
            handler = signal.getsignal(signal.SIGALRM)
            def timeout(*_args):
                signal.getsignal(signal.SIGALRM)(signal.SIGALRM, None)
            with patch('marmish_e2e.EngineBridge', side_effect=timeout), \
                    patch('marmish_e2e.signal.getitimer', return_value=(100, 0)), \
                    patch('marmish_e2e.signal.setitimer') as timer:
                with self.assertRaisesRegex(SmokeError, '^migration_quiescence_timeout$'):
                    harness.quiesce_migration()
                self.assertLessEqual(timer.call_args_list[0].args[1], 20)
                self.assertEqual(timer.call_args_list[-2].args[1], 0)
                self.assertGreater(timer.call_args_list[-1].args[1], 0)
            self.assertEqual(signal.getsignal(signal.SIGALRM), handler)

    def test_migration_quiesces_before_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            harness = self.harness(Path(directory), migrate=True)
            events = []
            with patch.object(harness, 'quiesce_migration', side_effect=lambda: events.append('quiescent')), \
                    patch.object(harness, 'encrypted_snapshot', side_effect=SmokeError('snapshot_stop')):
                with self.assertRaisesRegex(SmokeError, '^snapshot_stop$'):
                    harness.migrate_profiles()
            self.assertEqual(events, ['quiescent'])

    def test_snapshot_tracks_ciphertext_and_spool_not_key_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            harness = self.harness(root)
            engine = root / 'server/engine'
            engine.mkdir(parents=True)
            (engine / 'engine.sqlite3').write_bytes(b'ciphertext')
            (engine / 'engine.sqlite3-wal').write_bytes(b'housekeeping')
            (engine / 'engine.sqlite3-shm').write_bytes(b'housekeeping')
            (engine / 'engine.native.json').write_bytes(b'public marker')
            (root / 'client/mdk').mkdir(parents=True)
            (root / 'client/mdk/state.db').write_bytes(b'MDK housekeeping')
            (engine / 'jobs').mkdir()
            (engine / 'jobs/output').write_bytes(b'encrypted spool')
            before = harness.encrypted_snapshot()
            self.assertEqual(len(before), 2)
            (engine / 'engine.native.json').write_bytes(b'changed marker')
            self.assertEqual(harness.encrypted_snapshot(), before)
            (engine / 'jobs/output').write_bytes(b'changed ciphertext')
            self.assertNotEqual(harness.encrypted_snapshot(), before)


if __name__ == '__main__':
    unittest.main()
