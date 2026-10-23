"""Offline confinement contracts; all remote fixtures use host loopback only."""
import hashlib
import json
import os
from pathlib import Path
import socket
import stat
import tempfile
import threading
import time
from marmish_platform import continuous_time
import unittest
from unittest.mock import patch, Mock

import marmish_transport_sandbox as sandbox


class ValidationTests(unittest.TestCase):
    def test_buffered_stream_finishes_request_before_forwarding_half_close(self):
        client, left = socket.socketpair()
        right, server = socket.socketpair()
        stop = threading.Event()
        request = b'request-block' * 32768
        response = b'complete-response' * 16384
        observed = []
        def remote():
            data = bytearray()
            while block := server.recv(4096):
                data.extend(block)
            observed.append(bytes(data))
            server.sendall(response)
            server.shutdown(socket.SHUT_WR)
        pump = threading.Thread(target=sandbox._pump, args=(left, right, stop), daemon=True)
        peer = threading.Thread(target=remote, daemon=True)
        for stream in (client, server):
            stream.settimeout(3)
        try:
            pump.start()
            peer.start()
            client.sendall(request)
            client.shutdown(socket.SHUT_WR)
            received = bytearray()
            while block := client.recv(4096):
                received.extend(block)
            self.assertEqual(bytes(received), response)
            self.assertEqual(observed, [request])
            pump.join(1)
            peer.join(1)
            self.assertFalse(pump.is_alive())
            self.assertFalse(peer.is_alive())
        finally:
            stop.set()
            for stream in (client, left, right, server):
                stream.close()
            pump.join(1)
            peer.join(1)

    def test_fixed_origin_connect_uses_numeric_resolution_and_original_port(self):
        broker = sandbox._Broker([])
        with socket.socket() as fixture:
            fixture.bind(('127.0.0.1', 0))
            fixture.listen(1)
            target = ('localhost', fixture.getsockname()[1])
            try:
                with broker._connect(target) as stream:
                    with fixture.accept()[0] as accepted:
                        accepted.sendall(b'fixed-origin')
                        self.assertEqual(stream.recv(32), b'fixed-origin')
            finally:
                broker.close()

    def test_resolution_is_bounded_and_numeric_results_are_validated(self):
        broker = sandbox._Broker([])
        rows = broker._resolve(('localhost', 443), continuous_time() + 3)
        self.assertTrue(rows)
        self.assertTrue(all(address in ('127.0.0.1', '::1') for _, address in rows))
        import subprocess
        process = Mock(returncode=-9)
        process.communicate.side_effect = [subprocess.TimeoutExpired('resolver', .01), (b'', None)]
        with patch.object(sandbox.subprocess, 'Popen', return_value=process):
            with self.assertRaisesRegex(OSError, 'resolver_timeout'):
                broker._resolve(('approved.invalid', 443), continuous_time() + .01)
        process.kill.assert_called_once()
        self.assertEqual(broker.resolvers, set())
        broker.close()

    def test_explicit_dns_root_origins_only(self):
        self.assertEqual(sandbox.canonical_relays(['wss://RELAY.example:443/']),
                         ['wss://relay.example/'])
        for values in ([], ['ws://relay.example/'], ['wss://127.0.0.1/'],
                       ['wss://relay.example/path'], ['wss://relay.example/?x'],
                       ['wss://user@relay.example/'], ['wss://é.example/']):
            with self.subTest(values=values), self.assertRaises(sandbox.SandboxError):
                sandbox.canonical_relays(values)

    def test_hash_and_home_permissions_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / 'wnd'
            binary.write_bytes(b'not executable')
            daemon = sandbox.ConfinedDaemon(root / 'home', binary, ['wss://relay.example/'], '0' * 64)
            with self.assertRaisesRegex(sandbox.SandboxError, 'daemon_hash_mismatch'):
                daemon.start()
            self.assertIsNone(daemon.process)
            os.chmod(daemon.home, 0o755)
            with self.assertRaisesRegex(sandbox.SandboxError, 'unsafe_private_path'):
                daemon.start()
            daemon.close()

    def test_home_symlink_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'real').mkdir(mode=0o700)
            (root / 'alias').symlink_to(root / 'real')
            daemon = sandbox.ConfinedDaemon(root / 'alias', Path('/bin/true'),
                                            ['wss://relay.example/'], '0' * 64)
            with self.assertRaisesRegex(sandbox.SandboxError, 'unsafe_private_path'):
                daemon.start()


class BwrapSelectionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='marmish-bwrap-')
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.binary = self.root / 'bwrap'
        self.binary.write_bytes(b'owned offline executable fixture')
        self.binary.chmod(0o700)
        self.environment = {'MARMISH_BWRAP': str(self.binary),
                            'MARMISH_BWRAP_SHA256': hashlib.sha256(self.binary.read_bytes()).hexdigest()}

    def selected(self):
        with patch.dict(os.environ, self.environment, clear=True):
            return sandbox.validated_bwrap()

    def test_explicit_selection_is_pinned_and_never_uses_path(self):
        with patch.object(sandbox.shutil, 'which', side_effect=AssertionError('PATH lookup')):
            self.assertEqual(self.selected(), self.binary)
        for change in ({'MARMISH_BWRAP_SHA256': '0' * 64},
                       {'MARMISH_BWRAP_SHA256': ''}, {'MARMISH_BWRAP': 'relative/bwrap'},
                       {'MARMISH_BWRAP': str(self.root / '..' / self.root.name / 'bwrap')},
                       {'MARMISH_BWRAP': ''}):
            with self.subTest(change=change):
                original = self.environment.copy()
                self.environment.update(change)
                with self.assertRaises(sandbox.SandboxError):
                    self.selected()
                self.environment = original
        for key in self.environment:
            with patch.dict(os.environ, {key: self.environment[key]}, clear=True):
                with self.assertRaisesRegex(sandbox.SandboxError, 'invalid_bwrap_selection'):
                    sandbox.validated_bwrap()

    def test_fixed_system_default_ignores_ambient_path(self):
        with patch.dict(os.environ, {'PATH': str(self.root)}, clear=True), \
                patch.object(sandbox.os, 'open', side_effect=FileNotFoundError) as opened, \
                patch.object(sandbox.shutil, 'which', side_effect=AssertionError('PATH lookup')):
            with self.assertRaises(sandbox.SandboxError):
                sandbox.validated_bwrap()
            self.assertEqual(opened.call_args.args[0], '/')
        # The selected default's final basename is fixed even under hostile PATH.
        with patch.dict(os.environ, {'PATH': str(self.root)}, clear=True), \
                patch.object(sandbox.os, 'open', side_effect=[10, 11, 12, FileNotFoundError]) as opened, \
                patch.object(sandbox.os, 'fstat', return_value=Mock(st_uid=0, st_mode=stat.S_IFDIR | 0o755)), \
                patch.object(sandbox.os, 'close'):
            with self.assertRaises(sandbox.SandboxError):
                sandbox.validated_bwrap()
            self.assertEqual([call.args[0] for call in opened.call_args_list], ['/', 'usr', 'bin', 'bwrap'])

    def test_file_modes_symlinks_and_untrusted_ancestors_fail_closed(self):
        for mode in (0o600, 0o775, 0o4755, 0o2755):
            with self.subTest(mode=mode):
                self.binary.chmod(mode)
                with self.assertRaises(sandbox.SandboxError):
                    self.selected()
        self.binary.chmod(0o700)
        self.root.chmod(0o777)
        with self.assertRaises(sandbox.SandboxError):
            self.selected()
        self.root.chmod(0o700)
        link = self.root / 'alias'
        link.symlink_to(self.binary)
        self.environment['MARMISH_BWRAP'] = str(link)
        with self.assertRaises(sandbox.SandboxError):
            self.selected()
        link.unlink()
        link.symlink_to(self.root, target_is_directory=True)
        self.environment['MARMISH_BWRAP'] = str(link / 'bwrap')
        with self.assertRaises(sandbox.SandboxError):
            self.selected()
        self.environment['MARMISH_BWRAP'] = str(self.root / 'fifo')
        os.mkfifo(self.environment['MARMISH_BWRAP'], 0o700)
        with self.assertRaises(sandbox.SandboxError):
            self.selected()

    def test_ownership_uses_supervisor_euid_before_mapping(self):
        supervisor_uid = os.geteuid() or 1001
        original = sandbox.os.fstat
        def owned_file(fd):
            metadata = original(fd)
            if stat.S_ISREG(metadata.st_mode):
                return Mock(st_uid=supervisor_uid, st_mode=metadata.st_mode)
            return metadata
        with patch.object(sandbox.os, 'geteuid', return_value=supervisor_uid), \
                patch.object(sandbox.os, 'fstat', side_effect=owned_file), \
                patch.object(sandbox.os, 'access', return_value=True):
            self.assertEqual(self.selected(), self.binary)
        with patch.object(sandbox.os, 'geteuid', return_value=0), \
                patch.object(sandbox.os, 'fstat', side_effect=owned_file):
            with self.assertRaisesRegex(sandbox.SandboxError, 'unsafe_bwrap_(path|executable)'):
                self.selected()
        def wrong_ancestor(fd):
            metadata = original(fd)
            if stat.S_ISDIR(metadata.st_mode):
                return Mock(st_uid=123456, st_mode=stat.S_IFDIR | 0o700)
            return metadata
        with patch.object(sandbox.os, 'fstat', side_effect=wrong_ancestor):
            with self.assertRaisesRegex(sandbox.SandboxError, 'unsafe_bwrap_path'):
                self.selected()

    def test_access_without_effective_ids_requires_matching_real_credentials(self):
        with patch.object(sandbox.os, 'supports_effective_ids', set()), \
                patch.object(sandbox.os, 'access', return_value=True) as access:
            self.assertEqual(self.selected(), self.binary)
            access.assert_called_once_with(self.binary, os.X_OK)
        for credential in ('getuid', 'getgid'):
            with self.subTest(credential=credential), \
                    patch.object(sandbox.os, 'supports_effective_ids', set()), \
                    patch.object(sandbox.os, credential, return_value=getattr(os, credential)() + 1), \
                    patch.object(sandbox.os, 'access', return_value=True) as access:
                with self.assertRaisesRegex(sandbox.SandboxError, 'unsafe_bwrap_executable'):
                    self.selected()
                access.assert_not_called()

    def test_outer_executes_exact_prevalidated_path_without_mapped_uid_recheck(self):
        config = self.root / 'config.json'
        config.write_text(json.dumps({'directory': str(self.root), 'home': '/owned/state',
                                     'wnd': '/owned/wnd', 'execution_marker': {'EPIPHYTIC_EXECUTION_ID': 'test-1'}}))
        with patch.object(sandbox.subprocess, 'run'), patch.object(sandbox.os, 'execv') as execute, \
                patch.object(sandbox.os, 'geteuid', side_effect=AssertionError('mapped uid check')), \
                patch.object(sandbox, 'validated_bwrap', side_effect=AssertionError('revalidation')):
            sandbox._outer(str(config), str(self.binary))
        executable, command = execute.call_args.args
        self.assertEqual(executable, str(self.binary))
        self.assertEqual(command[0], executable)
        self.assertEqual(command.count(executable), 1)
        self.assertIn('--clearenv', command)
        self.assertIn('--cap-drop', command)
        self.assertIn('--die-with-parent', command)
        self.assertEqual(command[command.index('PATH') + 1], '/usr/bin:/usr/sbin:/bin')
        self.assertFalse(any('MARMISH_BWRAP' in argument for argument in command))
        self.assertNotIn(str(self.binary), config.read_text())

    def test_supervisor_passes_path_only_as_outer_argv(self):
        daemon = sandbox.ConfinedDaemon(self.root / 'home', self.binary, ['wss://relay.example/'], '0' * 64)
        daemon._temporary = Mock(name=str(self.root))
        daemon._temporary.name = str(self.root)
        daemon._bwrap = self.binary
        with patch.dict(os.environ, self.environment), patch.object(daemon, '_prepare'), \
                patch.object(daemon, 'close'), \
                patch.object(sandbox.subprocess, 'Popen', side_effect=sandbox.SandboxError('stop')) as launch:
            with self.assertRaisesRegex(sandbox.SandboxError, 'stop'):
                daemon.start()
        self.assertEqual(launch.call_args.args[0][-3:], ['--outer', str(self.root / 'config.json'), str(self.binary)])
        self.assertFalse(any(key.startswith('MARMISH_BWRAP') for key in launch.call_args.kwargs['env']))


