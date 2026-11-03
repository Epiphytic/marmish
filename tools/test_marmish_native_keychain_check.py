"""Offline supervisor contracts; real native provider success is a CI gate."""
import json
import signal
import sys
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import marmish_native_keychain_check as check


class InstalledProviderSupervisor(unittest.TestCase):
    def test_product_timeout_reaps_only_its_owned_group(self):
        original = check.subprocess.Popen
        children = []
        def start(*args, **kwargs):
            child = original(*args, **kwargs)
            children.append(child)
            return child
        with mock.patch.object(check.subprocess, 'Popen', side_effect=start), \
                mock.patch.object(check.os, 'killpg', wraps=check.os.killpg) as kill:
            with self.assertRaisesRegex(check.Refused, 'operation_timeout'):
                check.execute(['/bin/sh', '-c', 'exec /bin/sleep 5'], {}, timeout=0.05)
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll())
        self.assertTrue(all(call.args[0] == children[0].pid for call in kill.call_args_list))

    def test_completed_leader_descendant_is_failure_after_owned_cleanup(self):
        child = mock.Mock(pid=98765)
        child.poll.return_value = 0
        # Group initially remains, is signalled, then disappears after TERM.
        with mock.patch.object(check.os, 'killpg', side_effect=[None, None, ProcessLookupError]) as kill:
            with self.assertRaisesRegex(check.Refused, 'owned_descendant_remained'):
                check.stop(child)
        self.assertEqual([call.args[0] for call in kill.call_args_list], [98765] * 3)
        child.wait.assert_not_called()

    def test_naturally_completed_descendant_exit_before_signal_still_fails(self):
        child = mock.Mock(pid=98765)
        child.poll.return_value = 0
        with mock.patch.object(check.os, 'killpg', side_effect=[None, ProcessLookupError]) as kill:
            with self.assertRaisesRegex(check.Refused, 'owned_descendant_remained'):
                check.stop(child)
        self.assertEqual([call.args[0] for call in kill.call_args_list], [98765] * 2)

    def test_capture_limit_is_enforced_while_child_runs(self):
        # Exercise the real short-lived-child cleanup window repeatedly.
        for _ in range(8):
            with mock.patch.object(check, 'MAX_CAPTURE', 16):
                with self.assertRaisesRegex(check.Refused, '^operation_output_limit$'):
                    check.execute(['/bin/sh', '-c', 'printf 12345678901234567890'], {}, timeout=1)

    def test_permission_probe_requires_exact_reap_and_fresh_absence(self):
        for exited in (None, 0):
            with self.subTest(exited=exited):
                child = mock.Mock(pid=98765)
                child.poll.return_value = exited
                with mock.patch.object(check.os, 'killpg',
                        side_effect=[PermissionError, ProcessLookupError]) as kill:
                    check.stop(child)
                child.wait.assert_called_once_with(timeout=3)
                self.assertEqual(kill.call_args_list, [mock.call(98765, 0)] * 2)

    def test_permission_probe_remaining_unknown_or_denied_group_is_red(self):
        for result in (None, PermissionError(), OSError('unknown')):
            with self.subTest(result=type(result).__name__):
                child = mock.Mock(pid=98765)
                with mock.patch.object(check.os, 'killpg',
                        side_effect=[PermissionError, result]) as kill:
                    with self.assertRaisesRegex(check.Refused, '^child_cleanup_incomplete$'):
                        check.stop(child)
                child.wait.assert_called_once_with(timeout=3)
                self.assertEqual(kill.call_args_list, [mock.call(98765, 0)] * 2)

    def test_permission_probe_wait_failure_is_finite_and_never_signals(self):
        for error in (subprocess.TimeoutExpired('owned', 3), OSError('unknown')):
            with self.subTest(error=type(error).__name__):
                child = mock.Mock(pid=98765)
                child.wait.side_effect = error
                with mock.patch.object(check.os, 'killpg', side_effect=PermissionError) as kill:
                    with self.assertRaisesRegex(check.Refused, '^child_cleanup_incomplete$'):
                        check.stop(child)
                self.assertEqual(kill.call_args_list, [mock.call(98765, 0)])

    def test_helper_only_requests_public_metadata(self):
        qualification = check.Qualification.__new__(check.Qualification)
        qualification.bins, qualification.environment = Path('/owned/bin'), {}
        value = {'version': 1, 'ok': True, 'result': {'items': []}}
        with mock.patch.object(check, 'execute', return_value=(0, json.dumps(value).encode())) as run:
            self.assertEqual(qualification.helper('list'), value)
            request = json.loads(run.call_args.args[3])
            self.assertEqual(request, {'version': 1, 'action': 'list', 'namespace': 'marmish-v2'})
            with self.assertRaisesRegex(check.Refused, 'forbidden_helper_operation'):
                qualification.helper('load')
            self.assertEqual(run.call_count, 1)

    def test_daemon_frontend_reload_uses_actual_daemon_and_rejects_mismatch(self):
        qualification = check.Qualification.__new__(check.Qualification)
        qualification.bins, qualification.state = Path('/owned/bin'), Path('/owned/state')
        qualification.environment = {'HOME': '/owned'}
        with mock.patch.object(check, 'execute', return_value=(0, b'{"identity":"public"}')) as run:
            qualification.daemon_reload('second', 'public')
            self.assertEqual(run.call_args.args, (['/owned/bin/marmishd', '--state-dir',
                '/owned/state', '-i', 'second', 'init'], qualification.environment, 120))
            with self.assertRaisesRegex(check.Refused, 'fresh_identity_reload'):
                qualification.daemon_reload('second', 'different')
        with mock.patch.object(check, 'execute', side_effect=check.Refused('operation_failed')):
            with self.assertRaisesRegex(check.Refused, 'operation_failed'):
                qualification.daemon_reload('second', 'public')

    def test_profile_refuses_file_fallback_and_shared_reference(self):
        with tempfile.TemporaryDirectory() as tmp:
            qualification = check.Qualification.__new__(check.Qualification)
            qualification.state = Path(tmp)
            directory = qualification.state / 'default'
            directory.mkdir()
            manifest = {'backend': 'keychain', 'phase': 'ready', 'nostr_ref': 'first', 'sqlcipher_ref': 'second'}
            path = directory / 'profile.json'
            path.write_text(json.dumps(manifest))
            self.assertEqual(qualification.manifest('default'), manifest)
            (directory / 'keys').mkdir()
            with self.assertRaisesRegex(check.Refused, 'plaintext_fallback'):
                qualification.manifest('default')
            (directory / 'keys').rmdir()
            path.write_text(json.dumps(dict(manifest, sqlcipher_ref='first')))
            with self.assertRaisesRegex(check.Refused, 'separate_key_references'):
                qualification.manifest('default')

    def test_linux_restart_reaps_then_waits_for_bus_release_before_new_owner(self):
        provider = check.LinuxProvider(Path('/owned'), {})
        old, fresh = mock.Mock(), mock.Mock()
        provider.keyring = old
        calls = []
        with mock.patch.object(check, 'stop', side_effect=lambda child: calls.append(('stop', child))), \
                mock.patch.object(provider, 'wait_service', side_effect=lambda present: calls.append(('bus', present))), \
                mock.patch.object(check.subprocess, 'Popen', side_effect=lambda *a, **kw: calls.append(('start', kw)) or fresh):
            provider.lock()
        self.assertEqual(calls[:2], [('stop', old), ('bus', False)])
        self.assertEqual(calls[2][0], 'start')
        self.assertEqual(calls[2][1]['stdin'], subprocess.DEVNULL)
        self.assertEqual(calls[3], ('bus', True))

    def test_staged_inputs_must_match_before_any_provider_construction(self):
        with tempfile.TemporaryDirectory() as tmp:
            staging = Path(tmp)
            (staging / 'asset').write_bytes(b'changed')
            pins = staging / 'pins.json'
            pins.write_text(json.dumps({'asset': '0' * 64}))
            with mock.patch.object(check, 'Qualification') as constructor:
                with self.assertRaisesRegex(check.Refused, 'staged_input_changed'):
                    check.inner(staging / 'bootstrap', staging, staging / 'report', pins)
                constructor.assert_not_called()



