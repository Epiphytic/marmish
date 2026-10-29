import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from mdk_smoke import SmokeError
from marmish_keys import KeySession, NativeHelper, encode_nsec, protected_read, protected_unlink
from marmish_protocol import save_json


class FakeNative:
    def __init__(self, error=None):
        self.error = error
        self.items = {}
        self.creates = []
        self.probes = 0

    def probe(self):
        self.probes += 1
        if self.error:
            raise SmokeError(self.error)

    def derive_nostr(self, secret):
        return hashlib.sha256(secret).hexdigest()

    def load(self, reference):
        if reference['id'] not in self.items:
            raise SmokeError('missing')
        return self.items[reference['id']]

    def create(self, reference, label, secret):
        if reference['id'] in self.items:
            raise SmokeError('exists')
        self.creates.append(reference.copy())
        self.items[reference['id']] = secret


class Keys(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='marmish-keys-')
        self.base = Path(self.temp.name)
        self.root = self.base / 'profile'
        self.catalog = self.base / 'catalog' / 'keys.json'
        self.native = FakeNative()

    def tearDown(self):
        self.temp.cleanup()

    def session(self, **kw):
        return KeySession(self.root, helper=self.native, catalog=self.catalog, **kw)

    def provision(self, *, master=True, **kw):
        with self.session(**kw) as session:
            nostr = session.prepare_nostr(pristine=True)
            sql = session.prepare_master(self.root / 'engine', pristine=True) if master else None
            value = session.commit({'version': 1, 'account': nostr.account, 'initialized': False})
        return nostr, sql, value

    def test_native_bindings_are_sticky_separate_and_public_only(self):
        nostr, master, value = self.provision(key_name='remote 🦫')
        self.assertNotEqual(nostr.reference, master.reference)
        self.assertNotEqual(nostr.secret, master.secret)
        self.assertNotIn(nostr.secret.hex(), repr(nostr))
        self.assertNotIn(master.secret.hex(), repr(master))
        self.assertNotIn('secret', json.dumps(value))
        self.assertFalse((self.root / 'keys.pending.json').exists())
        with self.session(key_name='remote 🦫') as session:
            loaded = session.prepare_nostr(existing_account=nostr.account)
            self.assertEqual(loaded.secret, nostr.secret)
            self.assertEqual(session.prepare_master(self.root / 'engine').secret, master.secret)
            session.commit(value)
        self.assertEqual(len(self.native.creates), 2)
        self.assertEqual(self.native.probes, 1)

    def test_named_binding_cannot_move_or_copy(self):
        nostr, _, _ = self.provision(master=False, key_name='one')
        second = self.base / 'second'
        shutil.copytree(self.root, second)
        with KeySession(second, helper=self.native, catalog=self.catalog, key_name='one') as session:
            with self.assertRaisesRegex(SmokeError, 'key_name_bound_elsewhere'):
                session.prepare_nostr(existing_account=nostr.account)

    def test_named_entry_cannot_enroll_another_profile(self):
        self.provision(master=False, key_name='one')
        with KeySession(self.base / 'second', helper=self.native, catalog=self.catalog, key_name='one') as session:
            with self.assertRaisesRegex(SmokeError, 'key_name_bound_elsewhere'):
                session.prepare_nostr(pristine=True)
        self.assertEqual(len(self.native.creates), 1)

    def test_only_unavailable_auto_falls_back_and_files_never_probes(self):
        self.native.error = 'unavailable'
        with self.session() as session:
            binding = session.prepare_nostr(pristine=True)
            self.assertEqual(binding.backend, 'files')
            session.commit({'account': 'ab' * 32})
        self.assertFalse(self.catalog.exists())
        with self.session(storage='files') as session:
            self.assertEqual(session.prepare_nostr(existing_account='ab' * 32).backend, 'files')
        self.assertEqual(self.native.probes, 1)
        for error in ('locked_or_denied', 'uninitialized', 'ambiguous', 'backend_failure'):
            root = self.base / error
            with KeySession(root, helper=FakeNative(error), catalog=self.catalog) as session:
                with self.assertRaisesRegex(SmokeError, error):
                    session.prepare_nostr(pristine=True)

    def test_profile_lock_fails_closed_and_native_binding_cannot_switch_backend(self):
        with self.session():
            with self.assertRaisesRegex(SmokeError, 'key_storage_busy'):
                with self.session():
                    pass
        self.provision(master=False)
        with self.session(storage='files') as session:
            with self.assertRaisesRegex(SmokeError, 'key_storage_conflict'):
                session.prepare_nostr()

    def test_files_choice_is_durable_before_daemon_or_profile_publication(self):
        self.native.error = 'unavailable'
        with self.session() as session:
            binding = session.prepare_nostr(pristine=True)
            profile_id = binding.profile_id
        self.assertFalse((self.root / 'profile.json').exists())
        marker = json.loads((self.root / 'keys.mode.json').read_text())
        self.assertEqual(set(marker), {'version', 'backend', 'profile_id', 'profile'})
        self.native.error = None
        with self.session() as session:
            binding = session.prepare_nostr(pristine=True)
            self.assertEqual(binding.backend, 'files')
            self.assertEqual(binding.profile_id, profile_id)
        self.assertEqual(self.native.probes, 1)
        self.assertEqual(self.native.creates, [])

    def test_explicit_native_never_falls_back_and_named_files_fail(self):
        self.native.error = 'unavailable'
        with self.session(storage='native') as session:
            with self.assertRaisesRegex(SmokeError, 'unavailable'):
                session.prepare_nostr(pristine=True)
        with self.session(storage='files', key_name='name') as session:
            with self.assertRaisesRegex(SmokeError, 'named_key_requires_native'):
                session.prepare_nostr(pristine=True)

    def test_existing_files_remain_files_without_helper_or_catalog(self):
        self.root.mkdir(mode=0o700)
        save_json(self.root / 'profile.json', {'account': 'aa' * 32})
        with KeySession(self.root, catalog=self.catalog) as session:
            self.assertEqual(session.prepare_nostr(existing_account='aa' * 32).backend, 'files')
        self.assertFalse(self.catalog.exists())
        with self.session(storage='native') as session:
            with self.assertRaisesRegex(SmokeError, 'explicit_key_migration_required'):
                session.prepare_nostr(existing_account='aa' * 32)

    def test_crash_before_create_never_regenerates_random_identity(self):
        original_create = self.native.create
        self.native.create = lambda *a: (_ for _ in ()).throw(SmokeError('backend_failure'))
        with self.session() as session:
            with self.assertRaisesRegex(SmokeError, 'backend_failure'):
                session.prepare_nostr(pristine=True)
        self.native.create = original_create
        with self.session() as session:
            with self.assertRaisesRegex(SmokeError, 'missing'):
                session.prepare_nostr(pristine=True)
        self.assertEqual(self.native.creates, [])

    def test_crash_after_create_recovers_exact_reference(self):
        with self.session() as session:
            first = session.prepare_nostr(pristine=True)
        with self.session() as session:
            second = session.prepare_nostr(pristine=True)
            self.assertEqual(first.public(), second.public())
            self.assertEqual(first.secret, second.secret)
            session.commit({'account': second.account, 'initialized': False})
        self.assertEqual(len(self.native.creates), 1)

    def test_client_native_can_add_independent_engine_master(self):
        first, _, value = self.provision(master=False)
        with self.session() as session:
            session.prepare_nostr(existing_account=first.account)
            master = session.prepare_master(self.root / 'engine', pristine=True)
            value = session.commit(value)
        self.assertIsNotNone(master.secret)
        with self.session() as session:
            session.prepare_nostr(existing_account=first.account)
            self.assertEqual(session.commit(value)['keys']['master'], master.public())

    def test_migration_journals_both_targets_before_creates_and_retries_exact(self):
        self.root.mkdir(mode=0o700)
        raw_nostr, raw_master = b'n' * 32, b'm' * 32
        account = self.native.derive_nostr(raw_nostr)
        save_json(self.root / 'profile.json', {'account': account})
        with self.session(storage='native', key_name='legacy') as session:
            session.begin_migration(account)
            target = session.migration_targets(self.root / 'engine')
            self.assertEqual(self.native.creates, [])
            pending = json.loads((self.root / 'keys.pending.json').read_text())
            self.assertEqual(pending['master'], target['master'])
        with self.session(storage='native', key_name='legacy') as session:
            session.begin_migration(account)
            self.assertEqual(target, session.migration_targets(self.root / 'engine'))
            session.prepare_nostr(existing_account=account, migration_secret=raw_nostr)
            session.prepare_master(self.root / 'engine', migration_secret=raw_master)
            self.assertEqual(session.pending['state'], 'NATIVE_CREATED')
            session.commit({'account': account})
            self.assertEqual(session.pending['state'], 'COMMITTED')
            session.mark_cleaned()
        self.assertEqual(len(self.native.creates), 2)

    def test_wrong_migration_identity_does_not_create(self):
        with self.session(storage='native') as session:
            session.begin_migration('ab' * 32)
            session.migration_targets()
            with self.assertRaisesRegex(SmokeError, 'profile_identity_mismatch'):
                session.prepare_nostr(existing_account='ab' * 32, migration_secret=b'n' * 32)
        self.assertEqual(self.native.creates, [])

    def test_pending_migration_requires_original_nostr_before_commit(self):
        account = self.native.derive_nostr(b'n' * 32)
        with self.session(storage='native') as session:
            session.begin_migration(account)
            session.migration_targets()
        with self.session(storage='native') as session:
            session.begin_migration(account)
            session.migration_targets()
            with self.assertRaisesRegex(SmokeError, 'explicit_key_migration_required'):
                session.prepare_nostr(existing_account=account)
        self.assertEqual(self.native.creates, [])

    def test_committed_migration_recovers_after_profile_publish_before_journal(self):
        raw_nostr, raw_master = b'n' * 32, b'm' * 32
        account = self.native.derive_nostr(raw_nostr)
        with self.session(storage='native') as session:
            session.begin_migration(account)
            targets = session.migration_targets(self.root / 'engine')
            with self.assertRaisesRegex(SmokeError, 'migration_not_committed'):
                session.load_migration_master()
            session.prepare_nostr(existing_account=account, migration_secret=raw_nostr)
            session.prepare_master(self.root / 'engine', migration_secret=raw_master)
            real_save = save_json
            def crash(path, value):
                if Path(path).name == 'keys.pending.json' and value.get('state') == 'COMMITTED':
                    raise SmokeError('simulated_crash')
                real_save(path, value)
            with patch('marmish_keys.save_json', side_effect=crash):
                with self.assertRaisesRegex(SmokeError, 'simulated_crash'):
                    session.commit({'account': account})
        with self.session(storage='native') as session:
            self.assertEqual(session.pending['state'], 'COMMITTED')
            session.begin_migration(account)
            self.assertEqual(session.migration_targets(self.root / 'engine'), targets)
            self.assertEqual(session.load_migration_master(), raw_master)
            self.assertEqual(session.prepare_nostr(existing_account=account).secret, raw_nostr)
            self.assertEqual(session.prepare_master(self.root / 'engine').secret, raw_master)
            session.commit(session.value)
            session.mark_cleaned()
        self.assertEqual(len(self.native.creates), 2)

    def test_missing_catalog_never_reenrolls_committed_identity(self):
        nostr, _, _ = self.provision(master=False)
        self.catalog.unlink()
        with self.session() as session:
            with self.assertRaisesRegex(SmokeError, 'key_catalog_missing'):
                session.prepare_nostr(existing_account=nostr.account)
        self.assertEqual(len(self.native.creates), 1)

    def test_journal_cannot_authorize_unverified_committed_keys(self):
        with self.session() as session:
            session.prepare_nostr(pristine=True)
        path = self.root / 'keys.pending.json'
        journal = json.loads(path.read_text())
        journal['state'] = 'COMMITTED'
        save_json(path, journal)
        with self.assertRaisesRegex(SmokeError, 'invalid_key_journal'):
            with self.session():
                pass

    def test_helper_controller_private_stdio_and_host_discovery_only(self):
        helper = self.base / 'helper'
        helper.write_text('#!' + sys.executable + '\n' +
            'import sys,json,os\n'
            'request=json.load(sys.stdin)\n'
            'json.dump({"version":1,"ok":True,"result":{"argv":sys.argv[1:],"env":dict(os.environ),"action":request["action"]}},sys.stdout)\n')
        helper.chmod(0o700)
        with patch.dict(os.environ, {'GH_TOKEN': 'do-not-pass', 'AWS_SECRET_ACCESS_KEY': 'do-not-pass',
                                   'HOME': str(self.base), 'DBUS_SESSION_BUS_ADDRESS': 'unix:path=owned'}):
            result = NativeHelper(helper).call('probe')
        self.assertEqual(result['argv'], [])
        self.assertNotIn('GH_TOKEN', result['env'])
        self.assertNotIn('AWS_SECRET_ACCESS_KEY', result['env'])
        self.assertEqual(result['env']['HOME'], str(self.base))
        self.assertEqual(result['env']['DBUS_SESSION_BUS_ADDRESS'], 'unix:path=owned')
        self.assertEqual(result['action'], 'probe')

    def test_helper_controller_bounds_outputs_deadline_and_rejects_bad_protocol(self):
        cases = [
            ('import time;time.sleep(2)', 'key_helper_timeout', .05),
            ('import sys;sys.stdout.write("x"*140000)', 'key_helper_response_limit', 2),
            ('import sys;sys.stderr.write("x"*10000)', 'key_helper_response_limit', 2),
            ('print(\'{"version":1,"ok":true,"ok":true,"result":{}}\')', 'invalid_json', 2),
            ('print(\'{"version":1,"ok":false,"error":[]}\')', 'key_helper_response', 2),
        ]
        for index, (code, expected, timeout) in enumerate(cases):
            helper = self.base / ('helper-' + str(index))
            helper.write_text('#!' + sys.executable + '\nimport sys\nsys.stdin.read()\n' + code + '\n')
            helper.chmod(0o700)
            with self.subTest(expected=expected):
                with self.assertRaisesRegex(SmokeError, expected):
                    NativeHelper(helper, timeout=timeout).probe()

    def test_private_file_helpers_reject_symlinks_modes_and_wrong_cleanup(self):
        path = self.base / 'key'
        path.write_bytes(b'exact')
        path.chmod(0o600)
        self.assertEqual(protected_read(path), b'exact')
        with self.assertRaisesRegex(SmokeError, 'legacy_key_mismatch'):
            protected_unlink(path, b'other')
        link = self.base / 'link'
        link.symlink_to(path)
        with self.assertRaisesRegex(SmokeError, 'unsafe_key_path'):
            protected_read(link)
        path.chmod(0o644)
        with self.assertRaisesRegex(SmokeError, 'unsafe_key_file'):
            protected_read(path)
        path.chmod(0o600)
        protected_unlink(path, b'exact')
        self.assertFalse(path.exists())

    def test_nsec_known_vector_and_binding_secrets_not_repr(self):
        encoded = encode_nsec(bytes(31) + b'\x01')
        self.assertEqual(encoded, 'nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsmhltgl')


if __name__ == '__main__':
    unittest.main()
