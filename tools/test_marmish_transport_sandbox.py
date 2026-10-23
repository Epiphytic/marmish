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
