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



class ProviderDiagnostics(unittest.TestCase):
    def fixture(self, report, failure=None, cleanup_failure=None, changed_runner=False, inner=None, cancelled=False, calls=None):
        from contextlib import ExitStack, nullcontext
        from types import SimpleNamespace
        seen = []
        original_stage = check.provider_stage
        lookups = 0
        def stage(result, name):
            original_stage(result, name)
            seen.append(name)
            if name == failure or name == cleanup_failure:
                raise check.Refused('operation_failed', exit_code=7)
        def account(name):
            nonlocal lookups
            lookups += 1
            if lookups == 1:
                raise KeyError(name)
            return SimpleNamespace(pw_uid=55000, pw_dir='/Users/' + name)
        def execute(argv, *args, **kwargs):
            if calls is not None:
                calls.append(list(argv))
            if argv[:2] == ['/usr/bin/dscl', '.']:
                return 0, b'runner 501\n'
            if argv[0] == '/usr/bin/xcrun':
                return 0, b'/usr/bin/python3\n'
            if '/bin/cat' in argv:
                return 0, json.dumps(inner or {'kind':'installed_auto_provider','ok':True}).encode()
            if argv[0] == '/usr/bin/security':
                return 0, b'changed' if changed_runner and seen[-1].startswith('cleanup_') else b'runner'
            return 0, b''
        with ExitStack() as patches:
            patches.enter_context(mock.patch.object(check, 'provider_stage', side_effect=stage))
            patches.enter_context(mock.patch.object(check, 'execute', side_effect=execute))
            patches.enter_context(mock.patch.object(check.pwd, 'getpwnam', side_effect=account))
            patches.enter_context(mock.patch.object(check.pwd, 'getpwuid', return_value=SimpleNamespace(pw_dir='/runner')))
            patches.enter_context(mock.patch.object(check, 'staged_provider_inputs', return_value=nullcontext(Path('/stage'))))
            patches.enter_context(mock.patch.object(check, 'digest', return_value='a' * 64))
            patches.enter_context(mock.patch.object(Path, 'exists', return_value=False))
            patches.enter_context(mock.patch.object(Path, 'is_symlink', return_value=False))
            patches.enter_context(mock.patch.object(Path, 'is_file', return_value=True))
            patches.enter_context(mock.patch.object(Path, 'lstat', return_value=SimpleNamespace(st_uid=55000)))
            if cancelled:
                patches.enter_context(mock.patch.object(check, 'ProviderSignals', return_value=
                    nullcontext(SimpleNamespace(cancelled=True, cleanup=lambda:None, limit_inner=lambda _:None))))
            code = check.mac_account(Path('/bootstrap'), Path('/assets'), report)
        return code, json.loads(report.read_text()), seen

    def test_fresh_private_home_creation_precedes_owner_only_chown(self):
        calls = []
        with tempfile.TemporaryDirectory() as tmp:
            code, result, _ = self.fixture(Path(tmp) / 'report.json', calls=calls)
        self.assertEqual(code, 0, result)
        chowns = [argv for argv in calls if '/usr/sbin/chown' in argv]
        self.assertEqual(len(chowns), 1)
        chown = chowns[0]
        home = chown[-1]
        self.assertTrue(home.startswith('/Users/marmishq'))
        self.assertEqual(chown, ['/usr/bin/sudo', '-n', '/usr/sbin/chown', '55000', home])
        self.assertNotIn(':', chown[3])
        mkdir = ['/usr/bin/sudo', '-n', '/bin/mkdir', '-m', '700', home]
        self.assertEqual(calls.count(mkdir), 1)
        self.assertLess(calls.index(mkdir), calls.index(chown))
        self.assertIn(['/usr/bin/sudo', '-n', '/usr/bin/dscl', '.', '-create',
            home, 'PrimaryGroupID', '-2'], calls)

    def test_every_outer_stage_has_finite_failure_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / 'report.json'
            code, result, seen = self.fixture(report)
            self.assertEqual((code, result['ok']), (0, True), result)
            self.assertEqual(set(seen), check.STAGES - {'diagnostic_invalid'})
            for stage in sorted(set(seen)):
                with self.subTest(stage=stage):
                    cleanup = stage.startswith('cleanup_')
                    code, result, _ = self.fixture(report,
                        failure=None if cleanup else stage, cleanup_failure=stage if cleanup else None)
                    self.assertEqual(code, 1, result)
                    self.assertFalse(result['ok'])
                    prefix = 'cleanup_' if cleanup else ''
                    self.assertEqual(result[prefix + 'failure_stage'], stage)
                    self.assertEqual(result[prefix + 'exit_code'], 7)
                    self.assertNotIn('_stage', result)
                    self.assertNotIn('SENTINEL', json.dumps(result))

    def test_primary_failure_survives_cleanup_failure_and_changed_runner_is_red(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / 'report.json'
            _, result, _ = self.fixture(report, failure='inner_launch', cleanup_failure='cleanup_bootout')
            self.assertEqual(result['failure_stage'], 'inner_launch')
            self.assertEqual(result['cleanup_failure_stage'], 'cleanup_bootout')
            self.assertFalse(result['owned_account_removed'])
            _, result, _ = self.fixture(report, changed_runner=True)
            self.assertFalse(result['ok'])
            self.assertFalse(result['runner_keychain_unchanged'])
            self.assertEqual(result['cleanup_failure_stage'], 'cleanup_runner_compare')

    def test_actual_exit_status_is_finite_and_timeout_has_no_status(self):
        with self.assertRaises(check.Refused) as failed:
            check.execute(['/bin/sh', '-c', 'echo SENTINEL >&2; exit 7'], {})
        self.assertEqual(str(failed.exception), 'operation_failed')
        self.assertEqual(failed.exception.exit_code, 7)
        with self.assertRaises(check.Refused) as timeout:
            check.execute(['/bin/sh', '-c', 'exec /bin/sleep 5'], {}, timeout=0.01)
        self.assertIsNone(timeout.exception.exit_code)
        self.assertNotIn('SENTINEL', str(failed.exception))

    def test_sanitizer_is_closed_idempotent_and_does_not_invent_early_stage(self):
        for invalid in ('SENTINEL', [], None, 7):
            value = {'failure_stage': invalid, 'cleanup_failure_stage': invalid,
                'exit_code': True, 'cleanup_exit_code': 256, '_stage': 'SENTINEL'}
            sanitized = check.sanitize_provider_report(value)
            self.assertEqual(sanitized, {'failure_stage':'diagnostic_invalid',
                'cleanup_failure_stage':'diagnostic_invalid'})
            # The main rewrite reads the already-sanitized JSON and sanitizes again.
            self.assertEqual(check.sanitize_provider_report(json.loads(json.dumps(sanitized))), sanitized)
        early = {}
        check.provider_failure(early, ValueError('SENTINEL'))
        self.assertNotIn('failure_stage', check.sanitize_provider_report(early))
        self.assertNotIn('SENTINEL', json.dumps(early))
        for code in (-128, -6, 0, 7, 255):
            self.assertEqual(check.sanitize_provider_report({'exit_code':code}), {'exit_code':code})
        source = Path(check.__file__).read_text()
        self.assertIn('json.dumps(sanitize_provider_report(value)', source)

    def test_inner_failure_cannot_replace_supervisor_category_or_leak_sentinel(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, result, _ = self.fixture(Path(tmp) / 'report.json', inner={
                'kind':'installed_auto_provider','ok':False,'failure':'SENTINEL inner text'})
        self.assertEqual(result['failure'], 'owned_account_qualification_failed')
        self.assertEqual(result['failure_stage'], 'qualification_validate')
        self.assertNotIn('SENTINEL', json.dumps(result))

    def test_stop_error_replaces_exit_without_fabricating_status(self):
        result = {'ok':True}
        check.provider_stage(result, 'inner_launch')
        with mock.patch.object(check, 'stop', side_effect=check.Refused('child_cleanup_incomplete')):
            try:
                check.execute(['/bin/sh', '-c', 'echo SENTINEL >&2; exit 7'], {})
            except check.Refused as error:
                self.assertIsNone(error.exit_code)
                check.provider_failure(result, error)
            else:
                self.fail('expected cleanup failure')
        self.assertEqual(result['failure'], 'child_cleanup_incomplete')
        self.assertEqual(result['failure_stage'], 'inner_launch')
        self.assertNotIn('exit_code', result)
        self.assertNotIn('SENTINEL', json.dumps(result))

    def test_signal_override_drops_stale_primary_stage_and_status(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, result, _ = self.fixture(Path(tmp) / 'report.json', failure='inner_launch', cancelled=True)
        self.assertEqual(result['failure'], 'lifecycle_provider_signal')
        self.assertNotIn('failure_stage', result)
        self.assertNotIn('exit_code', result)


class ChildDiagnostics(unittest.TestCase):
    def inner_fixture(self, action, cleanup=None):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            report = home / 'result.json'
            provider_root = home / 'provider'
            provider_root.mkdir(mode=0o700)
            provider = check.MacProvider(provider_root, {'HOME':tmp})
            qualification = SimpleNamespace(provider=provider)
            qualification.run = lambda result: action(qualification, result)
            if cleanup is not None:
                provider.close = lambda: cleanup(provider)
            with mock.patch.object(check.pwd, 'getpwuid', return_value=SimpleNamespace(pw_dir=tmp)), \
                    mock.patch.object(check, 'Qualification', return_value=qualification), \
                    mock.patch.object(check.signal, 'signal'), mock.patch.object(check.signal, 'alarm'):
                code = check.inner(home / 'bootstrap', home / 'assets', report)
            return code, json.loads(report.read_text())

    def test_specific_mac_failure_and_cleanup_keep_first_stage_status(self):
        def action(q, result):
            q.provider.start()
        with mock.patch.object(check, 'execute', side_effect=check.Refused('operation_failed', 7)):
            code, result = self.inner_fixture(action)
        self.assertEqual(code, 1)
        self.assertEqual(result['child_failure'], 'operation_failed')
        self.assertEqual(result['child_failure_stage'], 'mac_create_keychain')
        self.assertEqual(result['child_exit_code'], 7)
        self.assertNotIn('_stage', result)
        self.assertFalse(result['owned_children_reaped'])

    def test_generic_primary_cannot_acquire_cleanup_status(self):
        def action(q, result):
            check.child_stage(result, 'quickstart')
            raise RuntimeError('SENTINEL exception')
        def cleanup(provider):
            check.child_stage(provider._diagnostics, 'mac_delete_keychain')
            raise check.Refused('operation_failed', 9)
        _, result = self.inner_fixture(action, cleanup)
        self.assertEqual(result['child_failure'], 'provider_qualification_failed')
        self.assertEqual(result['child_failure_stage'], 'quickstart')
        self.assertNotIn('child_exit_code', result)
        self.assertNotIn('SENTINEL', json.dumps(result))

    def test_cleanup_only_records_precise_mac_stage_real_status(self):
        def cleanup(provider):
            check.child_stage(provider._diagnostics, 'mac_delete_keychain')
            raise check.Refused('operation_failed', 8)
        _, result = self.inner_fixture(lambda *_:None, cleanup)
        self.assertEqual(result['child_failure_stage'], 'mac_delete_keychain')
        self.assertEqual(result['child_failure'], 'operation_failed')
        self.assertEqual(result['child_exit_code'], 8)

    def test_success_shape_legacy_and_optional_tracker(self):
        _, result = self.inner_fixture(lambda *_:None, lambda _:None)
        self.assertTrue(result['ok'])
        self.assertFalse(any(key.startswith('child_') or key == '_stage' for key in result))
        self.assertEqual(check.sanitize_child_diagnostics({'ok':False}), {'ok':False})
        provider = check.MacProvider(Path('/owned'), {})
        provider._diagnostics = None
        with mock.patch.object(check, 'execute', return_value=(0,b'')):
            provider.security('lock-keychain', 'SENTINEL', stage='mac_lock_keychain')

    def test_generic_exception_cannot_supply_checked_subprocess_status(self):
        error = RuntimeError('SENTINEL custom status')
        error.exit_code = 7
        result = {'_stage':'install'}
        check.child_failure(result, error)
        self.assertNotIn('child_exit_code', result)
        self.assertEqual(result['child_failure'], 'provider_qualification_failed')
        self.assertNotIn('SENTINEL', json.dumps(check.sanitize_provider_report(result)))

    def test_closed_boundary_sanitizes_malformed_and_outer_only_values(self):
        for value in ('SENTINEL', 'inner_launch', [], None, True):
            result={'ok':False,'child_failure':value,'child_failure_stage':value,'child_exit_code':True}
            check.sanitize_provider_report(result)
            self.assertEqual(result, {'ok':False,'child_failure':'diagnostic_invalid',
                'child_failure_stage':'diagnostic_invalid'})
        for status in (-129,256,True,'7',None):
            result={'child_failure':'operation_failed','child_failure_stage':'install','child_exit_code':status}
            self.assertNotIn('child_exit_code', check.sanitize_child_diagnostics(result))
        self.assertEqual(check.sanitize_child_diagnostics({'child_exit_code':7}), {
            'child_failure':'diagnostic_invalid','child_failure_stage':'diagnostic_invalid','child_exit_code':7})
        self.assertEqual(check.sanitize_child_diagnostics({'ok':True,'child_failure':'SENTINEL',
            'child_failure_stage':'install','child_exit_code':7}), {'ok':True})
        for category in ('lifecycle_upgrade_reload_failed','absent_provider_category','private_bus_start'):
            self.assertEqual(check.sanitize_child_diagnostics({'child_failure':category})['child_failure'],
                'diagnostic_invalid')

    def test_outer_preserves_only_validated_child_tuple_and_own_primary(self):
        harness = ProviderDiagnostics()
        with tempfile.TemporaryDirectory() as tmp:
            _, result, _ = harness.fixture(Path(tmp) / 'report.json', inner={
                'kind':'installed_auto_provider','ok':False,'failure':'SENTINEL raw',
                'child_failure':'operation_failed','child_failure_stage':'install','child_exit_code':7})
        self.assertEqual(result['failure'], 'owned_account_qualification_failed')
        self.assertEqual(result['failure_stage'], 'qualification_validate')
        self.assertEqual(result['child_failure_stage'], 'install')
        self.assertEqual(result['child_exit_code'], 7)
        self.assertNotIn('SENTINEL', json.dumps(result))

    def test_nonsecurity_install_does_not_keep_stale_mac_stage(self):
        from types import SimpleNamespace
        result = {}
        provider = SimpleNamespace()
        def start():
            check.child_stage(result, 'mac_default_verify')
            return {}
        provider.start = start
        qualification = check.Qualification.__new__(check.Qualification)
        qualification.provider = provider
        qualification.bootstrap, qualification.assets = Path('/SENTINEL'), Path('/assets')
        qualification.prefix, qualification.bins = Path('/prefix'), Path('/bin')
        with mock.patch.object(check, 'execute', side_effect=check.Refused('operation_failed', 7)):
            with self.assertRaises(check.Refused) as failed:
                qualification.run(result)
        check.child_failure(result, failed.exception)
        self.assertEqual(result['child_failure_stage'], 'install')
        self.assertNotIn('SENTINEL', json.dumps(check.sanitize_provider_report(result)))
        lifecycle = check.LifecycleQualification.__new__(check.LifecycleQualification)
        lifecycle.provider = provider
        lifecycle.a_assets = mock.Mock()
        lifecycle.a_assets.glob.side_effect = RuntimeError('SENTINEL')
        with self.assertRaises(RuntimeError):
            lifecycle.run(result)
        self.assertEqual(result['_stage'], 'qualification_remaining')

    def test_report_parser_failure_is_at_quickstart_validation(self):
        from types import SimpleNamespace
        qualification = check.Qualification.__new__(check.Qualification)
        qualification.provider = SimpleNamespace(start=lambda:{})
        qualification.bootstrap, qualification.assets = Path('/bootstrap'), Path('/assets')
        qualification.prefix, qualification.bins = Path('/prefix'), Path('/bin')
        qualification.state = Path('/state')
        qualification.wait_probe = lambda:None
        result = {}
        with mock.patch.object(check, 'execute', return_value=(0,b'')), \
                mock.patch.object(check, 'interactive', return_value='SENTINEL'), \
                mock.patch.object(check, 'reports', side_effect=RuntimeError('SENTINEL parser')):
            with self.assertRaises(RuntimeError) as failed:
                qualification.run(result)
        check.child_failure(result, failed.exception)
        self.assertEqual(result['child_failure_stage'], 'quickstart_validate')
        self.assertNotIn('SENTINEL', json.dumps(check.sanitize_provider_report(result)))

    def test_real_exit_and_stop_replacement_use_child_stage_without_raw_output(self):
        for stop_error in (False, True):
            result = {'_stage':'install'}
            original = check.stop
            def stopping(child):
                original(child)
                if stop_error:
                    raise check.Refused('child_cleanup_incomplete')
            with mock.patch.object(check, 'stop', side_effect=stopping):
                try:
                    check.execute(['/bin/sh','-c','echo SENTINEL >&2; exit 7'], {})
                except check.Refused as error:
                    check.child_failure(result, error)
            check.sanitize_provider_report(result)
            self.assertEqual(result['child_failure_stage'], 'install')
            self.assertEqual(result.get('child_exit_code'), None if stop_error else 7)
            self.assertNotIn('SENTINEL', json.dumps(result))


class MacPreferencePreparation(unittest.TestCase):
    def fixture(self, home):
        from types import SimpleNamespace
        root = home / 'provider'
        root.mkdir(mode=0o700)
        provider = check.MacProvider(root, {'HOME':str(home)})
        provider._diagnostics = {}
        return provider, mock.patch.object(check.pwd, 'getpwuid',
            return_value=SimpleNamespace(pw_dir=str(home)))

    def test_private_parents_ready_before_original_security_sequence(self):
        import stat
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            provider, account = self.fixture(home)
            calls = []
            def execute(argv, *_args, **_kwargs):
                for directory in (home / 'Library', home / 'Library' / 'Preferences'):
                    info = directory.lstat()
                    self.assertTrue(stat.S_ISDIR(info.st_mode))
                    self.assertEqual(stat.S_IMODE(info.st_mode), 0o700)
                    self.assertEqual(info.st_uid, check.os.geteuid())
                calls.append(argv)
                return 0, ('"' + str(provider.keychain) + '"').encode()
            with account, mock.patch.object(check, 'execute', side_effect=execute):
                provider.start()
                provider.start()  # Existing valid private directories are left intact.
            expected = [
                ['/usr/bin/security','create-keychain','-p',check.PASSWORD.decode().strip(),str(provider.keychain)],
                ['/usr/bin/security','default-keychain','-d','user','-s',str(provider.keychain)],
                ['/usr/bin/security','list-keychains','-d','user','-s',str(provider.keychain)],
                ['/usr/bin/security','unlock-keychain','-p',check.PASSWORD.decode().strip(),str(provider.keychain)],
                ['/usr/bin/security','set-keychain-settings',str(provider.keychain)],
                ['/usr/bin/security','default-keychain','-d','user']]
            self.assertEqual(calls, expected + expected)

    def test_invalid_existing_paths_refuse_before_any_security(self):
        for invalid in ('symlink','file','mode','foreign_uid','home_mismatch','home_symlink','root_symlink','unrelated_root'):
            with self.subTest(invalid=invalid), tempfile.TemporaryDirectory() as tmp:
                from contextlib import ExitStack
                from types import SimpleNamespace
                home = Path(tmp)
                provider, account = self.fixture(home)
                library = home / 'Library'
                if invalid == 'symlink': library.symlink_to(provider.root, target_is_directory=True)
                if invalid == 'file': library.write_text('SENTINEL')
                if invalid == 'mode':
                    library.mkdir(mode=0o755)
                    library.chmod(0o755)  # Keep this invalid fixture independent of umask.
                if invalid == 'home_mismatch': provider.environment['HOME'] = str(home / 'other')
                if invalid == 'home_symlink':
                    alias = home / 'alias'
                    alias.symlink_to(home, target_is_directory=True)
                    provider.environment['HOME'] = str(alias)
                if invalid == 'root_symlink':
                    provider.root.rmdir()
                    provider.root.symlink_to(home, target_is_directory=True)
                if invalid == 'unrelated_root': provider.root = home.parent / 'other'
                with ExitStack() as stack:
                    stack.enter_context(account)
                    if invalid == 'foreign_uid':
                        original = Path.lstat
                        def lstat(path, *args, **kwargs):
                            info = original(path, *args, **kwargs)
                            return SimpleNamespace(st_uid=check.os.geteuid()+1,st_mode=info.st_mode) if path == home else info
                        stack.enter_context(mock.patch.object(Path,'lstat',lstat))
                    execute = stack.enter_context(mock.patch.object(check,'execute'))
                    with self.assertRaises(check.Refused) as failed:
                        provider.start()
                self.assertEqual(str(failed.exception),'owned_preferences_directory')
                self.assertIsNone(failed.exception.exit_code)
                self.assertEqual(provider._diagnostics['_stage'],'mac_preferences_prepare')
                execute.assert_not_called()

    def test_mkdir_race_and_oserror_are_finite_without_security_or_status(self):
        for race in (False, True):
            with self.subTest(race=race), tempfile.TemporaryDirectory() as tmp:
                provider, account = self.fixture(Path(tmp))
                original = Path.mkdir
                def mkdir(path, *args, **kwargs):
                    if race:
                        original(path, *args, **kwargs)
                        raise FileExistsError('SENTINEL raced')
                    raise PermissionError('SENTINEL permission')
                with account, mock.patch.object(Path,'mkdir',mkdir), mock.patch.object(check,'execute') as execute:
                    with self.assertRaises(check.Refused) as failed:
                        provider.start()
                check.child_failure(provider._diagnostics, failed.exception)
                report = check.sanitize_provider_report(provider._diagnostics)
                self.assertEqual(report, {'child_failure':'owned_preferences_directory',
                    'child_failure_stage':'mac_preferences_prepare'})
                self.assertNotIn('SENTINEL',json.dumps(report))
                execute.assert_not_called()


class QualificationUtilityDiscovery(unittest.TestCase):
    def test_fixed_system_discovery_preserves_private_runtime_path(self):
        expected_path = '/usr/bin:/bin:/usr/sbin:/sbin'
        self.assertEqual(check.UTILITY_DISCOVERY_PATH, expected_path)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            decoy = root / 'decoy'
            decoy.mkdir()
            for name in ('python3', 'curl'):
                (decoy / name).write_text('decoy')
                (decoy / name).chmod(0o700)
            found = {name: ('/usr/sbin/sysctl' if name == 'sysctl' else '/usr/bin/' + name)
                for name in check.UTILITIES if name != 'sha256sum'}
            def discover(name, *, path):
                self.assertEqual(path, expected_path)
                if name == 'sysctl' and '/usr/sbin' not in path.split(':'):
                    return None
                return found.get(name)
            with mock.patch.dict(check.os.environ, {'PATH': str(decoy)}), \
                    mock.patch.object(check.shutil, 'which', side_effect=discover) as which:
                qualification = check.Qualification(root, root / 'bootstrap', root / 'assets')
                self.assertEqual(check.os.environ['PATH'], str(decoy))
            self.assertEqual(which.call_args_list,
                [mock.call(name, path=expected_path) for name in check.UTILITIES])
            utilities = root / 'utilities'
            self.assertEqual(qualification.environment['PATH'], str(utilities))
            self.assertEqual({entry.name for entry in utilities.iterdir()}, set(found))
            for name, target in found.items():
                self.assertTrue((utilities / name).is_symlink())
                self.assertEqual(check.os.readlink(utilities / name), target)
            self.assertFalse((utilities / 'sha256sum').is_symlink())
            self.assertFalse((utilities / 'python3').is_symlink())
            self.assertFalse((utilities / 'python').is_symlink())

    def test_missing_utilities_remain_absent(self):
        with tempfile.TemporaryDirectory() as directory, \
                mock.patch.object(check.shutil, 'which', return_value=None) as which:
            root = Path(directory)
            qualification = check.Qualification(root, root / 'bootstrap', root / 'assets')
            self.assertEqual(list((root / 'utilities').iterdir()), [])
            self.assertEqual(qualification.environment['PATH'], str(root / 'utilities'))
            self.assertEqual(which.call_count, len(check.UTILITIES))


class QuickstartFailureDiagnostics(unittest.TestCase):
    def snapshot(self, output=b'', status=None):
        return check._fixture.interactive_snapshot(RuntimeError('quickstart_terminal_failure'),
            output, 2, status)

    def test_exact_complete_lines_and_sentinel_exclusion(self):
        raw = (b'/secret/SENTINEL npubSENTINEL errno SENTINEL\n'
            b'{"secret":"SENTINEL"}\n'
            b'marmish installer debug: stage=setup_choices\r\n'
            b'marmish: identity: locked_or_denied\r\n'
            b'marmish installer: native_setup_failed (stage=setup_choices)\r\n'
            b'marmish: identity: missing')
        value = self.snapshot(raw, 7 << 8)
        self.assertEqual(value, {'reason':'terminal_failure', 'prompts_seen':2,
            'exit_code':7, 'installer_stage':'setup_choices',
            'installer_failure':'native_setup_failed', 'native_failure':'identity_locked_or_denied'})
        self.assertNotIn('SENTINEL', json.dumps(value))
        for raw in (b'marmish: identity: SENTINEL\n', b'marmish: identity: missing\x00\n',
                b'marmish installer debug: stage=SENTINEL\n',
                b'marmish installer: SENTINEL (stage=SENTINEL)\n'):
            value = self.snapshot(raw)
            self.assertIn('diagnostic_invalid', value.values())
            self.assertNotIn('SENTINEL', json.dumps(value))
        for raw in (b'prefix marmish: identity: missing\n', b'marmish: identity: missing',
                b'\x1bmarmish: identity: missing\n', b'\xff\n', b'x' * 2048 + b'\n'):
            self.assertEqual(self.snapshot(raw)['native_failure'], 'absent')

    def test_native_families_are_finite(self):
        for text, expected in ((b'local I/O failed','local_io'),
                (b'operation deadline exceeded','timeout'), (b'storage: SENTINEL','storage'),
                (b'transport: SENTINEL','transport'), (b'marmot: SENTINEL','marmot'),
                (b'invalid input: SENTINEL','invalid_input'), (b'unknown SENTINEL','diagnostic_invalid')):
            value = self.snapshot(b'marmish: ' + text + b'\n')
            self.assertEqual(value['native_failure'], expected)
            self.assertNotIn('SENTINEL', json.dumps(value))

    def test_nested_reports_strict_bounds_stage_gate_and_success_strip(self):
        # Raw wait status 9 is SIGKILL; use actual status in the valid tuple.
        valid = self.snapshot(b'marmish: identity: missing\n', 9)
        self.assertEqual(valid['exit_code'], -9)
        for hostile in ({'SENTINEL':'SENTINEL'}, None, [],
                {**valid,'extra':'SENTINEL'}, {**valid,'prompts_seen':True, 'exit_code':True},
                {**valid,'prompts_seen':999999, 'exit_code':999999}):
            report = {'ok':False,'child_failure':'provider_qualification_failed',
                'child_failure_stage':'quickstart', 'child_quickstart':hostile,
                '_quickstart_diagnostics':{'raw':'SENTINEL'}, '_other':'SENTINEL'}
            check.sanitize_provider_report(report)
            self.assertNotIn('SENTINEL', json.dumps(report))
            self.assertLess(len(json.dumps(report['child_quickstart'])), 1024)
            self.assertNotIn('_other', report)
            self.assertNotIn('extra', report['child_quickstart'])
            self.assertNotEqual(type(report['child_quickstart'].get('exit_code')), bool)
        for report in ({'ok':True,'child_quickstart':valid},
                {'ok':False,'child_quickstart':valid},
                {'ok':False,'child_failure':'operation_failed','child_failure_stage':'install','child_quickstart':valid}):
            check.sanitize_provider_report(report)
            self.assertNotIn('child_quickstart', report)

    def test_snapshot_once_parser_fault_and_first_failure_immutable(self):
        target = {}
        check._fixture.capture_interactive_failure(target, PermissionError('SENTINEL'), b'', 0, None)
        original = dict(target)
        check._fixture.capture_interactive_failure(target, OSError('SENTINEL'), b'', 4, 0)
        self.assertEqual(target, original)
        self.assertEqual(target['reason'], 'permission_failed')
        self.assertNotIn('exit_code', target)
        with mock.patch.object(check._fixture, 'interactive_snapshot', side_effect=ValueError('SENTINEL')):
            value = {}
            check._fixture.capture_interactive_failure(value, RuntimeError(), b'', 0, None)
        self.assertNotIn('SENTINEL', json.dumps(value))
        result = {'_stage':'quickstart','_quickstart_diagnostics':original}
        check.child_failure(result, RuntimeError('SENTINEL'))
        first = dict(result['child_quickstart'])
        result['_stage'] = 'mac_delete_keychain'
        result['_quickstart_diagnostics'] = {'raw':'SENTINEL'}
        check.child_failure(result, check.Refused('operation_failed', 7), cleanup=True)
        self.assertEqual(result['child_quickstart'], first)
        self.assertEqual(result['child_failure_stage'], 'quickstart')

    def test_real_0644_script_nonzero_signal_timeout_and_optional_off(self):
        for script, code, reason in (('exit 7',7,'terminal_failure'),
                ('kill -9 $$',-9,'terminal_failure'), ('exec /bin/sleep 5',None,'observation_limit'),
                ('exit 0',0,'terminal_failure')):
            with self.subTest(script=script), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'bootstrap'
                path.write_text(script + '\n')
                path.chmod(0o644)
                diagnostics = {}
                with self.assertRaises(RuntimeError):
                    check.interactive([str(path)], {'PATH':'/bin'},
                        timeout=0.08 if code is None else 2,
                        failure_diagnostics=diagnostics)
                self.assertEqual(diagnostics['reason'], reason)
                self.assertEqual(diagnostics.get('exit_code'), code)
                self.assertEqual(diagnostics['prompts_seen'], 0)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'bootstrap'
            # Exercise all unchanged prompt gates with no product or account.
            path.write_text(''.join("printf '%s' " + "'" + prompt.decode().replace("'", "'\\''") +
                "'; read answer\n" for prompt, _ in check._fixture.DIALOG))
            path.chmod(0o644)
            without = check.interactive([str(path)], {'PATH':'/bin'}, timeout=2)
            diagnostics = {}
            with_diagnostics = check.interactive([str(path)], {'PATH':'/bin'}, timeout=2,
                failure_diagnostics=diagnostics)
            self.assertEqual(without, with_diagnostics)
            self.assertEqual(diagnostics, {})

    def test_outer_copy_revalidates_nested_tuple_without_leaks(self):
        raw = self.snapshot(b'marmish: identity: locked_or_denied\n', 7 << 8)
        raw['native_failure'] = 'SENTINEL /secret npubSENTINEL'
        for ok, stage in ((False,'quickstart'), (False,'install'), (True,'quickstart')):
            with self.subTest(ok=ok, stage=stage), tempfile.TemporaryDirectory() as directory:
                _, report, _ = ProviderDiagnostics().fixture(Path(directory) / 'report', inner={
                    'kind':'installed_auto_provider', 'ok':ok,
                    'child_failure':'provider_qualification_failed', 'child_failure_stage':stage,
                    'child_quickstart':raw, '_quickstart_diagnostics':{'raw':'SENTINEL'}})
                self.assertNotIn('SENTINEL', json.dumps(report))
                if not ok and stage == 'quickstart':
                    self.assertEqual(report['child_quickstart']['native_failure'], 'diagnostic_invalid')
                    self.assertEqual(report['child_quickstart']['exit_code'], 7)
                else:
                    self.assertNotIn('child_quickstart', report)

    def test_fork_failure_diagnostic_and_cleanup_cannot_replace_snapshot(self):
        with mock.patch.object(check._fixture.pty, 'fork', side_effect=PermissionError('SENTINEL')):
            diagnostics = {}
            with self.assertRaises(PermissionError):
                check.interactive([], {}, failure_diagnostics=diagnostics)
            self.assertEqual(diagnostics['reason'], 'permission_failed')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'bootstrap'
            path.write_text('exit 7\n')
            diagnostics = {}
            original_close = check._fixture.os.close
            def close(fd):
                original_close(fd)
                raise OSError('SENTINEL cleanup')
            with mock.patch.object(check._fixture.os, 'close', side_effect=close):
                with self.assertRaises(OSError):
                    check.interactive([str(path)], {'PATH':'/bin'}, timeout=2,
                        failure_diagnostics=diagnostics)
            self.assertEqual(diagnostics['exit_code'], 7)
            self.assertEqual(diagnostics['reason'], 'terminal_failure')
            self.assertNotIn('SENTINEL', json.dumps(diagnostics))


if __name__ == '__main__':
    unittest.main()
