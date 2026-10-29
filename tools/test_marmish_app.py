"""Offline frontend checks; no daemon, namespace, relay or command is started."""
import copy
import hashlib
import io
import os
from pathlib import Path
import tempfile
import time
from marmish_platform import continuous_time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import marmish_app as app
from marmish_protocol import b64, encode, envelope, load_json, save_json
from mdk_smoke import SmokeError, public_key

CLIENT, SERVER, GROUP, SID = '11' * 32, '22' * 32, '33' * 32, '44' * 16
INVOCATION, TOKEN, NEW_TOKEN = '55' * 16, '66' * 32, '77' * 32
RELAYS = ['wss://relay.example/']
NPUB = 'npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6'
COMMAND = {'mode': 'exec', 'argv': [b64(b'/bin/echo'), b64(b'a unicode\xe2\x98\x83 \"quoted\" $HOME `literal`')]}


class DestinationTests(unittest.TestCase):
    def test_explicit_destination_and_u_semantics(self):
        self.assertEqual(app.destination(CLIENT + '@relay.example', None, RELAYS), CLIENT)
        self.assertEqual(app.destination(CLIENT + '@wss://relay.example/', None, RELAYS), CLIENT)
        self.assertEqual(app.destination(NPUB + '@relay.example', None, RELAYS), public_key(NPUB))
        self.assertEqual(app.destination('relay.example', NPUB, RELAYS), public_key(NPUB))
        for destination, user in ((CLIENT + '@relay.example', CLIENT),
                                  (CLIENT + '@other.example', None),
                                  ('relay.example', None),
                                  (CLIENT + '@ws://relay.example', None),
                                  (CLIENT + '@relay.example/path', None)):
            with self.subTest(destination=destination, user=user), self.assertRaises(SmokeError):
                app.destination(destination, user, RELAYS)

    def test_profile_relay_mismatch_fails_before_daemon_construction(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            save_json(root / 'profile.json', {'version': 1, 'account': CLIENT,
                      'relays': RELAYS, 'sessions': {}, 'server_groups': {}})
            args = SimpleNamespace(identity=root, relay=['wss://other.example'],
                                   wnd=Path('/not-executed'), wnd_sha256='00' * 32)
            with patch.object(app, 'ConfinedDaemon') as daemon:
                with self.assertRaisesRegex(SmokeError, 'profile_relay_mismatch'):
                    app.Profile(args)
                daemon.assert_not_called()
                args.relay = ['wss://RELAY.example:443/']
                profile = app.Profile(args)
                self.assertEqual(profile.relays, RELAYS)
                daemon.assert_not_called() # Keys and bootstrap are selected on context entry.


class NativeProfileTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        os.chmod(self.root, 0o700)
        self.helper = Mock()
        self.helper.probe.return_value = None
        self.helper.derive_nostr.return_value = CLIENT
        self.helper.secrets = {}
        def load(reference):
            if reference['id'] not in self.helper.secrets:
                raise SmokeError('missing')
            return self.helper.secrets[reference['id']]
        self.helper.load.side_effect = load
        self.helper.create.side_effect = lambda ref, label, secret: self.helper.secrets.__setitem__(ref['id'], secret)
        self.args = SimpleNamespace(identity=self.root / 'profile', relay=RELAYS,
            wnd=Path('/not-executed'), wnd_sha256='00' * 32, key_storage='auto',
            key_name='work', keychain_helper=self.helper, key_catalog=self.root / 'catalog/keys.json')

    def test_binding_published_before_daemon_and_verified_on_restart(self):
        rpc = Mock()
        rpc.capabilities.return_value = {'version': 1, 'secret_store': 'MarmishStdin', 'public_key': CLIENT}
        rpc.accounts.return_value = []
        rpc.login.return_value = CLIENT
        def start():
            saved = load_json(self.args.identity / 'profile.json')
            self.assertEqual(saved['account'], CLIENT)
            self.assertFalse(saved['initialized'])
            self.assertEqual(saved['keys']['backend'], 'native')
        with patch.object(app, 'ConfinedDaemon') as daemon, patch.object(app, 'MdkRpc', return_value=rpc):
            daemon.return_value.start.side_effect = start
            with app.Profile(self.args) as profile:
                self.assertEqual(profile.account, CLIENT)
                frame = daemon.call_args.kwargs['bootstrap']
                self.assertEqual(len(frame), 72)
                self.assertEqual(frame[:8], b'MRMNST01')
            self.assertTrue(load_json(self.args.identity / 'profile.json')['initialized'])
            rpc.login.assert_called_once()
        # Existing initialized metadata is mandatory; deleting it cannot import anew.
        with patch.object(app, 'ConfinedDaemon') as daemon:
            with self.assertRaisesRegex(SmokeError, 'missing_account_state'):
                with app.Profile(self.args):
                    pass
            daemon.assert_not_called()

    def test_wrong_capability_fails_before_login_and_closes(self):
        rpc = Mock()
        rpc.capabilities.return_value = {'public_key': SERVER}
        with patch.object(app, 'ConfinedDaemon') as daemon, patch.object(app, 'MdkRpc', return_value=rpc):
            with self.assertRaisesRegex(SmokeError, 'account_mismatch'):
                with app.Profile(self.args):
                    pass
            rpc.login.assert_not_called()
            daemon.return_value.close.assert_called_once()
        self.assertFalse(load_json(self.args.identity / 'profile.json')['initialized'])

    def _pending_native_login(self):
        from marmish_protocol import private_directory
        rpc = Mock()
        rpc.capabilities.return_value = {'public_key': CLIENT}
        rpc.accounts.return_value = []
        rpc.login.side_effect = SmokeError('interrupted_login')
        with patch.object(app, 'ConfinedDaemon'), patch.object(app, 'MdkRpc', return_value=rpc):
            with self.assertRaisesRegex(SmokeError, 'interrupted_login'):
                with app.Profile(self.args):
                    pass
        home = private_directory(self.args.identity / 'mdk')
        accounts = private_directory(home / 'accounts')
        account = private_directory(accounts / CLIENT)
        save_json(account / 'account.json', {'label': CLIENT, 'account_id_hex': CLIENT,
            'local_signing': True, 'external_signing': False, 'signed_out': False})
        save_json(account / '.account-setup.json', {'account_id_hex': CLIENT,
            'reused_account_id_credential': False, 'kind': 'imported_identity',
            'phase': 'key_package_publication_started'})
        rpc.accounts.return_value = [CLIENT]
        rpc.login.reset_mock()
        return rpc

    def test_pending_setup_resumes_login_even_when_whoami_has_account(self):
        rpc = self._pending_native_login()
        secret_count = len(self.helper.secrets)
        def login(nsec, relays):
            self.assertFalse(load_json(self.args.identity / 'profile.json')['initialized'])
            from marmish_keys import encode_nsec
            self.assertEqual(nsec, encode_nsec(next(iter(self.helper.secrets.values()))))
            self.assertEqual(relays, RELAYS)
            return CLIENT
        rpc.login.side_effect = login
        with patch.object(app, 'ConfinedDaemon'), patch.object(app, 'MdkRpc', return_value=rpc):
            with app.Profile(self.args) as profile:
                self.assertTrue(profile.value['initialized'])
        rpc.login.assert_called_once()
        self.assertEqual(len(self.helper.secrets), secret_count)
        self.assertTrue(load_json(self.args.identity / 'profile.json')['initialized'])

    def test_pending_setup_failed_login_keeps_uninitialized(self):
        rpc = self._pending_native_login()
        rpc.login.side_effect = SmokeError('setup_publication_failed')
        # Even stale initialized metadata cannot override the MDK setup journal.
        value = load_json(self.args.identity / 'profile.json')
        value['initialized'] = True
        save_json(self.args.identity / 'profile.json', value)
        with patch.object(app, 'ConfinedDaemon') as daemon, patch.object(app, 'MdkRpc', return_value=rpc):
            with self.assertRaisesRegex(SmokeError, 'setup_publication_failed'):
                with app.Profile(self.args):
                    pass
            daemon.return_value.close.assert_called_once()
        rpc.login.assert_called_once()
        self.assertFalse(load_json(self.args.identity / 'profile.json')['initialized'])

    def _legacy_server(self):
        from marmish_protocol import private_directory
        root = private_directory(self.args.identity)
        home = private_directory(root / 'mdk')
        accounts = private_directory(home / 'accounts')
        account = private_directory(accounts / CLIENT)
        save_json(account / 'account.json', {'label': CLIENT, 'account_id_hex': CLIENT,
            'local_signing': True, 'external_signing': False, 'signed_out': False})
        save_json(account / 'secret.json', {'version': 1, 'backend': 'local-dev-file',
                                           'secret_key_hex': '11' * 32})
        engine = private_directory(root / 'engine')
        (engine / 'engine.key').write_bytes(b'MRMSKEY1' + b'2' * 32)
        (engine / 'engine.key').chmod(0o600)
        save_json(root / 'profile.json', {'version': 1, 'account': CLIENT,
            'relays': RELAYS, 'sessions': {}, 'server_groups': {}})
        self.args.__dict__.update(server_mode=True, migrate_keys_to_native=True,
            allow=[SERVER], worker=Path('/worker'), bridge=Path('/bridge'),
            cwd=self.root, allow_shell=False, loaded_command_policy=None)
        return account / 'secret.json', engine / 'engine.key'

    def test_migration_quiesces_before_create_and_commits_before_cleanup(self):
        nostr_file, master_file = self._legacy_server()
        original = self.helper.create.side_effect
        events = []
        def create(*args):
            self.assertEqual(events, ['quiescent'])
            original(*args)
        self.helper.create.side_effect = create
        def bridge(_executable, policy):
            self.assertEqual(policy['storage']['mode'], 'migrate_native')
            self.assertTrue(nostr_file.exists() and master_file.exists())
            events.append('quiescent')
            guard = Mock()
            def commit(op):
                self.assertEqual(op, 'migration_commit')
                self.assertEqual(load_json(self.args.identity / 'profile.json')['keys']['backend'], 'native')
                self.assertTrue(nostr_file.exists())
                master_file.unlink()
            guard.call.side_effect = commit
            return guard
        with patch.object(app, 'EngineBridge', side_effect=bridge), patch.object(app, 'ConfinedDaemon') as daemon:
            with app.Profile(self.args) as profile:
                self.assertTrue(profile.migrated)
                self.assertEqual(profile.account, CLIENT)
            daemon.assert_not_called()
        self.assertEqual(len(self.helper.secrets), 2)
        self.assertFalse(nostr_file.exists() or master_file.exists())
        self.assertFalse((self.args.identity / 'keys.pending.json').exists())

    def test_migration_busy_keeps_originals_and_creates_no_native_secret(self):
        nostr_file, master_file = self._legacy_server()
        original_profile = (self.args.identity / 'profile.json').read_bytes()
        with patch.object(app, 'EngineBridge', side_effect=SmokeError('busy')):
            with self.assertRaisesRegex(SmokeError, 'busy'):
                with app.Profile(self.args):
                    pass
        self.assertTrue(nostr_file.exists() and master_file.exists())
        self.assertEqual((self.args.identity / 'profile.json').read_bytes(), original_profile)
        self.assertFalse(self.helper.secrets)

    def test_unbound_nonempty_mdk_never_generates_new_identity(self):
        from marmish_protocol import private_directory
        private_directory(self.args.identity)
        home = private_directory(self.args.identity / 'mdk')
        (home / 'state').write_bytes(b'old state')
        with patch.object(app, 'ConfinedDaemon') as daemon:
            with self.assertRaisesRegex(SmokeError, 'nonpristine_key_state'):
                with app.Profile(self.args):
                    pass
            daemon.assert_not_called()



class ClientTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.client = app.Client.__new__(app.Client)
        self.client.profile = SimpleNamespace(root=Path(self.temp.name), account=CLIENT)
        self.client.peer, self.client.group, self.client.sid = SERVER, GROUP, SID
        self.client.deadline = continuous_time() + 30
        self.client.call = Mock()

    def journal(self, command=COMMAND, token=TOKEN):
        value = {'server': SERVER, 'session': SID, 'group': GROUP,
                 'invocation': INVOCATION, 'command': copy.deepcopy(command), 'token': token}
        save_json(self.client.journal_path(INVOCATION), value)
        return value

    def test_default_session_reuses_bound_room_and_explicit_new_creates_group(self):
        key = hashlib.sha256(encode({'peer': SERVER, 'relays': RELAYS})).hexdigest()
        rpc = Mock()
        rpc.history.return_value = []
        rpc.members.return_value = [
            {'member_id': CLIENT, 'npub': CLIENT, 'local': True},
            {'member_id': SERVER, 'npub': SERVER, 'local': False}]
        new_group, new_sid = 'aa' * 32, 'bb' * 16
        rpc.show_group.side_effect = lambda account, group: {'group': {'profile': {
            'name': 'marmish/v1/' + (SID if group == GROUP else new_sid),
            'description': 'marmish/v1 command transport'}}}
        rpc.create_group.return_value = {'group_id': new_group}
        profile = SimpleNamespace(root=self.client.profile.root, rpc=rpc, account=CLIENT,
            relays=RELAYS, value={'sessions': {key: {'group': GROUP, 'session': SID}}}, save=Mock())
        with patch.object(app.Client, 'hello'), patch.object(app.secrets, 'token_hex', return_value=new_sid):
            resumed = app.Client(profile, SERVER)
            self.assertEqual((resumed.group, resumed.sid), (GROUP, SID))
            rpc.create_group.assert_not_called()
            rpc.fetch_keys.assert_not_called()
            fresh = app.Client(profile, SERVER, new_session=True)
        self.assertEqual((fresh.group, fresh.sid), (new_group, new_sid))
        rpc.fetch_keys.assert_called_once_with(CLIENT, SERVER, RELAYS)
        rpc.create_group.assert_called_once_with(CLIENT, SERVER, new_sid, retention='86400s')
        self.assertEqual(profile.value['sessions'][key], {'group': new_group, 'session': new_sid})
        profile.save.assert_called_once_with()

    def test_submission_fsyncs_original_journal_before_first_send(self):
        synced = []
        real_fsync = os.fsync

        def fsync(fd):
            synced.append(fd)
            return real_fsync(fd)

        def call(op, **body):
            if op == 'token':
                return {'token': TOKEN}
            self.assertEqual(op, 'submit')
            self.assertGreaterEqual(len(synced), 2, 'file and containing directory must be synced')
            path = self.client.journal_path(INVOCATION)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(load_json(path), {'server': SERVER, 'session': SID, 'group': GROUP,
                'invocation': INVOCATION, 'command': COMMAND, 'token': TOKEN})
            self.assertEqual(body, {'invocation': INVOCATION, 'token': TOKEN, 'command': COMMAND})
            return {'disposition': 'accepted', 'status': {'state': 'running'}}

        self.client.call.side_effect = call
        with patch('marmish_protocol.os.fsync', side_effect=fsync):
            invocation, status = self.client.submit(COMMAND, INVOCATION)
        self.assertEqual((invocation, status), (INVOCATION, {'state': 'running'}))
        self.assertEqual([call.args[0] for call in self.client.call.call_args_list], ['token', 'submit'])

    def test_ambiguous_first_send_keeps_retry_material(self):
        self.client.call.side_effect = [{'token': TOKEN}, SmokeError('rpc_timeout')]
        with self.assertRaisesRegex(SmokeError, 'rpc_timeout'):
            self.client.submit(COMMAND, INVOCATION)
        self.assertEqual(load_json(self.client.journal_path(INVOCATION))['command'], COMMAND)
        self.client.call.reset_mock()
        self.client.call.side_effect = [{'state': 'running'}]
        invocation, status = self.client.submit(COMMAND, INVOCATION)
        self.assertEqual((invocation, status['state']), (INVOCATION, 'running'))
        self.client.call.assert_called_once_with('status', invocation=INVOCATION)
        self.assertFalse(self.client.submitted_new_input, 'reattachment must not resend stdin')

    def test_status_first_retry_uses_same_invocation_and_new_durable_token(self):
        self.journal()
        calls = []

        def call(op, **body):
            calls.append((op, body))
            if op == 'status':
                raise SmokeError('not_found')
            if op == 'token':
                return {'token': NEW_TOKEN}
            saved = load_json(self.client.journal_path(INVOCATION))
            self.assertEqual(saved['token'], NEW_TOKEN)
            self.assertEqual(body, {'invocation': INVOCATION, 'command': COMMAND, 'token': NEW_TOKEN})
            return {'disposition': 'accepted', 'status': {'state': 'running'}}

        self.client.call.side_effect = call
        self.assertEqual(self.client.retry(INVOCATION), {'state': 'running'})
        self.assertEqual([op for op, _ in calls], ['status', 'token', 'submit'])
        self.assertTrue(self.client.submitted_new_input, 'new admission must send stdin')

    def test_invalid_token_admission_race_queries_status(self):
        self.journal()
        self.client.call.side_effect = [SmokeError('not_found'), {'token': NEW_TOKEN},
                                        SmokeError('invalid_token'), {'state': 'running'}]
        self.assertEqual(self.client.retry(INVOCATION), {'state': 'running'})
        calls = self.client.call.call_args_list
        self.assertEqual([call.args[0] for call in calls], ['status', 'token', 'submit', 'status'])
        self.assertEqual(calls[2].kwargs['invocation'], INVOCATION)
        self.assertEqual(calls[2].kwargs['command'], COMMAND)
        self.assertEqual(calls[2].kwargs['token'], NEW_TOKEN)

    def test_other_status_errors_never_resubmit_and_conflicting_command_is_rejected(self):
        self.journal()
        self.client.call.side_effect = SmokeError('unauthorized')
        with self.assertRaisesRegex(SmokeError, 'unauthorized'):
            self.client.retry(INVOCATION)
        self.client.call.assert_called_once_with('status', invocation=INVOCATION)
        self.client.call.reset_mock()
        with self.assertRaisesRegex(SmokeError, 'invocation_conflict'):
            self.client.submit({'mode': 'exec', 'argv': [b64(b'/bin/false')]}, INVOCATION)
        self.client.call.assert_not_called()

    def test_scrubbed_completed_journal_allows_status_but_never_relaunch(self):
        self.journal()
        self.client.scrub(INVOCATION)
        saved = load_json(self.client.journal_path(INVOCATION))
        self.assertNotIn('command', saved)
        self.assertNotIn('token', saved)
        self.client.call.return_value = {'state': 'exited'}
        self.assertEqual(self.client.retry(INVOCATION), {'state': 'exited'})
        self.client.call.reset_mock()
        self.client.call.side_effect = SmokeError('not_found')
        with self.assertRaisesRegex(SmokeError, 'missing_command_journal'):
            self.client.retry(INVOCATION)
        self.client.call.assert_called_once_with('status', invocation=INVOCATION)

    def test_empty_stdin_already_closed_is_success_at_begin_or_eof(self):
        self.client.call.side_effect = SmokeError('input_closed')
        self.client.send_input(INVOCATION, io.BytesIO())
        self.client.call.assert_called_once_with('begin_input', invocation=INVOCATION)
        self.client.call.reset_mock()
        self.client.call.side_effect = [{'generation': 'aa' * 16}, SmokeError('input_closed')]
        self.client.send_input(INVOCATION, io.BytesIO())
        self.assertEqual([call.args[0] for call in self.client.call.call_args_list],
                         ['begin_input', 'stdin_eof'])

    def test_early_exit_stops_stdin_so_output_and_exit_status_can_be_read(self):
        for failure_at in ('begin', 'write', 'eof'):
            with self.subTest(failure_at=failure_at):
                self.client.call.reset_mock()
                replies = [SmokeError('input_closed')]
                if failure_at == 'write':
                    replies = [{'generation': 'aa' * 16}, SmokeError('input_closed')]
                elif failure_at == 'eof':
                    replies = [{'generation': 'aa' * 16}, {'accepted': 1}, SmokeError('input_closed')]
                self.client.call.side_effect = replies
                self.client.send_input(INVOCATION, io.BytesIO(b'x'))

    def test_binary_stdin_chunks_offsets_and_eof_are_exact(self):
        payload = bytes(range(256)) * 17
        self.client.call.side_effect = [{'generation': 'aa' * 16}, {'accepted': 4096},
                                       {'accepted': len(payload)}, {'health': 'eof_pending'}]
        self.client.send_input(INVOCATION, io.BytesIO(payload))
        calls = self.client.call.call_args_list
        self.assertEqual(calls[1].kwargs, {'invocation': INVOCATION, 'generation': 'aa' * 16,
            'offset': 0, 'bytes': b64(payload[:4096]), 'fresh_ms': 60000})
        self.assertEqual(calls[2].kwargs['offset'], 4096)
        self.assertEqual(calls[2].kwargs['bytes'], b64(payload[4096:]))
        self.assertEqual(calls[3].kwargs['final_offset'], len(payload))


class ServerTests(unittest.TestCase):
    def test_transient_restore_and_tick_errors_recover_without_quarantine(self):
        with tempfile.TemporaryDirectory() as temp:
            rpc = Mock()
            rpc.history.return_value = []
            rpc.show_group.return_value = {'group': {'pending_confirmation': False}}
            rpc.invites.return_value = []
            profile = SimpleNamespace(root=Path(temp), account=SERVER, rpc=rpc,
                value={'server_groups': {GROUP: {'peer': CLIENT, 'session': SID}}}, save=Mock())
            with patch.object(app, 'EngineBridge', return_value=Mock()), \
                    patch.object(app, 'verify_room', side_effect=app.RpcError('timeout')):
                server = app.Server(profile, [CLIENT], '/bridge', '/worker', temp)
            self.assertEqual(set(server.pending), {GROUP})
            self.assertEqual(server.quarantine, {})
            server.last_maintenance = continuous_time()
            with patch.object(app, 'verify_room'):
                server.tick()
            self.assertEqual(set(server.groups), {GROUP})
            self.assertEqual(server.pending, {})
            rpc.history.side_effect = app.RpcError('io')
            with patch.object(app, 'verify_room'):
                server.tick()
            self.assertEqual(set(server.groups), {GROUP})
            self.assertEqual(server.quarantine, {})
            rpc.history.side_effect = None
            rpc.history.return_value = [self.row('03' * 32, '04' * 16)]
            with patch.object(app, 'verify_room'):
                server.tick()
            rpc.send.assert_called_once()

    def test_ambiguous_accept_is_reconciled_after_restart_without_pending_invite(self):
        with tempfile.TemporaryDirectory() as temp:
            rpc = Mock()
            rpc.invites.return_value = []
            state_path = Path(temp) / 'profile.json'
            profile = SimpleNamespace(root=Path(temp), account=SERVER, rpc=rpc,
                value={'server_groups': {}}, save=Mock())
            profile.save.side_effect = lambda: save_json(state_path, profile.value)
            with patch.object(app, 'EngineBridge', return_value=Mock()):
                server = app.Server(profile, [CLIENT], '/bridge', '/worker', temp)
            rpc.invites.return_value = [{'group_id': GROUP, 'welcomer_account_id': CLIENT,
                'pending_confirmation': True, 'profile': {'name': 'marmish/v1/' + SID,
                    'description': 'marmish/v1 command transport'}}]
            rpc.show_group.return_value = {'group': {'pending_confirmation': True}}
            def uncertain_accept(account, group):
                self.assertEqual(load_json(state_path)['server_pending'][GROUP],
                                 {'peer': CLIENT, 'session': SID})
                raise app.RpcError('timeout', uncertain=True)
            rpc.accept.side_effect = uncertain_accept
            server.invitations()
            self.assertEqual(set(server.pending), {GROUP})
            self.assertEqual(server.quarantine, {})
            profile.value = load_json(state_path)
            rpc.invites.return_value = []  # MDK accepted; invite has disappeared.
            rpc.show_group.return_value = {'group': {'pending_confirmation': False}}
            rpc.history.return_value = []
            with patch.object(app, 'EngineBridge', return_value=Mock()), \
                    patch.object(app, 'verify_room') as verify:
                restarted = app.Server(profile, [CLIENT], '/bridge', '/worker', temp)
                restarted.invitations()
            verify.assert_called_once_with(profile, GROUP, CLIENT, SID)
            rpc.accept.assert_called_once()
            self.assertEqual(set(restarted.groups), {GROUP})
            self.assertEqual(load_json(state_path)['server_pending'], {})
            self.assertEqual(restarted.quarantine, {})

    def test_startup_and_enrollment_baseline_old_hello_before_fresh_handshake(self):
        with tempfile.TemporaryDirectory() as temp:
            old = self.row('01' * 32, '02' * 16)
            rpc = Mock()
            rpc.history.return_value = [old]
            rpc.show_group.return_value = {'group': {'profile': {
                'name': 'marmish/v1/' + SID, 'description': 'marmish/v1 command transport'}}}
            rpc.members.return_value = [
                {'member_id': SERVER, 'npub': SERVER, 'local': True},
                {'member_id': CLIENT, 'npub': CLIENT, 'local': False}]
            rpc.invites.return_value = []
            profile = SimpleNamespace(root=Path(temp), account=SERVER, rpc=rpc,
                value={'server_groups': {GROUP: {'peer': CLIENT, 'session': SID}}}, save=Mock())
            engine = Mock()
            with patch.object(app, 'EngineBridge', return_value=engine):
                server = app.Server(profile, [CLIENT], '/unused/bridge', '/unused/worker', temp)
            server.last_maintenance = continuous_time()
            server.tick()
            rpc.send.assert_not_called()
            self.assertEqual(len(server.dispatcher.challenges), 0)
            engine.call.assert_not_called()
            fresh = self.row('03' * 32, '04' * 16)
            rpc.history.return_value = [fresh, old]
            server.tick()
            rpc.send.assert_called_once()
            reply = app.decode(rpc.send.call_args.args[2])
            self.assertEqual(reply['id'], '04' * 16)
            self.assertTrue(reply['body']['ok'])
            self.assertEqual(reply['body']['result']['nonce'], '99' * 32)
            self.assertEqual(len(server.dispatcher.challenges), 1)
            engine.call.assert_not_called()  # hello never reaches the command engine
            server.tick()
            rpc.send.assert_called_once()  # identical history does not repeat work

    def test_failed_restored_room_is_persistently_quarantined(self):
        with tempfile.TemporaryDirectory() as temp:
            profile = SimpleNamespace(root=Path(temp), account=SERVER, save=Mock(),
                value={'server_groups': {GROUP: {'peer': CLIENT, 'session': SID}}})
            state_path = Path(temp) / 'profile.json'
            profile.save.side_effect = lambda: save_json(state_path, profile.value)
            engine = Mock()
            with patch.object(app, 'EngineBridge', return_value=engine), \
                    patch.object(app, 'verify_room', side_effect=SmokeError('membership_mismatch')):
                server = app.Server(profile, [CLIENT], '/unused/bridge', '/unused/worker', temp)
            self.assertEqual(server.groups, {})
            self.assertEqual(profile.value['server_quarantine'], {GROUP: 'room_fault'})
            self.assertEqual(profile.value['server_groups'], {})
            profile.save.assert_called_once_with()
            engine.close.assert_not_called()
            profile.value = load_json(state_path)
            with patch.object(app, 'EngineBridge', return_value=engine), \
                    patch.object(app, 'verify_room') as verify:
                restarted = app.Server(profile, [CLIENT], '/unused/bridge', '/unused/worker', temp)
            verify.assert_not_called()
            self.assertEqual(restarted.quarantine, {GROUP: 'room_fault'})

    def test_failed_accepted_invitation_is_quarantined_before_next_poll(self):
        with tempfile.TemporaryDirectory() as temp:
            rpc = Mock()
            rpc.invites.return_value = []
            profile = SimpleNamespace(root=Path(temp), account=SERVER, rpc=rpc,
                value={'server_groups': {}}, save=Mock())
            with patch.object(app, 'EngineBridge', return_value=Mock()):
                server = app.Server(profile, [CLIENT], '/bridge', '/worker', temp)
            rpc.invites.return_value = [{'group_id': GROUP, 'welcomer_account_id': CLIENT,
                'pending_confirmation': True, 'profile': {'name': 'marmish/v1/' + SID,
                    'description': 'marmish/v1 command transport'}}]
            rpc.accept.return_value = {'accepted': True}
            rpc.show_group.return_value = {'group': {'pending_confirmation': True}}
            rpc.history.side_effect = SmokeError('history_limit')
            with patch.object(app, 'verify_room'):
                server.invitations()
                server.invitations()
            rpc.accept.assert_called_once()
            self.assertEqual(server.groups, {})
            self.assertEqual(server.histories, {})
            self.assertEqual(profile.value['server_quarantine'], {GROUP: 'room_fault'})

    def test_room_fault_does_not_stop_healthy_group_or_return_on_restart(self):
        for failure in ('history', 'membership'):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as temp:
                healthy = '88' * 32
                rpc = Mock()
                rpc.history.return_value = []
                rpc.invites.return_value = []
                profile = SimpleNamespace(root=Path(temp), account=SERVER, rpc=rpc,
                    value={'server_groups': {group: {'peer': CLIENT, 'session': SID}
                           for group in (GROUP, healthy)}}, save=Mock())
                with patch.object(app, 'EngineBridge', return_value=Mock()), \
                        patch.object(app, 'verify_room'):
                    server = app.Server(profile, [CLIENT], '/bridge', '/worker', temp)
                server.last_maintenance = continuous_time()
                fresh = self.row('03' * 32, '04' * 16)
                fresh['group_id'] = healthy
                fresh['plaintext'] = encode(envelope(SID, healthy, CLIENT, SERVER, '04' * 16,
                    'hello', {'nonce': '99' * 32})).decode('ascii')
                def history(account, group):
                    if group == GROUP and failure == 'history':
                        raise SmokeError('history_limit')
                    return [fresh] if group == healthy else []
                def verify(profile, group, peer, sid):
                    if group == GROUP and failure == 'membership':
                        raise SmokeError('membership_mismatch')
                rpc.history.side_effect = history
                with patch.object(app, 'verify_room', side_effect=verify):
                    server.tick()
                    server.tick()
                self.assertEqual(server.quarantine, {GROUP: 'room_fault'})
                self.assertEqual(set(server.groups), {healthy})
                rpc.send.assert_called_once()
                self.assertEqual(rpc.send.call_args.args[1], healthy)
                with patch.object(app, 'EngineBridge', return_value=Mock()), \
                        patch.object(app, 'verify_room') as verify_restored:
                    restarted = app.Server(profile, [CLIENT], '/bridge', '/worker', temp)
                self.assertEqual(set(restarted.groups), {healthy})
                self.assertEqual(verify_restored.call_args.args[1], healthy)

    def test_33rd_pending_room_is_ignored_without_interrupting_existing_rooms(self):
        server = app.Server.__new__(app.Server)
        server.allow, server.quarantine, server.pending = {CLIENT}, {}, {}
        server.groups = {f'{index:064x}': {'peer': CLIENT, 'session': SID} for index in range(32)}
        rpc = Mock()
        rpc.invites.return_value = [{'group_id': GROUP, 'welcomer_account_id': CLIENT,
            'pending_confirmation': True, 'profile': {'name': 'marmish/v1/' + SID,
                'description': 'marmish/v1 command transport'}}]
        server.profile = SimpleNamespace(rpc=rpc, account=SERVER, value={}, save=Mock())
        server.last_maintenance = continuous_time()
        server._tick_group = Mock()
        server.tick()
        server.tick()
        self.assertEqual(server._tick_group.call_count, 64)
        rpc.accept.assert_not_called()
        self.assertEqual(server.quarantine, {})

    def test_general_chat_and_unallowlisted_invitations_are_not_accepted(self):
        server = app.Server.__new__(app.Server)
        server.allow, server.groups, server.pending = {CLIENT}, {}, {}
        rpc = Mock()
        good_profile = {'name': 'marmish/v1/' + SID,
                        'description': 'marmish/v1 command transport'}
        rpc.invites.return_value = [
            {'group_id': GROUP, 'welcomer_account_id': CLIENT, 'pending_confirmation': True,
             'profile': {'name': 'ordinary DM', 'description': 'chat'}},
            {'group_id': GROUP, 'welcomer_account_id': SERVER, 'pending_confirmation': True,
             'profile': good_profile},
            {'group_id': GROUP, 'welcomer_account_id': CLIENT, 'pending_confirmation': False,
             'profile': good_profile}]
        server.profile = SimpleNamespace(rpc=rpc, account=SERVER)
        server.invitations()
        rpc.accept.assert_not_called()
        self.assertEqual(server.groups, {})

    @staticmethod
    def row(message, request):
        return {'message_id': message, 'group_id': GROUP, 'from': CLIENT,
                'direction': 'received', 'kind': 30100, 'tags': [['marmish', 'v1']],
                'plaintext': encode(envelope(SID, GROUP, CLIENT, SERVER, request,
                                           'hello', {'nonce': '99' * 32})).decode('ascii')}


if __name__ == '__main__':
    unittest.main()
