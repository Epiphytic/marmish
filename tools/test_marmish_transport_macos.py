import contextlib
import io
import hashlib
import json
import os
from pathlib import Path
import platform
import socket
import subprocess
import sys
import tempfile
import stat
from types import SimpleNamespace
import threading
import unittest
from unittest.mock import patch

import check_mdk_builders
import build_mdk
import marmish_mdk_build as build
import marmish_transport_macos as native
from marmish_transport_sandbox import SandboxError


class SocksTests(unittest.TestCase):
    def negotiate(self, request, allowed=None):
        left, right = socket.socketpair()
        self.addCleanup(left.close)
        self.addCleanup(right.close)
        left.sendall(b'\x05\x01\x00' + request)
        result = native.socks_target(right, allowed or {('selected.example', 443)},
                                     native.continuous_time() + 1)
        self.assertEqual(left.recv(2), b'\x05\x00')
        return result

    def test_shared_relay_canonicalization_fixtures(self):
        for row in json.loads(build.FIXTURE.read_bytes()):
            with self.subTest(input=row['input']):
                if row['canonical'] is None:
                    with self.assertRaises(SandboxError):
                        native.canonical_relays([row['input']])
                else:
                    self.assertEqual(native.canonical_relays([row['input']]), [row['canonical']])

    def test_domain_only_exact_destination(self):
        name = b'SELECTED.example'
        self.assertEqual(self.negotiate(b'\x05\x01\x00\x03' + bytes([len(name)]) + name + b'\x01\xbb'),
                         ('selected.example', 443))

    def test_rejects_ip_forms_commands_names_ports_before_connect(self):
        requests = [b'\x05\x01\x00\x01' + b'\x7f\x00\x00\x01\x01\xbb',
                    b'\x05\x01\x00\x04' + b'\0' * 18,
                    b'\x05\x02\x00\x03\x01x\x01\xbb',
                    b'\x05\x01\x00\x03\x09' + b'127.0.0.1' + b'\x01\xbb',
                    b'\x05\x01\x00\x03\x05other\x01\xbb',
                    b'\x05\x01\x00\x03\x10selected.example\x00\x50',
                    b'\x05\x01\x00\x03\x01\xff\x01\xbb']
        for request in requests:
            with self.subTest(request=request), self.assertRaises(OSError):
                self.negotiate(request)

    def test_malformed_socks_never_invokes_resolver(self):
        broker = native.SocksBroker(['wss://selected.example/']).start()
        self.addCleanup(broker.close)
        with patch.object(broker, '_resolve') as resolver:
            with socket.create_connection(broker.address, timeout=1) as stream:
                stream.sendall(b'\x05\x01\x00\x05\x01\x00\x03\x05other\x01\xbb')
                self.assertEqual(stream.recv(2), b'\x05\x00')
                self.assertEqual(stream.recv(1), b'')
            resolver.assert_not_called()

    def test_half_close_preserves_response(self):
        left, app = socket.socketpair()
        upstream, service = socket.socketpair()
        for stream in (left, app, upstream, service):
            self.addCleanup(stream.close)
            stream.settimeout(2)
        stop = threading.Event()
        thread = threading.Thread(target=native._pump, args=(app, upstream, stop))
        thread.start()
        try:
            left.sendall(b'a\x00\xff')
            left.shutdown(socket.SHUT_WR)
            self.assertEqual(service.recv(3), b'a\x00\xff')
            self.assertEqual(service.recv(1), b'')
            service.sendall(b'finished')
            service.shutdown(socket.SHUT_WR)
            self.assertEqual(left.recv(8), b'finished')
            self.assertEqual(left.recv(1), b'')
        finally:
            stop.set()
            thread.join(2)
        self.assertFalse(thread.is_alive())

    def test_expired_continuous_deadline_rejects_before_read(self):
        left, right = socket.socketpair()
        with left, right, patch.object(native, 'continuous_time', return_value=20):
            with self.assertRaises(OSError):
                native._read_exact(left, 1, 19)


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.wnd = self.root / 'wnd'
        self.wnd.write_bytes(b'disposable artifact')
        self.sha = build.digest(self.wnd)
        self.path = Path(str(self.wnd) + '.manifest.json')
        self.value = {'version': 1, 'source': build.SOURCE, 'revision': build.REVISION,
                      'clean_checkout': True, 'platform': 'linux', 'architecture': platform.machine(),
                      'rust': '1.97.1', 'cargo_lock_sha256': build.LOCK_SHA256,
                      'patch_sha256': None, 'deployment_target': None,
                      'restricted_protocol': None, 'helpers': {}, 'sha256': self.sha,
                      'rustc_version': 'rustc 1.97.1 (fixture)', 'target': build._target('linux'),
                      'codesign': None, 'relay_fixture_sha256': None, 'transport_tests': None}

    def write(self):
        self.path.write_text(json.dumps(self.value))
        self.path.chmod(0o600)

    def test_manifest_binds_revision_lock_toolchain_and_artifact(self):
        self.write()
        self.assertEqual(build.verify_manifest(self.wnd, self.sha, expected_platform='linux'), self.value)
        for key, value in [('revision', '0' * 40), ('cargo_lock_sha256', '0' * 64),
                           ('architecture', 'wrong'), ('rustc_version', 'rustc 1.0'),
                           ('version', True), ('surprise', 1)]:
            with self.subTest(key=key):
                original = dict(self.value)
                self.value[key] = value
                self.write()
                with self.assertRaises(SandboxError):
                    build.verify_manifest(self.wnd, self.sha, expected_platform='linux')
                self.value = original
        self.write()
        self.wnd.write_bytes(b'changed')
        with self.assertRaises(SandboxError):
            build.verify_manifest(self.wnd, self.sha, expected_platform='linux')

    def test_transport_test_build_gate_requires_all_expected_tests(self):
        def successful(command, **_kwargs):
            name = command[command.index('--lib') + 1]
            self.assertIn('--exact', command)
            return SimpleNamespace(stdout='running 1 test\ntest ' + name +
                                   ' ... ok\ntest result: ok. 1 passed; 0 failed;')
        with patch.object(build_mdk.subprocess, 'run', side_effect=successful) as run, \
                contextlib.redirect_stdout(io.StringIO()):
            build_mdk.run_transport_tests(self.root, [], {})
            self.assertEqual(run.call_count, 3)
        for output in ('running 0 tests\ntest result: ok. 0 passed; 0 failed;',
                       'running 1 test\ntest unrelated ... ok\ntest result: ok. 1 passed; 0 failed;'):
            with patch.object(build_mdk.subprocess, 'run', return_value=SimpleNamespace(stdout=output)), \
                    contextlib.redirect_stdout(io.StringIO()), \
                    self.assertRaisesRegex(RuntimeError, 'transport_test_not_executed'):
                build_mdk.run_transport_tests(self.root, [], {})

    def test_duplicate_json_and_fifo_rejected_without_blocking(self):
        self.write()
        self.path.write_text(self.path.read_text().replace('"version": 1', '"version": 1,"version": 1'))
        with self.assertRaises(SandboxError):
            build.verify_manifest(self.wnd, self.sha, expected_platform='linux')
        self.path.unlink()
        os.mkfifo(self.path, 0o600)
        with self.assertRaises(SandboxError):
            build.verify_manifest(self.wnd, self.sha, expected_platform='linux')

    def test_guard_rejects_new_production_builder(self):
        source = self.root / 'crates/marmot-app/src'
        source.mkdir(parents=True)
        (source / 'marmish_transport.rs').write_text('fn factory() { nostr_sdk::Client::builder(); }')
        baseline = check_mdk_builders.source_digest(self.root)
        check_mdk_builders.check(self.root, expected=baseline)
        for constructor in ['NostrSdkClient::builder()', 'nostr_sdk::prelude::Client::builder()',
                            'ClientBuilder::default()', 'nostr_sdk::client::Client::new(keys)',
                            'Client::with_opts(keys, opts)', 'type Alias = Client; Alias::builder()',
                            '<Client>::builder()', 'RelayPool::builder()',
                            '#[cfg(test)]\nmod tests {}\nfn production(){ Client::builder(); }']:
            with self.subTest(constructor=constructor):
                (source / 'bypass.rs').write_text(constructor)
                with self.assertRaises(ValueError):
                    check_mdk_builders.check(self.root, expected=baseline)
        (source / 'bypass.rs').unlink()
        (source / 'link.rs').symlink_to(source / 'marmish_transport.rs')
        with self.assertRaises(ValueError):
            check_mdk_builders.check(self.root, expected=baseline)

    def test_close_unstarted_preserves_someone_elses_run_record(self):
        self.root.chmod(0o700)
        control = native.control_dir(self.root)
        control.mkdir(mode=0o700)
        self.addCleanup(lambda: __import__('shutil').rmtree(control))
        record = control / '.marmish-run.json'
        record.write_bytes(b'not ours')
        daemon = native.DarwinConfinedDaemon(self.root, self.wnd, ['wss://selected.example/'], self.sha)
        daemon.close()
        daemon.close()
        self.assertEqual(record.read_bytes(), b'not ours')


class ControlTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.root.chmod(0o700)
        self.home = self.root / 'data'
        self.home.mkdir(mode=0o700)
        self.daemon = native.DarwinConfinedDaemon(self.home, self.root / 'wnd',
                                                 ['wss://selected.example/'], '0' * 64)
        self.daemon._control_fd = native._open_control(self.home)
        self.addCleanup(self.daemon.close)
        self.boot = '12345678-1234-1234-1234-123456789abc'
        self.identity = {'pid': 123, 'boot_id': self.boot, 'start': 456}

    def record(self, **changes):
        identity = dict(self.identity, **changes)
        native._atomic_record(self.daemon._control_fd,
                              {'daemon': identity, 'shim': identity, 'runtime': '.marmish-native-test'})

    def test_old_boot_absent_reused_and_other_user_never_signaled(self):
        for case in ('oldboot', 'absent', 'reused', 'otheruser'):
            with self.subTest(case=case):
                self.record()
                value = dict(self.identity, uid=os.geteuid(), ruid=os.geteuid())
                if case == 'absent':
                    value = None
                elif case == 'reused':
                    value['start'] += 1
                elif case == 'otheruser':
                    value['uid'] += 1
                current = dict(self.identity, boot_id='aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa') if case == 'oldboot' else self.identity
                with patch.object(native, 'process_identity', return_value=current), \
                        patch.object(native, 'inspect_process', return_value=value) as inspect, \
                        patch.object(native.os, 'killpg') as kill:
                    self.daemon._recover()
                    kill.assert_not_called()
                    if case == 'oldboot':
                        inspect.assert_not_called()
                self.assertFalse(self.daemon._record.exists())

    def test_uninspectable_malformed_and_truncated_preserve_record(self):
        self.record()
        with patch.object(native, 'process_identity', return_value=self.identity), \
                patch.object(native, 'inspect_process', side_effect=RuntimeError), \
                patch.object(native.os, 'killpg') as kill:
            with self.assertRaises(SandboxError):
                self.daemon._recover()
            kill.assert_not_called()
        self.assertTrue(self.daemon._record.exists())
        self.record(boot_id='invalid')
        with self.assertRaises(SandboxError):
            self.daemon._recover()
        self.daemon._record.write_bytes(b'{')
        with self.assertRaises(ValueError):
            self.daemon._recover()
        self.assertTrue(self.daemon._record.exists())

    def test_recovery_group_exit_race_requires_verified_absence(self):
        for absent in (True, False):
            with self.subTest(absent=absent):
                self.record()
                observations = [self.identity, None, None] if absent else [self.identity, self.identity]
                with patch.object(native, 'process_identity', return_value=self.identity), \
                        patch.object(native, '_current_identity', side_effect=observations), \
                        patch.object(native.os, 'getpgid', side_effect=ProcessLookupError), \
                        patch.object(native.os, 'killpg') as kill:
                    if absent:
                        self.daemon._recover()
                        self.assertFalse(self.daemon._record.exists())
                    else:
                        with self.assertRaisesRegex(SandboxError, 'orphan_identity_unverified'):
                            self.daemon._recover()
                        self.assertTrue(self.daemon._record.exists())
                    kill.assert_not_called()

    def test_missing_executable_is_typed_and_record_retained(self):
        self.record()
        def path(_pid, buffer, _size):
            buffer.value = b'/missing-owned-native-executable'
            return len(buffer.value)
        library = SimpleNamespace(proc_pidpath=path)
        with patch.object(native, 'process_identity', return_value=self.identity), \
                patch.object(native, '_current_identity', return_value=self.identity), \
                patch.object(native.os, 'getpgid', return_value=self.identity['pid']), \
                patch('ctypes.CDLL', return_value=library), \
                patch.object(native, 'digest', side_effect=FileNotFoundError), \
                patch.object(native.os, 'killpg') as kill:
            with self.assertRaisesRegex(SandboxError, 'orphan_executable_unverified'):
                self.daemon._recover()
            kill.assert_not_called()
        self.assertTrue(self.daemon._record.exists())

    def test_control_modes_types_and_independent_homes(self):
        other = self.root / 'other'
        other.mkdir(mode=0o700)
        control = native.control_dir(other)
        self.assertNotEqual(control, self.daemon._control)
        for kind in ('mode', 'file', 'symlink'):
            with self.subTest(kind=kind):
                if kind == 'mode':
                    control.mkdir(mode=0o750)
                    control.chmod(0o750)
                elif kind == 'file':
                    control.write_bytes(b'x')
                else:
                    control.symlink_to(self.daemon._control, target_is_directory=True)
                with self.assertRaises((SandboxError, OSError)):
                    native._open_control(other)
                if control.is_dir() and not control.is_symlink():
                    control.rmdir()
                else:
                    control.unlink()
        with self.assertRaises(SandboxError):
            native._ancestor(SimpleNamespace(st_uid=os.geteuid(), st_mode=stat.S_IFDIR | 0o0777))
        native._ancestor(SimpleNamespace(st_uid=0, st_mode=stat.S_IFDIR | stat.S_ISVTX | 0o0777))

    def test_daemon_argv_and_socket_staging_budget(self):
        self.daemon._directory = self.daemon._control / '.marmish-native-argv'
        self.daemon._directory.mkdir(mode=0o700)
        self.daemon._binary = self.daemon._directory / 'wnd'
        self.daemon._broker = SimpleNamespace(address=('127.0.0.1', 12345), close=lambda: None)
        command = self.daemon._daemon_command()
        offset = command.index(str(self.daemon._binary))
        self.assertEqual(command[offset:], [str(self.daemon._binary), '--home', str(self.home),
                         '--socket', str(self.home / 's'), '--secret-store', 'file',
                         '--discovery-relays', 'wss://selected.example/',
                         '--default-account-relays', 'wss://selected.example/'])
        self.assertTrue(native.socket_path_fits('/' + 'x' * 78))
        self.assertFalse(native.socket_path_fits('/' + 'x' * 90))

    def test_atomic_record_gc_and_cleanup_retains_lock_inode(self):
        self.record()
        self.assertEqual(native._private_json('.marmish-run.json', dir_fd=self.daemon._control_fd)['daemon'], self.identity)
        self.daemon._record.unlink()
        stale = self.daemon._control / '.marmish-native-stale'
        stale.mkdir(mode=0o700)
        (stale / 'ack').write_bytes(b'ephemeral')
        sentinel = self.daemon._control / '.marmish-probe-record'
        sentinel.write_bytes(b'interrupted destructive probe')
        sentinel.chmod(0o600)
        native.cleanup_control(self.home)
        self.assertFalse(sentinel.exists())
        self.assertFalse(stale.exists())
        lock = self.daemon._control / '.marmish-transport.lock'
        inode = lock.stat().st_ino
        native.cleanup_control(self.home)
        self.assertEqual(lock.stat().st_ino, inode)