class NamespaceTests(unittest.TestCase):
    def test_approved_bridge_decoy_denial_capabilities_and_cleanup(self):
        # This is a required test: unsupported namespaces fail, not silently skip.
        with tempfile.TemporaryDirectory(prefix='marmish-ns-test-') as temp:
            root = Path(temp)
            fixture = socket.socket()
            fixture.bind(('127.0.0.1', 0))
            fixture.listen(8)
            fixture.settimeout(.1)
            decoy = socket.socket()
            decoy.bind(('127.0.0.1', 0))
            decoy.listen(8)
            decoy.settimeout(.1)
            stop = threading.Event()
            counts = {'approved': 0, 'decoy': 0}
            def serve(listener, category):
                while not stop.is_set():
                    try:
                        stream, _ = listener.accept()
                    except socket.timeout:
                        continue
                    except OSError:
                        return
                    counts[category] += 1
                    with stream:
                        if category == 'approved':
                            stream.sendall(b'offline-fixture')
            threads = [threading.Thread(target=serve, args=(fixture, 'approved')),
                       threading.Thread(target=serve, args=(decoy, 'decoy'))]
            for thread in threads:
                thread.start()
            fake = root / 'wnd'
            fake.write_text('''#!/usr/bin/python3
import json,os,pathlib,socket,subprocess,time
caps={line.split(':')[0]:line.split(':')[1].strip() for line in pathlib.Path('/proc/self/status').read_text().splitlines() if line.startswith(('CapEff:','CapPrm:','CapInh:','CapAmb:','CapBnd:'))}
assert set(caps.values())=={'0000000000000000'}
assert subprocess.run(['/usr/sbin/ip','link','set','lo','down'],capture_output=True).returncode != 0
with socket.create_connection(('approved.invalid',443),timeout=3) as stream:
 assert stream.recv(100)==b'offline-fixture'
denied={}
for label,target in [('decoy',('127.0.0.1',DECOY_PORT)),('external',('192.0.2.1',443)),('dns',('unapproved.invalid',443))]:
 try:
  with socket.create_connection(target,timeout=.3):pass
  denied[label]=False
 except OSError:denied[label]=True
assert all(denied.values())
assert not pathlib.Path('/home').exists() and not pathlib.Path('/run').exists()
assert not any('proxy' in k.lower() for k in os.environ)
pathlib.Path('/state/result').write_text(json.dumps({'caps':caps,'denied':denied}))
# A detached descendant proves PID namespace teardown, not just direct-child kill.
child=subprocess.Popen(['/bin/sh','-c','while :; do echo tick >> /state/ticks; sleep .1; done'],start_new_session=True)
s=socket.socket(socket.AF_UNIX);s.bind('/state/s');s.listen()
while True:time.sleep(1)
'''.replace('DECOY_PORT', str(decoy.getsockname()[1])))
            fake.chmod(0o700)
            digest = hashlib.sha256(fake.read_bytes()).hexdigest()
            daemon = sandbox.ConfinedDaemon(root / 'home', fake, ['wss://approved.invalid/'], digest)
            destinations = []
            def fixed_resolve(target, deadline):
                destinations.append(target)
                self.assertEqual(target, ('approved.invalid', 443))
                return [(socket.AF_INET, '127.0.0.1')]
            # Keep the externally configured relay port443 in every broker
            # request; map only the synthetic TCP fixture at the final dial.
            def fixture_connect(broker, target):
                rows = broker._resolve(target, continuous_time() + 5)
                self.assertEqual(rows, [(socket.AF_INET, '127.0.0.1')])
                return socket.create_connection(fixture.getsockname(), timeout=1)
            host_threshold = Path('/proc/sys/net/ipv4/ip_unprivileged_port_start').read_text()
            try:
                with patch.object(sandbox._Broker, '_resolve', side_effect=fixed_resolve), \
                     patch.object(sandbox._Broker, '_connect', fixture_connect):
                    with daemon:
                        self.assertEqual(daemon.socket, root / 'home' / 's')
                        self.assertIs(daemon.start(), daemon)
                        result = json.loads((daemon.home / 'result').read_text())
                        self.assertTrue(all(result['denied'].values()))
                        self.assertTrue(stat.S_ISSOCK(daemon.socket.lstat().st_mode))
                        rival = sandbox.ConfinedDaemon(daemon.home, fake, daemon.relays, digest)
                        with self.assertRaisesRegex(sandbox.SandboxError, 'daemon_home_in_use'):
                            rival.start()
                        time.sleep(.15)
                    daemon.close()
                ticks = (daemon.home / 'ticks').read_bytes()
                time.sleep(.25)
                self.assertEqual(ticks, (daemon.home / 'ticks').read_bytes())
                self.assertFalse(daemon.socket.exists())
                self.assertTrue((daemon.home / 'result').exists())
                self.assertEqual(counts, {'approved': 1, 'decoy': 0})
                self.assertEqual(destinations, [('approved.invalid', 443)])
                self.assertEqual(host_threshold, Path('/proc/sys/net/ipv4/ip_unprivileged_port_start').read_text())
            finally:
                daemon.close()
                stop.set()
                fixture.close()
                decoy.close()
                for thread in threads:
                    thread.join(1)

    def test_startup_failure_releases_lock_and_private_resources(self):
        with tempfile.TemporaryDirectory(prefix='marmish-ns-fail-') as temp:
            root = Path(temp)
            binary = root / 'wnd'
            binary.write_text('#!/bin/sh\nexit 1\n')
            binary.chmod(0o700)
            daemon = sandbox.ConfinedDaemon(root / 'home', binary, ['wss://approved.invalid/'],
                                            hashlib.sha256(binary.read_bytes()).hexdigest())
            started = continuous_time()
            with self.assertRaisesRegex(sandbox.SandboxError, 'namespace_start_failed'):
                daemon.start()
            self.assertLess(continuous_time() - started, 6)
            self.assertIsNone(daemon.process)
            self.assertIsNone(daemon._temporary)
            self.assertIsNone(daemon._lock)
            # A second launch must fail from the executable, not the old lock.
            with self.assertRaisesRegex(sandbox.SandboxError, 'namespace_start_failed'):
                daemon.start()


if __name__ == '__main__':
    unittest.main()
