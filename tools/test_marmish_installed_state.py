"""Ownership and real subprocess tests for installed CI's private boundary."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import marmish_installed_state as state
from mdk_smoke import SmokeError
from test_marmish_install import archive_bytes, load_installer, VERSION, TARGET


class InstallerHandoff(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.pointer = self.base / 'pointer.json'
        self.ctx = {'run': '123', 'attempt': '2', 'sha': 'a' * 40}
        self.raw = b'#!/bin/sh\nexit 0\n'
        self.digest = hashlib.sha256(self.raw).hexdigest()

    def test_verified_handoff_skips_unsafe_checkout_but_default_remains_strict(self):
        checkout = self.base / 'checkout'
        (checkout / 'tools').mkdir(parents=True)
        (checkout / 'scripts').mkdir()
        (checkout / 'scripts/install.sh').write_bytes(self.raw)
        checkout.chmod(0o775)
        with patch.object(state, '__file__', str(checkout / 'tools/state.py')):
            with self.assertRaisesRegex(SmokeError, 'unsafe_state_path'):
                state.create_state(self.pointer, self.ctx, 'b' * 64, 'linux-x64')
            self.assertFalse(self.pointer.exists())
            value = state.create_state(self.pointer, self.ctx, 'b' * 64, 'linux-x64',
                installer_bytes=self.raw, installer_sha256=self.digest)
        self.addCleanup(lambda: shutil.rmtree(value['root'], ignore_errors=True))
        self.assertEqual(state.load_state(self.pointer), value)
        self.assertEqual(state.read_private(value['installer']), self.raw)
        self.assertEqual(value['installer_sha256'], self.digest)
        self.assertEqual(Path(value['installer']).stat().st_mode & 0o777, 0o600)

    def test_invalid_handoff_rejected_before_snapshot_or_any_root_allocation(self):
        cases = [dict(installer_bytes=self.raw), dict(installer_sha256=self.digest),
                 dict(installer_bytes=b'', installer_sha256=self.digest),
                 dict(installer_bytes=bytearray(self.raw), installer_sha256=self.digest),
                 dict(installer_bytes=self.raw, installer_sha256='0' * 64),
                 dict(installer_bytes=self.raw, installer_sha256='A' * 64),
                 dict(installer_bytes=b'x' * (state._PRIVATE_LIMIT + 1), installer_sha256=self.digest)]
        for options in cases:
            with self.subTest(keys=tuple(options)), patch.object(state, '_snapshot_unreadable') as snapshot, \
                    patch.object(state.tempfile, 'mkdtemp') as allocate:
                with self.assertRaises(SmokeError):
                    state.create_state(self.pointer, self.ctx, 'b' * 64, 'linux-x64', **options)
                snapshot.assert_not_called()
                allocate.assert_not_called()
        self.assertFalse(self.pointer.exists())
        self.assertFalse(self.pointer.with_name('pointer.json.public').exists())

    def test_copy_digest_is_read_back_before_pointer_is_persisted(self):
        original = state._file_bytes
        def corrupted(path, *args, **kwargs):
            if Path(path).name == 'install.sh':
                return b'corrupted'
            return original(path, *args, **kwargs)
        with patch.object(state, '_file_bytes', side_effect=corrupted):
            with self.assertRaisesRegex(SmokeError, 'install_verify_failed'):
                state.create_state(self.pointer, self.ctx, 'b' * 64, 'linux-x64',
                    installer_bytes=self.raw, installer_sha256=self.digest)
        self.assertFalse(self.pointer.exists())


class FiniteDiagnostics(unittest.TestCase):
    def test_only_allowlisted_reason_and_stage_are_emitted(self):
        for failure, reason in [(SmokeError('unsafe_state_file'), 'unsafe_state_file'),
                                (SmokeError('/private/SECRET'), 'other'),
                                (RuntimeError('SECRET'), 'other')]:
            with self.subTest(reason=reason), patch('sys.stderr', new_callable=io.StringIO) as output:
                state.failure_diagnostic(failure, stage=['SECRET'], category={'SECRET': 1})
                value = json.loads(output.getvalue())
            self.assertEqual(value['reason'], reason)
            self.assertEqual(value['stage'], 'other')
            self.assertEqual(value['category'], 'internal_failure')
            self.assertNotIn('SECRET', output.getvalue())

    def test_cleanup_stage_retained_for_unknown_exception_without_changing_public_schema(self):
        private = {'root': '/not-used'}
        with patch.object(state, '_validate_state'), \
                patch.object(state, '_stop_caffeinate', side_effect=TypeError('SECRET')), \
                patch('sys.stderr', new_callable=io.StringIO) as output:
            result = state.cleanup_state(private)
        self.assertEqual(set(result), {'outcome', 'checks', 'error', 'duration_ns'})
        self.assertEqual(result['outcome'], 'incomplete')
        value = json.loads(output.getvalue())
        self.assertEqual(value['stage'], 'cleanup_caffeinate')
        self.assertEqual(value['reason'], 'other')
        self.assertNotIn('SECRET', output.getvalue())


class InstalledStateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name).resolve()
        self.pointer = self.base / 'pointer.json'
        self.state = state.create_state(self.pointer,
            {'run': '123', 'attempt': '2', 'sha': 'a' * 40}, 'b' * 64, 'linux-x64')
        self.root = Path(self.state['root'])
        self.addCleanup(lambda: shutil.rmtree(self.root, ignore_errors=True))

    def run_child(self, source, **options):
        return state.run_bounded([sys.executable, '-I', '-B', '-c', source],
            self.state, timeout=options.pop('timeout', 3),
            output_limit=options.pop('output_limit', 65536), runtime=False, **options)

    def test_roundtrip_private_immutable_state_and_external_output(self):
        self.assertEqual(state.load_state(self.pointer), self.state)
        self.assertEqual(self.root.stat().st_mode & 0o777, 0o700)
        self.assertEqual(self.pointer.stat().st_mode & 0o777, 0o600)
        self.assertNotIn(self.root, Path(self.state['output']).parents)
        self.assertEqual(hashlib.sha256(Path(self.state['installer']).read_bytes()).hexdigest(),
                         self.state['installer_sha256'])
        with self.assertRaises(SmokeError):
            state.create_state(self.pointer, self.state['ctx'], 'b' * 64, 'linux-x64')
        raw = (self.root / 'state.json').read_bytes()
        (self.root / 'state.json').write_bytes(raw + b' ')
        with self.assertRaises(SmokeError):
            state.load_state(self.pointer)

    def test_symlink_hardlink_and_mode_are_rejected(self):
        output = self.base / 'record'
        state.write_private(output, {'ok': True})
        self.assertEqual(json.loads(state.read_private(output)), {'ok': True})
        with self.assertRaises(FileExistsError):
            state.write_private(output, {})
        link = self.base / 'link'
        link.symlink_to(output)
        with self.assertRaises(OSError):
            state.read_private(link)
        link.unlink()
        os.link(output, link)
        with self.assertRaises(SmokeError):
            state.read_private(output)
        link.unlink()
        output.chmod(0o644)
        with self.assertRaises(SmokeError):
            state.read_private(output)

    def test_environment_scrubs_ambient_credentials(self):
        with patch.dict(os.environ, {'GITHUB_TOKEN': 'private', 'PYTHONPATH': '/evil',
                                      'LD_PRELOAD': '/evil', 'SSH_AUTH_SOCK': '/evil'}):
            env = state.child_environment(self.state, runtime=False)
        self.assertFalse({'GITHUB_TOKEN', 'PYTHONPATH', 'LD_PRELOAD', 'SSH_AUTH_SOCK'} & set(env))
        self.assertEqual(env['HOME'], self.state['home'])

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux proc scan')
    def test_unreadable_same_uid_scan_refuses_cleanup(self):
        with patch.object(state, '_read_proc', side_effect=PermissionError('denied')):
            with self.assertRaises(PermissionError):
                state._linux_scan(self.state)

    def test_binary_io_concurrent_drain_and_real_exit(self):
        payload = bytes(range(256)) * 96
        result = self.run_child('import os,sys; x=sys.stdin.buffer.read(); '
            'os.write(2,b"err\\x00\\xff"); os.write(1,x); sys.exit(7)', stdin=payload)
        self.assertEqual(result['stdout'], payload)
        self.assertEqual(result['stderr'], b'err\x00\xff')
        self.assertEqual(result['returncode'], 7)
        self.assertEqual(result['cleanup'], 'complete')
        self.assertTrue(result['timing']['clock_valid'])
        self.assertIsInstance(result['first_output_ns'], int)
        self.assertGreaterEqual(result['last_output_ns'], result['first_output_ns'])
        self.assertLessEqual(result['last_output_ns'], result['timing']['elapsed_ns'])

    def test_combined_output_limit_and_deadline(self):
        result = self.run_child('import os; os.write(1,b"a"*4096); os.write(2,b"b"*4096)',
                                output_limit=5000)
        self.assertTrue(result['output_limited'])
        self.assertEqual(len(result['stdout']) + len(result['stderr']), 5000)
        self.assertEqual(result['cleanup'], 'complete')

    def test_runtime_timeout_uses_sigint_before_termination(self):
        with patch('marmish_ci_bwrap.environment', return_value={}):
            result = state.run_bounded([sys.executable, '-I', '-B', '-c',
                'import signal,sys,time; '
                'signal.signal(signal.SIGINT,lambda *_:sys.exit(23)); time.sleep(30)'],
                self.state, timeout=.2, output_limit=1024, runtime=True)
        self.assertTrue(result['timed_out'])
        self.assertEqual(result['returncode'], 23)
        self.assertEqual(result['cleanup'], 'complete')
        result = self.run_child('import time; time.sleep(30)', timeout=.1)
        self.assertTrue(result['timed_out'])
        self.assertLess(result['returncode'], 0)
        self.assertEqual(result['cleanup'], 'complete')

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux pidfd/marker cleanup')
    def test_setsid_grandchild_is_authenticated_and_stopped(self):
        fixture = self.root / 'fixture.py'
        fixture.write_text('import os,sys,time,signal,subprocess\n'
            'if len(sys.argv)>1:\n'
            ' signal.signal(signal.SIGINT,lambda *_:sys.exit(0))\n'
            ' time.sleep(60)\n'
            'else:\n'
            ' p=subprocess.Popen([sys.executable,__file__,"child"],start_new_session=True, '
            'stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)\n'
            ' print(p.pid,flush=True)\n')
        fixture.chmod(0o600)
        state.register_executable(self.state, fixture, hashlib.sha256(fixture.read_bytes()).hexdigest())
        result = state.run_bounded([sys.executable, str(fixture)], self.state,
                                  timeout=3, output_limit=1024, runtime=False)
        self.assertEqual(result['cleanup'], 'complete')
        pid = int(result['stdout'])
        # Container PID1 may retain a dead orphan zombie; pidfd proves termination.
        fd = os.pidfd_open(pid) if Path('/proc', str(pid)).exists() else None
        if fd is not None:
            import select
            self.addCleanup(os.close, fd)
            self.assertTrue(select.select([fd], [], [], 1)[0])
        self.assertTrue(list((self.root / 'registry').glob('observed-*.json')))

    def test_suspend_and_regressing_clocks_fail_comparability(self):
        start = {'continuous_ns': 10, 'monotonic_ns': 10, 'wall_ns': 10}
        self.assertFalse(state.elapsed(start,
            {'continuous_ns': 9, 'monotonic_ns': 20, 'wall_ns': 20})['clock_valid'])
        self.assertFalse(state.elapsed(start,
            {'continuous_ns': 10_000_000_010, 'monotonic_ns': 20, 'wall_ns': 20})['clock_valid'])

    def test_cleanup_empty_state_removes_root_and_retains_public(self):
        result = state.cleanup_state(self.state)
        self.assertEqual(result['outcome'], 'complete', result)
        self.assertEqual(result['checks'], dict.fromkeys(state.CHECKS, True))
        self.assertFalse(self.root.exists())
        self.assertFalse(self.pointer.exists())
        self.assertTrue(Path(self.state['output']).is_dir())

    def test_cleanup_refuses_engine_without_authenticated_ready(self):
        (self.root / 'server/engine').mkdir(parents=True)
        result = state.cleanup_state(self.state)
        self.assertEqual(result['outcome'], 'incomplete')
        self.assertIsNone(result['checks']['root_removed'])
        self.assertTrue(self.root.exists())

    def test_cleanup_refuses_changed_output_and_preserves_private_root(self):
        output = Path(self.state['output'])
        output.rename(output.with_name('old-public'))
        output.mkdir(mode=0o700)
        result = state.cleanup_state(self.state)
        self.assertEqual(result['outcome'], 'incomplete')
        self.assertTrue(self.root.exists())

    def test_cleanup_refuses_unowned_partial_install_launchers(self):
        launcher = Path(self.state['prefix']) / 'bin/marmish'
        launcher.parent.mkdir(parents=True)
        launcher.write_text('unowned')
        result = state.cleanup_state(self.state)
        self.assertEqual(result['outcome'], 'incomplete')
        self.assertIsNone(result['checks']['uninstall_complete'])
        self.assertTrue(self.root.exists())

    def test_engine_cleanup_callback_requires_both_positive_proofs(self):
        (self.root / 'client/engine').mkdir(parents=True)
        result = state.cleanup_state(self.state, runtime_cleanup=lambda _: {
            'workers_absent': True, 'engine_cleanup_ready': False})
        self.assertEqual(result['outcome'], 'incomplete')
        result = state.cleanup_state(self.state, runtime_cleanup=lambda _: {
            'workers_absent': True, 'engine_cleanup_ready': True})
        self.assertEqual(result['outcome'], 'complete', result)

    def test_identity_requires_account_and_distinct_profiles(self):
        success = {'returncode': 0, 'timed_out': False, 'output_limited': False,
                   'cleanup': 'complete', 'stdout': json.dumps({'account': 'c'*64}).encode(),
                   'stderr': b''}
        with patch.object(state, 'run_bounded', return_value=success) as command:
            self.assertEqual(state.initialize_identity(self.state, 'server', timeout=1), 'c'*64)
            argv = command.call_args.args[0]
            self.assertEqual(argv[0], str(Path(self.state['prefix']) / 'bin/marmish'))
            self.assertIn('--key-storage', argv)
            self.assertEqual(argv.count('--relay'), 2)
            with self.assertRaises(SmokeError):
                state.initialize_identity(self.state, 'client', timeout=1)
        success['stdout'] = b'{"account":"' + b'd'*64 + b'","extra":true}'
        with patch.object(state, 'run_bounded', return_value=success):
            with self.assertRaises(state.IdentityError):
                state.initialize_identity(self.state, 'client', timeout=1)

    def test_actual_installer_uninstall_removes_verified_fixture(self):
        installer = load_installer()
        archive = self.root / 'fixture.tar.gz'
        archive.write_bytes(archive_bytes(installer))
        prefix = Path(self.state['prefix'])
        installer.install_archive(archive, prefix, prefix / 'bin', VERSION, TARGET)
        result = state.cleanup_state(self.state)
        self.assertEqual(result['outcome'], 'complete', result)
        self.assertTrue(result['checks']['uninstall_complete'])
        self.assertFalse(self.root.exists())

    def test_identity_error_preserves_only_numeric_diagnostic(self):
        result = {'returncode': -6, 'timed_out': False, 'output_limited': False,
                  'cleanup': 'complete', 'stdout': b'secret', 'stderr': b'secret'}
        with patch.object(state, 'run_bounded', return_value=result):
            with self.assertRaises(state.IdentityError) as caught:
                state.initialize_identity(self.state, 'server', timeout=1)
        self.assertEqual(caught.exception.returncode, -6)
        self.assertEqual(caught.exception.signal, 6)
        self.assertNotIn('secret', str(caught.exception))

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux ownership snapshot')
    def test_snapshot_excludes_only_exact_preexisting_unreadable_identity(self):
        identity = state.process_identity(os.getpid())
        before = state.clock_sample()['continuous_ns'] // 1000
        original = state._read_proc
        path = Path('/proc') / str(os.getpid()) / 'environ'
        def denied(target):
            if target == path:
                raise PermissionError('newly nondumpable')
            return original(target)
        with patch.object(state, '_read_proc', side_effect=denied):
            self.assertIn(identity, state._snapshot_unreadable(before))
            with self.assertRaises(SmokeError):
                state._linux_scan(self.state)
            allowed = dict(self.state, preexisting_unreadable=[identity])
            # Include the already snapshotted services as well.
            allowed['preexisting_unreadable'] += self.state['preexisting_unreadable']
            self.assertEqual(state._linux_scan(allowed), [])
            allowed['preexisting_unreadable'] = [dict(row, start=row['start']-1)
                                                for row in allowed['preexisting_unreadable']]
            with self.assertRaises(SmokeError):
                state._linux_scan(allowed)

    def test_mac_interrupted_before_control_record_is_incomplete(self):
        record = {'identity': {'pid': 1}, 'runtime': True, 'finished': True,
                  'cleanup': 'complete', 'returncode': -9, 'timed_out': True,
                  'output_limited': False, 'control_verified': False}
        with patch.object(state, 'sys', SimpleNamespace(platform='darwin')), \
                patch.object(state, '_registry_records', return_value=[record]):
            with self.assertRaises(SmokeError):
                state._native_cleanup(self.state)

    def test_replaced_process_identity_is_absent_and_never_signaled(self):
        identity = state.process_identity(os.getpid())
        replaced = dict(identity, start=identity['start'] - 1)
        self.assertTrue(state._process_absent(replaced))
        child = SimpleNamespace(pid=os.getpid(), returncode=None)
        with patch.object(state.os, 'killpg') as kill:
            with self.assertRaises(SmokeError):
                state._signal_group(child, replaced, state.signal.SIGTERM)
            kill.assert_not_called()

    def test_changed_installer_reports_uninstall_failed_and_retains_root(self):
        installer = load_installer()
        archive = self.root / 'fixture.tar.gz'
        archive.write_bytes(archive_bytes(installer))
        prefix = Path(self.state['prefix'])
        installer.install_archive(archive, prefix, prefix / 'bin', VERSION, TARGET)
        Path(self.state['installer']).write_bytes(b'tampered installer\n')
        result = state.cleanup_state(self.state)
        self.assertEqual(result['error'], 'uninstall_failed')
        self.assertTrue(self.root.exists())

    def test_caffeinate_is_owned_tracked_and_stopped(self):
        child = SimpleNamespace(pid=43210)
        identity = {'pid': 43210, 'boot_id': 'b', 'start': 10}
        with patch.object(state, 'sys', SimpleNamespace(platform='darwin')), \
                patch.object(state, 'child_environment', return_value={}), \
                patch.object(state.subprocess, 'Popen', return_value=child) as spawn, \
                patch.object(state, 'process_identity', return_value=identity):
            state.start_caffeinate(self.state)
            self.assertEqual(spawn.call_args.args[0], ['/usr/bin/caffeinate', '-dimsu'])
            record = json.loads(state.read_private(self.root / 'registry/caffeinate.json'))
            self.assertEqual(record['identity'], identity)
            with patch.object(state, '_process_absent', side_effect=[False, True]), \
                    patch.object(state.os, 'kill') as kill:
                state._stop_caffeinate(self.state)
                kill.assert_called_once_with(43210, state.signal.SIGTERM)

    def transport_fixture(self):
        script = self.root / 'marmish_transport_sandbox.py'
        script.write_bytes((Path(state.__file__).parent / script.name).read_bytes())
        script.chmod(0o600)
        wnd = self.root / 'wnd'
        shutil.copyfile('/bin/true', wnd)
        wnd.chmod(0o700)
        for path in (script, wnd):
            state.register_executable(self.state, path, hashlib.sha256(path.read_bytes()).hexdigest())
        directory = Path(self.state['tmp']) / 'marmish-confinement-fixture'
        directory.mkdir(mode=0o700)
        config = {'home': str(self.root / 'server/mdk'), 'wnd': str(wnd),
                  'origins': [['127.0.0.2', 443, '0.sock'], ['127.0.0.3', 443, '1.sock']],
                  'relays': list(state.RELAYS), 'directory': str(directory),
                  'execution_marker': {'EPIPHYTIC_EXECUTION_ID': self.state['execution_id']},
                  'bootstrap': False}
        config_path = directory / 'config.json'
        config_path.write_text(json.dumps(config))
        return script, wnd, config_path

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux bound transport')
    def test_actual_outer_bwrap_bind_argv_authenticates(self):
        import marmish_transport_sandbox as sandbox
        script, _wnd, config = self.transport_fixture()
        captured = []
        def execv(_binary, argv):
            captured.extend(os.fsencode(arg) for arg in argv)
            raise RuntimeError('captured exec')
        with patch.object(sandbox, '__file__', str(script)), \
                patch.object(sandbox.subprocess, 'run'), patch.object(sandbox.os, 'execv', side_effect=execv):
            with self.assertRaisesRegex(RuntimeError, 'captured exec'):
                sandbox._outer(str(config), '/usr/bin/bwrap')
        artifacts = state._registry_records(self.state, 'artifact-')
        with patch.object(sandbox, 'validated_bwrap', return_value=Path('/usr/bin/bwrap')):
            self.assertTrue(state._verify_transport_process(self.state,
                state.process_identity(os.getpid()), Path('/usr/bin/bwrap'), captured, artifacts))
            captured[captured.index(b'/config.json')-1] += b'-wrong'
            self.assertFalse(state._verify_transport_process(self.state, state.process_identity(os.getpid()),
                    Path('/usr/bin/bwrap'), captured, artifacts))

    @unittest.skipUnless(sys.platform.startswith('linux'), 'Linux bound transport')
    def test_inner_bind_inode_checks_and_real_proc_file(self):
        import marmish_transport_sandbox as sandbox
        script, _wnd, config = self.transport_fixture()
        artifacts = state._registry_records(self.state, 'artifact-')
        identity = state.process_identity(os.getpid())
        with patch.object(sandbox, 'validated_bwrap', return_value=Path('/usr/bin/bwrap')), \
                patch.object(state, '_same_bound_file') as bind, \
                patch.object(state, '_bound_matches', return_value=True):
            self.assertTrue(state._verify_transport_process(self.state, identity,
                Path('/usr/bin/python3').resolve(),
                [b'/usr/bin/python3', b'/sandbox.py', b'--inner', b'/config.json'], artifacts))
            self.assertEqual(bind.call_args_list[0].args, (identity, 'config.json', config))
            self.assertEqual(bind.call_args_list[1].args, (identity, 'sandbox.py', script))
        python = Path('/proc/self/exe').resolve()
        state._same_bound_file(identity, str(python).lstrip('/'), python)
        with self.assertRaises(SmokeError):
            state._same_bound_file(identity, str(python).lstrip('/'), script)


if __name__ == '__main__':
    unittest.main()
