"""One-shot native credential handoff contracts without vault or relay access."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import marmish_bootstrap as bootstrap
import marmish_transport as transport
from marmish_transport_sandbox import SandboxError


class BootstrapTests(unittest.TestCase):
    def frame(self):
        return bytearray(bootstrap.MAGIC + bytes(range(64)))

    def test_prefilled_reader_exact_payload_eof_and_wiped_owner(self):
        frame = self.frame()
        expected = bytes(frame)
        reader = bootstrap.open_pipe(frame)
        try:
            self.assertFalse(os.get_inheritable(reader))
            self.assertEqual(os.read(reader, 100), expected)
            self.assertEqual(os.read(reader, 1), b'')
            self.assertEqual(frame, bytearray(72))
        finally:
            os.close(reader)
        self.assertEqual(bootstrap.open_pipe(None), subprocess.DEVNULL)

    def test_invalid_and_failed_pipe_wipe(self):
        for frame in (bytearray(71), bytearray(73), bytearray(b'wrongtag' + bytes(64))):
            with self.assertRaises(SandboxError):
                bootstrap.open_pipe(frame)
            self.assertFalse(any(frame))
        frame = self.frame()
        with patch.object(bootstrap.os, 'write', side_effect=OSError('test')), self.assertRaises(OSError):
            bootstrap.open_pipe(frame)
        self.assertFalse(any(frame))

    def test_constructor_failure_wipes_transferred_frame(self):
        frame = self.frame()
        with patch.object(transport, '_create_daemon', side_effect=SandboxError('test')), self.assertRaises(SandboxError):
            transport.ConfinedDaemon(None, None, None, None, bootstrap=frame)
        self.assertFalse(any(frame))

    @unittest.skipUnless(__import__('sys').platform.startswith('linux'), 'Linux lock surface')
    def test_migration_lock_excludes_transport_inode(self):
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            home.chmod(0o700)
            with transport.acquire_profile_home_lock(home):
                with self.assertRaisesRegex(SandboxError, 'daemon_home_in_use'):
                    with transport.acquire_profile_home_lock(home):
                        self.fail('contended lock acquired')
            with transport.acquire_profile_home_lock(home):
                pass


class LinuxHandoffTests(unittest.TestCase):
    def test_start_failure_wipes_owned_frame(self):
        import marmish_transport_sandbox as sandbox
        frame = bytearray(bootstrap.MAGIC + bytes(range(64)))
        with patch('marmish_mdk_build.verify_manifest', return_value={}):
            daemon = sandbox.ConfinedDaemon(Path('/unused-home'), Path('/unused-wnd'),
                                            ['wss://selected.example/'], '0' * 64, bootstrap=frame)
        with patch.object(daemon, '_prepare', side_effect=SandboxError('test_prepare_failure')):
            with self.assertRaisesRegex(SandboxError, 'test_prepare_failure'):
                daemon.start()
        self.assertFalse(any(frame))
        self.assertIsNone(daemon._bootstrap)

    def test_direct_native_constructor_requires_manifest_before_execution(self):
        import marmish_transport_sandbox as sandbox
        with tempfile.TemporaryDirectory() as temporary:
            wnd = Path(temporary) / 'wnd'
            wnd.write_bytes(b'unpatched daemon')
            frame = bytearray(bootstrap.MAGIC + bytes(range(64)))
            with patch.object(sandbox.subprocess, 'Popen') as execute:
                with self.assertRaisesRegex(SandboxError, 'invalid_manifest'):
                    sandbox.ConfinedDaemon(Path(temporary) / 'home', wnd, ['wss://selected.example/'],
                                            '0' * 64, bootstrap=frame)
                execute.assert_not_called()
            self.assertFalse(any(frame))

    def test_outer_helpers_cannot_consume_bootstrap(self):
        import json
        import marmish_transport_sandbox as sandbox
        with tempfile.TemporaryDirectory() as temporary:
            config = Path(temporary) / 'config.json'
            config.write_text(json.dumps({'directory': temporary, 'home': '/state', 'wnd': '/wnd',
                                          'execution_marker': {}, 'bootstrap': True}))
            with patch.object(sandbox.subprocess, 'run') as run, patch.object(sandbox.os, 'execv') as execute:
                sandbox._outer(str(config), '/reviewed/bwrap')
            self.assertEqual(run.call_count, 2)
            for call in run.call_args_list:
                self.assertEqual(call.kwargs['stdin'], subprocess.DEVNULL)
            self.assertEqual(execute.call_args.args[0], '/reviewed/bwrap')
            self.assertNotIn('--preserve-fds', execute.call_args.args[1])
            self.assertNotIn('--args', execute.call_args.args[1])

    def test_self_test_helpers_cannot_consume_bootstrap(self):
        import marmish_transport_sandbox as sandbox
        from unittest.mock import Mock
        caps = '\n'.join(name + ':\t0000000000000000' for name in
                         ('CapEff', 'CapPrm', 'CapInh', 'CapAmb', 'CapBnd'))
        with patch.object(sandbox.Path, 'read_text', return_value=caps), \
                patch.object(sandbox.Path, 'read_bytes', return_value=b''), \
                patch.object(sandbox.socket, 'socket'), \
                patch.object(sandbox.subprocess, 'run', return_value=Mock(returncode=1)) as run, \
                patch.object(sandbox.subprocess, 'check_output', return_value=b'[]') as output:
            sandbox._self_test()
        self.assertEqual(run.call_args.kwargs['stdin'], subprocess.DEVNULL)
        self.assertEqual(output.call_count, 2)
        self.assertTrue(all(call.kwargs['stdin'] == subprocess.DEVNULL for call in output.call_args_list))

    @unittest.skipUnless(__import__('sys').platform.startswith('linux'), 'Linux namespace boundary')
    def test_actual_unshare_bwrap_inner_chain_and_public_configuration(self):
        import hashlib
        import json
        import time
        import marmish_transport_sandbox as sandbox
        from unittest.mock import Mock
        with tempfile.TemporaryDirectory(prefix='marmish-frame-ns-') as temporary:
            root = Path(temporary)
            wnd = root / 'wnd'
            wnd.write_text('''#!/usr/bin/python3
import json,os,pathlib,socket,sys,time
frame=sys.stdin.buffer.read(73)
assert frame == b'MRMNST01'+bytes(range(64))
assert sys.argv[sys.argv.index('--secret-store')+1]=='marmish-stdin'
null=os.open('/dev/null',os.O_RDONLY);os.dup2(null,0);os.close(null)
assert os.read(0,1)==b''
pathlib.Path('/state/report').write_text(json.dumps({'size':len(frame),'eof':True}))
s=socket.socket(socket.AF_UNIX);s.bind('/state/s');s.listen()
while True:time.sleep(1)
''')
            wnd.chmod(0o700)
            frame = bytearray(bootstrap.MAGIC + bytes(range(64)))
            secret = bytes(frame)
            with patch('marmish_mdk_build.verify_manifest', return_value={}):
                daemon = sandbox.ConfinedDaemon(root / 'home', wnd, ['wss://selected.example/'],
                                                hashlib.sha256(wnd.read_bytes()).hexdigest(), bootstrap=frame)
            real_popen = sandbox.subprocess.Popen
            launches = []
            def observe(command, **kwargs):
                launches.append((command, kwargs.get('env', {})))
                return real_popen(command, **kwargs)
            try:
                with patch.object(sandbox.subprocess, 'Popen', side_effect=observe):
                    daemon.start()
                self.assertEqual(json.loads((daemon.home / 'report').read_text()), {'size': 72, 'eof': True})
                config = json.loads((Path(daemon._temporary.name) / 'config.json').read_text())
                self.assertIs(config['bootstrap'], True)
                for public in [json.dumps(config), repr(launches)]:
                    self.assertNotIn(secret.hex(), public)
                    self.assertNotIn(repr(secret), public)
                self.assertEqual(launches[0][0][0], 'unshare')
                self.assertFalse(any(frame))
            finally:
                daemon.close()