class LifecycleProviderSupervisor(unittest.TestCase):
    def lifecycle_fixture(self, root, version="0.2.0"):
        root = root.resolve(strict=True)
        a_version = version + "-rc.1"
        a = root / 'a-assets'
        a.mkdir()
        (a / f'marmish-{a_version}-aarch64-apple-darwin').write_bytes(b'offline fixture')
        manifest = a / f'marmish-{a_version}-aarch64-apple-darwin.manifest'
        manifest.write_bytes(b'offline pinned manifest')
        qualification = check.LifecycleQualification(root, root / 'bootstrap', root / 'b-assets',
            a, check.digest(manifest), version)
        qualification.bins.mkdir()
        old, new = root / 'old-helper', root / 'new-helper'
        old.write_bytes(b'old actual path')
        new.write_bytes(b'new actual path')
        binary = qualification.bins / 'marmish'
        qualification.provider = mock.Mock()
        qualification.provider.start.return_value = {}
        qualification.wait_probe = mock.Mock()
        qualification.command = mock.Mock(side_effect=[(0, b'{"backend":"keychain"}'), (1, b''), (1, b'')])
        qualification.manifest = mock.Mock(return_value={'public_key': 'public'})
        qualification.items = mock.Mock(return_value={('nostr', 'a'), ('sqlcipher', 'b')})
        qualification.snapshot = mock.Mock(return_value=(b'profile', b'config'))
        reloads = []
        qualification.reload = mock.Mock(side_effect=lambda *_: reloads.append(binary.resolve()))
        qualification.upgrade_reload = mock.Mock(side_effect=lambda public: qualification.reload('default', public))
        def execute(argv, *_arguments, **_keywords):
            if 'install' in argv:
                version = argv[argv.index('--select') + 1] if '--select' in argv else a_version
                binary.unlink(missing_ok=True)
                binary.symlink_to(old if version == a_version else new)
            elif argv[0] == '/bin/sh':
                binary.unlink()
                binary.symlink_to(new)
            if argv[-1] == 'release-info':
                selected = a_version if binary.resolve() == old else qualification.b_version
                return 0, json.dumps({'version': selected}).encode()
            return 0, b'{}'
        return qualification, execute, reloads, old, new

    def test_lifecycle_fixture_accepts_alias_parent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'real').mkdir()
            (root / 'alias').symlink_to(root / 'real', target_is_directory=True)
            qualification, execute, reloads, old, new = self.lifecycle_fixture(root / 'alias')
            self.assertEqual(old, old.resolve(strict=True))
            with mock.patch.object(check, 'execute', side_effect=execute):
                result = {}
                qualification.run(result)
            self.assertEqual(reloads, [old, new, new, old, new, new])
            self.assertTrue(result['lifecycle_rollback_loaded'])

    def test_lifecycle_fresh_process_order_and_denial_not_qualified(self):
        for denied in (False, True):
            with self.subTest(denied=denied), tempfile.TemporaryDirectory() as tmp:
                qualification, execute, reloads, old, new = self.lifecycle_fixture(Path(tmp))
                result = {'lifecycle_upgrade_loaded': False, 'lifecycle_rollback_loaded': False,
                    'lifecycle_locked_denied': False}
                if denied:
                    qualification.upgrade_reload.side_effect = check.Refused('lifecycle_upgrade_locked_or_denied')
                with mock.patch.object(check, 'execute', side_effect=execute):
                    if denied:
                        with self.assertRaisesRegex(check.Refused, 'lifecycle_upgrade_locked_or_denied'):
                            qualification.run(result)
                        self.assertFalse(any(result[key] for key in ('lifecycle_upgrade_loaded',
                            'lifecycle_rollback_loaded', 'lifecycle_locked_denied')))
                        self.assertEqual(result['lifecycle_stage'], 'upgrade_reload')
                        qualification.provider.lock.assert_not_called()
                    else:
                        qualification.run(result)
                        self.assertEqual(reloads, [old, new, new, old, new, new])
                        self.assertTrue(all(result[key] for key in ('lifecycle_upgrade_loaded',
                            'lifecycle_rollback_loaded', 'lifecycle_locked_denied')))
                        qualification.provider.lock.assert_called_once()
                        qualification.provider.unlock.assert_called_once()
                        self.assertEqual(result['lifecycle_stage'], 'complete')

    def test_derived_provider_versions_and_closed_argument_triplet(self):
        self.assertFalse(check.lifecycle_arguments(None, None, None))
        for version in ('0.2.0', '0.2.1', '0.2.12', '0.2.18446744073709551615'):
            self.assertEqual(check.lifecycle_versions(version), (version, version + '-rc.1'))
            with tempfile.TemporaryDirectory() as tmp:
                qualification, execute, reloads, old, new = self.lifecycle_fixture(Path(tmp), version)
                with mock.patch.object(check, 'execute', side_effect=execute):
                    result = {}
                    qualification.run(result)
                self.assertTrue(result['lifecycle_upgrade_loaded'])
                self.assertEqual(reloads, [old, new, new, old, new, new])
        for args in ((Path('a'), None, None), (None, '0' * 64, None),
                     (None, None, '0.2.1'), (Path('a'), '0' * 64, None)):
            with self.assertRaises(check.Refused):
                check.lifecycle_arguments(*args)
        for version in ('0.2.01', '0.2.18446744073709551616', '0.2.100000000000000000000', '0.2.1-rc.1',
                        '0.2.1+build', '0.3.1', '0.2.1\n'):
            with self.subTest(version=version), self.assertRaises(check.Refused):
                check.lifecycle_arguments(Path('a'), '0' * 64, version)

    def test_invalid_lifecycle_cli_arguments_keep_finite_outer_failure(self):
        for args in (['--lifecycle-b-version', '0.2.1'],
                     ['--lifecycle-a-assets', '/unused', '--lifecycle-a-manifest-sha256', '0' * 64,
                      '--lifecycle-b-version', '0.2.18446744073709551616']):
            with tempfile.TemporaryDirectory() as tmp:
                report = Path(tmp) / 'result.json'
                run = subprocess.run([sys.executable, str(Path(check.__file__).resolve()),
                    '--bootstrap', '/unused', '--assets-dir', '/unused', '--report', str(report),
                    '--source-commit', 'a' * 40, *args], capture_output=True, timeout=5)
                self.assertEqual(run.returncode, 1)
                self.assertEqual(json.loads(report.read_bytes())['failure'],
                                 'qualification_prerequisite_failed')

    def test_lifecycle_a_pin_checked_before_any_product_execution(self):
        with tempfile.TemporaryDirectory() as tmp:
            qualification, _, _, _, _ = self.lifecycle_fixture(Path(tmp))
            qualification.a_pin = '0' * 64
            with mock.patch.object(check, 'execute') as run:
                with self.assertRaisesRegex(check.Refused, 'lifecycle_a_manifest_pin'):
                    qualification.run({})
                run.assert_not_called()

    def test_retained_helper_checks_inode_hash_and_selected_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            qualification, _, _, old, new = self.lifecycle_fixture(Path(tmp))
            info = old.stat()
            expected = info.st_dev, info.st_ino, info.st_size, check.digest(old)
            qualification.retained(old, expected)
            (qualification.bins / 'marmish').symlink_to(new)
            with self.assertRaisesRegex(check.Refused, 'lifecycle_old_helper_retained'):
                qualification.retained(old, expected, selected=True)
            replacement = old.with_name('replacement')
            replacement.write_bytes(old.read_bytes())
            replacement.replace(old)
            with self.assertRaisesRegex(check.Refused, 'lifecycle_old_helper_retained'):
                qualification.retained(old, expected)
            info = old.stat()
            expected = info.st_dev, info.st_ino, info.st_size, check.digest(old)
            old.write_bytes(new.read_bytes())
            with self.assertRaisesRegex(check.Refused, 'lifecycle_old_helper_retained'):
                qualification.retained(old, expected)

    def test_upgrade_error_classifier_is_private_exact_and_finite(self):
        for error, category in ((b'marmish: identity: locked_or_denied',
                'lifecycle_upgrade_locked_or_denied'),
                (b'private provider diagnostic', 'lifecycle_upgrade_reload_failed')):
            with self.subTest(category=category), tempfile.TemporaryDirectory() as tmp:
                qualification = check.LifecycleQualification.__new__(check.LifecycleQualification)
                qualification.bins, qualification.state = Path(tmp), Path(tmp) / 'state'
                qualification.environment = {}
                executable = Path(tmp) / 'marmish'
                executable.write_bytes(b'#!/bin/sh\nprintf "%s\\n" "' + error + b'" >&2\nexit 1\n')
                executable.chmod(0o700)
                with self.assertRaises(check.Refused) as caught:
                    qualification.upgrade_reload('public')
                self.assertEqual(str(caught.exception), category)
                self.assertNotIn('private provider diagnostic', str(caught.exception))

    def test_original_outer_budget_reserves_cleanup_and_clamps_inner(self):
        self.assertEqual(check.provider_inner_budget(100, 540, True, 100), (480, 420))
        self.assertEqual(check.provider_inner_budget(100, 540, True, 160), (420, 400))
        self.assertEqual(check.provider_inner_budget(100, 300, True, 160), (180, 160))
        self.assertEqual(check.provider_inner_budget(100, 600, False, 100), (570, 540))
        with self.assertRaisesRegex(check.Refused, 'owned_account_setup_deadline'):
            check.provider_inner_budget(100, 540, True, 550)

    def test_outer_result_keeps_exact_upgrade_denial_and_requires_all_proofs(self):
        result = {'ok': False, 'failure': 'lifecycle_upgrade_locked_or_denied'}
        with self.assertRaisesRegex(check.Refused, '^lifecycle_upgrade_locked_or_denied$'):
            check.provider_result_check(1, result, True)
        result = {'ok': True, 'lifecycle_stage': 'complete', 'lifecycle_upgrade_loaded': True,
            'lifecycle_rollback_loaded': True, 'lifecycle_locked_denied': True}
        check.provider_result_check(0, result, True)
        for key in ('lifecycle_upgrade_loaded', 'lifecycle_rollback_loaded', 'lifecycle_locked_denied'):
            with self.assertRaisesRegex(check.Refused, 'lifecycle_provider_incomplete'):
                check.provider_result_check(0, dict(result, **{key: False}), True)

    def test_signal_context_owns_original_budget_and_restores_on_failure(self):
        signals = (signal.SIGALRM, signal.SIGTERM, signal.SIGINT)
        before = tuple(signal.getsignal(sig) for sig in signals)
        with mock.patch.object(check.signal, 'alarm') as alarm:
            with self.assertRaisesRegex(RuntimeError, 'owned failure'):
                with check.ProviderSignals(True, 540) as control:
                    self.assertTrue(all(callable(signal.getsignal(sig)) for sig in signals))
                    control.limit_inner(400)
                    control.cleanup()
                    self.assertTrue(callable(signal.getsignal(signal.SIGTERM)))
                    self.assertNotEqual(signal.getsignal(signal.SIGTERM), signal.SIG_IGN)
                    signal.raise_signal(signal.SIGTERM)
                    self.assertTrue(control.cancelled)
                    raise RuntimeError('owned failure')
            self.assertEqual([call.args[0] for call in alarm.call_args_list], [60, 400, 0, 0])
        self.assertEqual(tuple(signal.getsignal(sig) for sig in signals), before)

    def test_exact_staging_pins_and_cleanup_even_when_privileged_copy_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bootstrap = root / 'bootstrap.sh'
            bootstrap.write_bytes(b'offline bootstrap fixture')
            directories = []
            for version in ('0.2.1', '0.2.1-rc.1'):
                directory = root / version
                directory.mkdir()
                suffix = version + '-aarch64-apple-darwin'
                for name in ('marmish', 'marmishd', 'marmish-worker', 'THIRD-PARTY-LICENSES'):
                    (directory / (name + '-' + suffix)).write_bytes(name.encode())
                (directory / ('marmish-' + suffix + '.manifest')).write_bytes(b'manifest')
                directories.append(directory)
            stage = None
            with self.assertRaisesRegex(RuntimeError, 'privileged copy failed'):
                with check.staged_provider_inputs(bootstrap, *directories, lifecycle_b_version="0.2.1") as stage:
                    pins = json.loads((stage / 'input-pins.json').read_bytes())
                    self.assertEqual(len(pins), 13)
                    self.assertEqual(sum(name.startswith('assets/') for name in pins), 5)
                    self.assertEqual(sum(name.startswith('a-assets/') for name in pins), 5)
                    self.assertTrue(all(check.digest(stage / name) == pin for name, pin in pins.items()))
                    raise RuntimeError('privileged copy failed')
            self.assertFalse(stage.exists())
            (directories[1] / 'unexpected').touch()
            with self.assertRaisesRegex(check.Refused, 'lifecycle_asset_set'):
                with check.staged_provider_inputs(bootstrap, *directories, lifecycle_b_version="0.2.1"):
                    self.fail('unexpected source accepted')


if __name__ == '__main__':
    unittest.main()
