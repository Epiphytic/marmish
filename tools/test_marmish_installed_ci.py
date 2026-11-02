"""Orchestration trust boundaries; live acceptance still requires native jobs."""
import hashlib
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from mdk_smoke import SmokeError
import marmish_installed_ci as ci

CTX = {'run': '123', 'attempt': '1', 'sha': 'a' * 40}


class InstalledOrchestrationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.args = SimpleNamespace(state=self.root / 'state',
            pin_dir=self.root / 'pin', role='linux-x64', peers=self.root / 'peers')
        self.pin = {'installer_sha256': hashlib.sha256(
            (ci.REPO / 'scripts/install.sh').read_bytes()).hexdigest(),
            'version': '0.1.0-experimental.2', 'relays': [], 'targets': {}}

    def test_changed_installer_or_wrong_native_target_cannot_start_child(self):
        for digest, target in (('0' * 64, ci.ROLE_TARGETS['linux-x64']),
                               (self.pin['installer_sha256'],
                                ci.ROLE_TARGETS['macos-arm64'])):
            with self.subTest(digest=digest, target=target), \
                    patch.object(ci, 'native_target', return_value=target), \
                    patch.object(ci, 'create_state') as create, \
                    patch.object(ci, 'run_bounded') as run:
                self.pin['installer_sha256'] = digest
                with self.assertRaises(SmokeError):
                    ci.prepare(self.args, CTX, self.pin, ci.initial_report(CTX, 'linux-x64'))
                create.assert_not_called()
                run.assert_not_called()

    def test_failed_installer_does_not_verify_or_initialize_payload(self):
        state = {'root': str(self.root), 'prefix': str(self.root / 'install'),
                 'installer_sha256': self.pin['installer_sha256'],
                 'preparation_deadline': ci.clock_sample()['continuous_ns'] / 1e9 + 60}
        for changes, category in (({'returncode': 1}, 'installer_refused'),
                                  ({'timed_out': True}, 'installer_timeout'),
                                  ({'output_limited': True}, 'installer_output_limit')):
            result = dict(returncode=0, timed_out=False, output_limited=False,
                          cleanup='complete', stdout=b'private', stderr=b'private')
            result.update(changes)
            with self.subTest(category=category), \
                    patch.object(ci, 'native_target', return_value=ci.ROLE_TARGETS['linux-x64']), \
                    patch.object(ci, 'create_state', return_value=state), \
                    patch.object(ci, 'run_bounded', return_value=result), \
                    patch.object(ci, 'verify_installed') as verify, \
                    patch.object(ci, 'initialize_identity') as initialize:
                with self.assertRaisesRegex(SmokeError, '^' + category + '$'):
                    ci.prepare(self.args, CTX, self.pin, ci.initial_report(CTX, 'linux-x64'))
                verify.assert_not_called()
                initialize.assert_not_called()

    def test_duplicate_local_identities_never_publish_readiness(self):
        state = {'root': str(self.root), 'output': str(self.root / 'public'),
                 'preparation_deadline': ci.clock_sample()['continuous_ns'] / 1e9 + 60}
        with patch.object(ci, 'checked_state', return_value=state), \
                patch.object(ci, 'initialize_identity', return_value='a' * 64), \
                patch.object(ci, 'validate_identity') as validate:
            with self.assertRaisesRegex(SmokeError, 'identity_init_failed'):
                ci.identities(self.args, CTX, self.pin, ci.initial_report(CTX, 'linux-x64'))
            validate.assert_not_called()
            self.assertFalse((self.root / 'public').exists())

    def test_wrong_attempt_or_role_state_cannot_start_runtime(self):
        for changed in ({'ctx': {**CTX, 'attempt': '2'}}, {'role': 'macos-arm64'},
                        {'pin_sha256': '0' * 64}):
            state = {'ctx': CTX, 'role': 'linux-x64',
                     'pin_sha256': ci.pin_digest(self.pin), **changed}
            with self.subTest(changed=changed), \
                    patch.object(ci, 'load_state', return_value=state):
                with self.assertRaisesRegex(SmokeError, 'artifact_invalid'):
                    ci.checked_state(self.args, CTX, self.pin)

    def test_unknown_exception_text_never_enters_public_failure_report(self):
        argv = ['ci', 'prepare', '--state', str(self.args.state), '--pin-dir',
                str(self.args.pin_dir), '--role', self.args.role]
        with patch('sys.argv', argv), patch.object(ci, 'context', return_value=CTX), \
                patch.object(ci, 'load_pin', return_value=self.pin), \
                patch.object(ci, 'prepare', side_effect=RuntimeError('SECRET sentinel')):
            self.assertEqual(ci.main(), 1)
        raw = (ci.public_directory(self.args.state) / 'installation-result.json').read_text()
        self.assertNotIn('SECRET', raw)
        record = json.loads(raw)
        self.assertEqual(record['error'], 'internal_failure')
        self.assertEqual(record['outcome'], 'failed')

    def test_absent_pin_has_explicit_null_binding_and_never_starts_install(self):
        argv = ['ci', 'prepare', '--state', str(self.args.state), '--pin-dir',
                str(self.args.pin_dir), '--role', self.args.role]
        with patch('sys.argv', argv), patch.object(ci, 'context', return_value=CTX), \
                patch.object(ci, 'prepare') as prepare:
            self.assertEqual(ci.main(), 1)
            prepare.assert_not_called()
        record = json.loads((ci.public_directory(self.args.state) /
                             'installation-result.json').read_bytes())
        self.assertIsNone(record['pin_sha256'])
        self.assertEqual(record['error'], 'pin_unavailable')

    def test_cleanup_refusal_retains_failure_and_has_no_successful_checks(self):
        self.args.state.touch()
        with patch.object(ci, 'load_state', side_effect=SmokeError('unsafe_state_path')):
            self.assertEqual(ci.cleanup(self.args, CTX, 'b' * 64), 1)
        record = json.loads((ci.public_directory(self.args.state) /
                             'cleanup-result.json').read_bytes())
        self.assertEqual(record['outcome'], 'incomplete')
        self.assertEqual(record['error'], 'cleanup_incomplete')
        self.assertTrue(all(value is None for value in record['checks'].values()))
        self.assertTrue(self.args.state.exists())

    def test_invalid_clock_never_becomes_valid_performance_measurement(self):
        start = {'continuous_ns': 1, 'monotonic_ns': 1, 'wall_ns': 1}
        end = {'continuous_ns': 6_000_000_002, 'monotonic_ns': 2, 'wall_ns': 2}
        report = ci.initial_report(CTX, 'linux-x64')
        with patch.object(ci, 'clock_sample', side_effect=[start, end]):
            with self.assertRaisesRegex(SmokeError, 'clock_invalid'):
                ci.measure(report, 'install', lambda: None)
        self.assertFalse(report['timing']['stages']['install']['clock_valid'])

    def test_failed_adapter_category_reaches_barrier_report(self):
        state = {'output': str(self.root), 'barrier_deadline': 0}
        for category in ('barrier_timeout', 'artifact_network_failed',
                         'artifact_listing_unstable', 'artifact_invalid'):
            record = {**ci.envelope(CTX, self.args.role, ci.pin_digest(self.pin),
                                   'adapter-result'),
                      'stage': 'identity-ready', 'outcome': 'failed', 'error': category}
            status = self.args.peers.with_name(self.args.peers.name + '.status.json')
            ci.write_record(status, record)
            report = ci.initial_report(CTX, self.args.role)
            with self.subTest(category=category), \
                    patch.object(ci, 'checked_state', return_value=state):
                with self.assertRaisesRegex(SmokeError, '^' + category + '$'):
                    ci.barrier(self.args, CTX, self.pin, report)
                self.assertEqual(report['stage'], 'barrier')

    def test_missing_adapter_status_cannot_retain_passed_barrier(self):
        report = ci.initial_report(CTX, self.args.role)
        with patch.object(ci, 'checked_state', return_value={}):
            with self.assertRaisesRegex(SmokeError, 'artifact_invalid'):
                ci.barrier(self.args, CTX, self.pin, report)
        self.assertEqual(report['stage'], 'barrier')


if __name__ == '__main__':
    unittest.main()