@unittest.skipUnless(sys.platform == 'darwin', 'native Darwin process semantics')
class NativeLifetimeTests(unittest.TestCase):
    def build_helpers(self, directory):
        root = Path(directory).resolve()
        for name in ('marmish-lifetime', 'marmish-lifetime-test'):
            source = Path(native.__file__).resolve().parent / 'native' / (name + '.c')
            subprocess.run(['clang', '-Wall', '-Wextra', '-Werror', str(source), '-o', str(root / name)], check=True)
            subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', str(root / name)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        return root / 'marmish-lifetime', root / 'marmish-lifetime-test'

    def await_file(self, path):
        deadline = native.continuous_time() + 5
        while not path.exists() and native.continuous_time() < deadline:
            threading.Event().wait(.02)
        self.assertTrue(path.exists(), 'native child did not publish fixture')
        return int(path.read_text())

    def await_absent(self, pid):
        deadline = native.continuous_time() + 5
        while native.continuous_time() < deadline:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                return
            threading.Event().wait(.02)
        self.fail('native descendant remained after cleanup')

    def test_gate_signal_reset_natural_exit_and_term_ignoring_descendant(self):
        with tempfile.TemporaryDirectory(prefix='marmish-lifetime-test-') as directory:
            helper, stub = self.build_helpers(directory)
            for mode in ('nogrant', 'signal', 'fork', 'wait'):
                with self.subTest(mode=mode):
                    report = helper.parent / (mode + '.report')
                    reader, writer = os.pipe()
                    child = subprocess.Popen([str(helper), str(reader), str(os.getpid()), str(stub),
                                              'signal' if mode == 'nogrant' else mode, str(report)],
                                             pass_fds=(reader,), stdout=subprocess.PIPE,
                                             stderr=subprocess.DEVNULL, start_new_session=True)
                    os.close(reader)
                    try:
                        leader = int(child.stdout.readline())
                        if mode != 'nogrant':
                            os.write(writer, b'G')
                            value = self.await_file(report)
                            if mode == 'signal':
                                self.assertEqual(value, 1, 'SIGPIPE ignore leaked into daemon')
                        else:
                            self.assertFalse(report.exists())
                        os.close(writer)
                        writer = None
                        child.wait(timeout=5)
                        self.await_absent(leader)
                        if mode in ('fork', 'wait'):
                            self.await_absent(value)
                        if mode == 'nogrant':
                            self.assertFalse(report.exists(), 'child executed without durable grant')
                    finally:
                        if writer is not None:
                            os.close(writer)
                        if child.poll() is None:
                            child.kill()
                            child.wait()
                        child.stdout.close()

    def test_parent_sigkill_kqueue_works_with_live_pipe_writer(self):
        with tempfile.TemporaryDirectory(prefix='marmish-lifetime-test-') as directory:
            helper, stub = self.build_helpers(directory)
            report = helper.parent / 'grandchild'
            reader, writer = os.pipe()
            supervisor_code = """import os,subprocess,sys,time
p=subprocess.Popen([sys.argv[1],sys.argv[2],str(os.getpid()),sys.argv[3],'wait',sys.argv[4]],pass_fds=(int(sys.argv[2]),),stdout=subprocess.PIPE,start_new_session=True)
print(p.pid, int(p.stdout.readline()), flush=True)
while True: time.sleep(1)
"""
            supervisor = subprocess.Popen([sys.executable, '-c', supervisor_code, str(helper), str(reader),
                                           str(stub), str(report)], pass_fds=(reader,), stdout=subprocess.PIPE)
            os.close(reader)
            try:
                shim, leader = map(int, supervisor.stdout.readline().split())
                os.write(writer, b'G')
                descendant = self.await_file(report)
                supervisor.kill()
                supervisor.wait(timeout=5)
                # The writer remains open here: parent kqueue, not pipe EOF, owns cleanup.
                self.await_absent(leader)
                self.await_absent(descendant)
            finally:
                os.close(writer)
                if supervisor.poll() is None:
                    supervisor.kill()
                    supervisor.wait()
                supervisor.stdout.close()

    def test_sigkill_shim_recovers_only_verified_owned_process_group(self):
        with tempfile.TemporaryDirectory(prefix='mr-', dir='/private/tmp') as directory:
            helper, stub = self.build_helpers(directory)
            home = helper.parent / 'data'
            home.mkdir(mode=0o700)
            daemon = native.DarwinConfinedDaemon(home, stub, ['wss://selected.example/'], build.digest(stub))
            daemon._control_fd = native._open_control(home)
            runtime = daemon._control / '.marmish-native-recovery'
            runtime.mkdir(mode=0o700)
            report = home / 'descendant'
            reader, writer = os.pipe()
            child = subprocess.Popen([str(helper), str(reader), str(os.getpid()), str(stub),
                                      'wait', str(report)], pass_fds=(reader,), stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL, start_new_session=True)
            os.close(reader)
            leader = None
            try:
                leader = int(child.stdout.readline())
                record = {'daemon': native.process_identity(leader), 'shim': native.process_identity(child.pid),
                          'daemon_sha256': build.digest(stub), 'shim_sha256': build.digest(helper),
                          'runtime': runtime.name}
                native._atomic_record(daemon._control_fd, record)
                os.write(writer, b'G')
                descendant = self.await_file(report)
                child.kill()
                child.wait(timeout=5)
                daemon._recover()
                self.assertFalse(daemon._record.exists())
                self.await_absent(leader)
                self.await_absent(descendant)
            finally:
                os.close(writer)
                if child.poll() is None:
                    child.kill()
                    child.wait()
                if leader is not None:
                    with contextlib.suppress(ProcessLookupError):
                        os.killpg(leader, 9)
                child.stdout.close()
                os.close(daemon._control_fd)
                daemon._control_fd = None
                if daemon._record.exists():
                    daemon._record.unlink()
                native.cleanup_control(home)

    def test_pipe_eof_reaps_daemon_without_parent_finally(self):
        with tempfile.TemporaryDirectory(prefix='marmish-lifetime-test-') as directory:
            helper = Path(directory).resolve() / 'marmish-lifetime'
            source = Path(native.__file__).resolve().parent / 'native/marmish-lifetime.c'
            subprocess.run(['clang', '-Wall', '-Wextra', '-Werror', str(source), '-o', str(helper)], check=True)
            subprocess.run(['/usr/bin/codesign', '--force', '--sign', '-', str(helper)], check=True,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            reader, writer = os.pipe()
            try:
                child = subprocess.Popen([str(helper), str(reader), str(os.getpid()), '/bin/sleep', '30'],
                                         pass_fds=(reader,), stdout=subprocess.PIPE,
                                         stderr=subprocess.DEVNULL, start_new_session=True)
                try:
                    pid = int(child.stdout.readline())
                    identity = native.process_identity(pid)
                    os.write(writer, b'G')
                    os.close(writer)
                    writer = None
                    child.wait(timeout=5)
                    self.assertIsNone(native._current_identity(identity))
                finally:
                    if child.poll() is None:
                        child.kill()
                        child.wait()
                    child.stdout.close()
            finally:
                os.close(reader)
                if writer is not None:
                    os.close(writer)


if __name__ == '__main__':
    unittest.main()
