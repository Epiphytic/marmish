"""Security boundary and byte-preservation checks for trusted native CI."""
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import time
import unittest
from unittest.mock import Mock, patch
from types import SimpleNamespace
import zipfile

import marmish_rendezvous as rendezvous
import marmish_interop as interop
from marmish_protocol import PURPOSE, save_json
from mdk_smoke import SmokeError


class RendezvousTests(unittest.TestCase):
    def metadata(self):
        return {'version': 1, 'run': '123', 'attempt': '1', 'sha': 'a' * 40,
                'role': 'linux', 'nonce': '1' * 64, 'server': '2' * 64,
                'client': '3' * 64, 'intruder': '5' * 64, 'relays': rendezvous.RELAYS,
                'fixture': '/tmp/mci-abcdef/f/main', 'fixture_sha256': '4' * 64,
                'platform': 'linux', 'published_at': int(time.time()),
                'expires_at': int(time.time()) + 4200}

    def archive(self, payload, extra=b'', name='record.json'):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            info = zipfile.ZipInfo(name)
            info.extra = extra
            archive.writestr(info, payload)
        return stream.getvalue()

    def test_exact_context_and_platform(self):
        value = self.metadata()
        context = {key: value[key] for key in ('run', 'attempt', 'sha')}
        self.assertEqual(rendezvous.validate(value, context, 'linux', 'ready'), value)
        for key, invalid in [('attempt', '2'), ('platform', 'darwin'), ('version', True),
                             ('fixture', '/tmp/mci-abcdef/../f/main'), ('expires_at', 0),
                             ('relays', ['wss://example.org/'])]:
            with self.subTest(key=key):
                bad = dict(value, **{key: invalid})
                with self.assertRaises(SmokeError):
                    rendezvous.validate(bad, context, 'linux', 'ready')

    def test_archive_digest_and_strict_json(self):
        value = self.metadata()
        packed = self.archive(json.dumps(value))
        digest = 'sha256:' + hashlib.sha256(packed).hexdigest()
        self.assertEqual(rendezvous.read_zip(packed, digest), value)
        with self.assertRaisesRegex(SmokeError, 'artifact_digest_mismatch'):
            rendezvous.read_zip(packed, 'sha256:' + '0' * 64)
        for body in [b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":1e999}',
                     b'\xef\xbb\xbf{}', b'[' * 2000, b'x' * 8193]:
            with self.subTest(body=body[:30]), self.assertRaises(SmokeError):
                rendezvous.read_zip(self.archive(body))

    def test_reject_zip64_extra_and_traversal_without_extracting(self):
        # Even a small file can smuggle a ZIP64 extra with an ordinary version.
        for extra, name in [(b'\x01\x00\x00\x00', 'record.json'),
                            (b'', '../record.json'), (b'', 'sub/record.json')]:
            with self.subTest(name=name, extra=extra), self.assertRaises(SmokeError):
                rendezvous.read_zip(self.archive(b'{}', extra, name))

    def test_redirect_drops_token_and_rejects_other_origins(self):
        packed = self.archive(json.dumps(self.metadata()))
        with patch.object(rendezvous, 'request', side_effect=[
                (302, b'', 'https://unit.blob.core.windows.net/object?signature=private'),
                (200, packed, None)]) as request:
            self.assertEqual(rendezvous.archive({'id': 1}, 'secret'), self.metadata())
            self.assertEqual(request.call_args_list[1].args,
                             ('https://unit.blob.core.windows.net/object?signature=private',))
        for url in ['https://evil.example/x', 'http://unit.blob.core.windows.net/x',
                    'https://user@unit.blob.core.windows.net/x']:
            with patch.object(rendezvous, 'request', return_value=(302, b'', url)):
                with self.assertRaisesRegex(SmokeError, 'invalid_artifact_redirect'):
                    rendezvous.archive({'id': 1}, 'secret')

    def test_shared_pairing_deadline_prevents_second_full_wait(self):
        with patch.dict(os.environ, {'GITHUB_TOKEN': 'test'}), \
             patch.object(rendezvous, 'continuous_time', return_value=20), \
             patch.object(rendezvous, 'request') as request:
            with self.assertRaisesRegex(SmokeError, 'pairing_prerequisite_timeout'):
                rendezvous.wait({'run': '1', 'attempt': '1'}, 'linux', 'paired', 1200, deadline=10)
            request.assert_not_called()

    def test_wall_clock_skew_is_diagnostic_and_identities_are_distinct(self):
        value = self.metadata()
        value.update(published_at=1, expires_at=4201)
        ctx = {key: value[key] for key in ('run', 'attempt', 'sha')}
        self.assertEqual(rendezvous.validate(value, ctx, 'linux', 'ready'), value)
        value['intruder'] = value['server']
        with self.assertRaises(SmokeError):
            rendezvous.validate(value, ctx, 'linux', 'ready')

    def test_transient_download_retries_and_http_cannot_overrun_deadline(self):
        value = self.metadata()
        ctx = {key: value[key] for key in ('run', 'attempt', 'sha')}
        item = {'id': 1, 'name': rendezvous.name(ctx, 'linux', 'ready'),
                'expired': False, 'size_in_bytes': 100}
        response = (200, json.dumps({'artifacts': [item]}).encode(), None)
        with patch.dict(os.environ, {'GITHUB_TOKEN': 'test'}), \
             patch.object(rendezvous, 'continuous_time', return_value=1), \
             patch.object(rendezvous, 'request', return_value=response), \
             patch.object(rendezvous.time, 'sleep'), \
             patch.object(rendezvous, 'archive', side_effect=[
                 SmokeError('artifact_download_transient'), value]) as archive:
            self.assertEqual(rendezvous.wait(ctx, 'linux', 'ready', 10), value)
            self.assertEqual(archive.call_count, 2)
        with patch.dict(os.environ, {'GITHUB_TOKEN': 'test'}), \
             patch.object(rendezvous, 'continuous_time', side_effect=[1, 1, 12]), \
             patch.object(rendezvous, 'request', return_value=response), \
             patch.object(rendezvous, 'archive') as archive:
            with self.assertRaisesRegex(SmokeError, 'pairing_prerequisite_timeout'):
                rendezvous.wait(ctx, 'linux', 'ready', 10)
            archive.assert_not_called()


class CoordinatorTests(unittest.TestCase):
    def config(self):
        local = RendezvousTests().metadata()
        peer = dict(local, role='macos', platform='darwin', nonce='6' * 64,
                    server='7' * 64, client='8' * 64, intruder='9' * 64)
        return {'root': '/tmp/mci-abcdef', 'role': 'linux', 'ready': local, 'peer': peer}

    def test_directional_markers_bind_both_nonces(self):
        config = self.config()
        reversed_config = dict(config, role='macos', ready=config['peer'], peer=config['ready'])
        self.assertEqual(interop.marker(config, 'linux'), interop.marker(reversed_config, 'linux'))
        self.assertNotEqual(interop.marker(config, 'linux'), interop.marker(config, 'macos'))
        config['peer'] = dict(config['peer'], nonce='0' * 64)
        self.assertNotEqual(interop.marker(config, 'linux'), interop.marker(reversed_config, 'linux'))

    def test_intruder_requires_authenticated_pending_dedicated_exact_room(self):
        config = self.config()
        server, intruder = config['ready']['server'], config['peer']['intruder']
        invite = {'welcomer_account_id': intruder, 'group_id': 'group',
                  'profile': {'name': 'marmish/v1/' + 'a' * 32, 'description': PURPOSE},
                  'pending_confirmation': True}
        rows = [{'member_id': key, 'npub': key, 'local': key == server}
                for key in (server, intruder)]
        rpc = Mock()
        rpc.invites.return_value = [invite]
        rpc.members.return_value = rows
        self.assertTrue(interop.observe_intruder(rpc, config))
        rpc.invites.assert_called_with(server)
        rpc.members.assert_called_with(server, 'group')
        rpc.invites.return_value = [dict(invite, welcomer_account_id='0' * 64)]
        self.assertFalse(interop.observe_intruder(rpc, config))
        for bad in [dict(invite, pending_confirmation=False),
                    dict(invite, profile={'name': 'general dm', 'description': PURPOSE})]:
            rpc.invites.return_value = [bad]
            with self.assertRaises(SmokeError):
                interop.observe_intruder(rpc, config)
        rpc.invites.return_value = [invite]
        rpc.members.return_value = rows + [rows[0]]
        with self.assertRaisesRegex(SmokeError, 'membership_mismatch'):
            interop.observe_intruder(rpc, config)

    def test_completion_tolerates_partial_write_but_rejects_other_content(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'marker'
            for data, expected in [(b'', False), (b'ab', False), (b'abc', True)]:
                marker.write_bytes(data)
                self.assertIs(interop.completion_matches(marker, b'abc'), expected)
            for data in [b'abx', b'abcd', b'x' * 1000]:
                marker.write_bytes(data)
                with self.assertRaises(SmokeError):
                    interop.completion_matches(marker, b'abc')

    def test_cleanup_retains_unverified_engine_and_removes_only_safe_initial_state(self):
        with tempfile.TemporaryDirectory() as directory:
            base = Path(directory)
            root, pointer = base / 'owned', base / 'pointer'
            root.mkdir(mode=0o700)
            save_json(root / 'config.json', {'root': str(root)})
            pointer.write_text(str(root))
            (root / 'server/engine').mkdir(parents=True)
            args = SimpleNamespace(state=pointer)
            with self.assertRaisesRegex(SmokeError, 'cleanup_unverified_state_retained'):
                interop.cleanup(args)
            self.assertTrue(root.exists())
            (root / 'server/engine').rmdir()
            interop.cleanup(args)
            self.assertFalse(root.exists())
            self.assertFalse(pointer.exists())

    def test_native_cleanup_checks_each_existing_control_and_propagates_failure(self):
        from marmish_transport_sandbox import SandboxError
        with patch.object(interop.sys, 'platform', 'darwin'), \
             patch('marmish_transport_macos.control_dir', side_effect=lambda home: home.parent / 'control'), \
             patch.object(interop.os.path, 'lexists', side_effect=[True, False, True]), \
             patch('marmish_transport_macos.cleanup_control') as cleanup:
            interop.cleanup_controls(Path('/tmp/private'))
            self.assertEqual([call.args[0] for call in cleanup.call_args_list],
                             [Path('/tmp/private/server/mdk'), Path('/tmp/private/intruder/mdk')])
        with patch.object(interop.sys, 'platform', 'darwin'), \
             patch('marmish_transport_macos.control_dir', return_value=Path('/tmp/control')), \
             patch.object(interop.os.path, 'lexists', return_value=True), \
             patch('marmish_transport_macos.cleanup_control', side_effect=SandboxError('busy')):
            with self.assertRaises(SandboxError):
                interop.cleanup_controls(Path('/tmp/private'))

    def test_additive_policy_contains_narrow_allow_and_both_denies(self):
        config = self.config()
        policy = interop.policy(config)
        base = config['root'] + '/f'
        self.assertIn(base + '/*', policy['defaults']['allow_exec'])
        self.assertNotIn(base + '/narrowed', policy['keys'][0]['restrictions']['allow_exec'])
        self.assertEqual(policy['defaults']['deny_exec'], [base + '/deny-default'])
        self.assertEqual(policy['keys'][1]['restrictions']['deny_exec'], [base + '/deny-key'])
        self.assertTrue(all(key['principal'] == config['peer']['client'] for key in policy['keys']))

    def test_workflow_has_fixed_trust_boundary_and_token_only_in_polling_steps(self):
        workflow = (Path(__file__).parents[1] / '.github/workflows/native-validation.yml').read_text()
        self.assertIn('branches: [master]', workflow)
        self.assertNotIn('workflow_dispatch', workflow)
        self.assertNotIn('pull_request', workflow)
        self.assertEqual(workflow.count("if: github.ref == 'refs/heads/master'"), 2)
        self.assertEqual(workflow.count('GITHUB_TOKEN:'), 2)
        self.assertIn('persist-credentials: false', workflow)
        self.assertIn('max-parallel: 1', workflow)
        self.assertIn('RUNNER_TEMP/live-clock.json', workflow)
        for source in ('patches/mdk/source-guard.json', 'tests/fixtures/relay-canonicalization.json',
                       'tools/check_mdk_builders.py'):
            self.assertIn(source, workflow)
        for line in workflow.splitlines():
            if 'uses: actions/' in line:
                self.assertRegex(line.strip(), r'^-?\s*uses: actions/[a-z-]+@[a-f0-9]{40}$')


@unittest.skipUnless(shutil.which('rustc'), 'native Rust compiler required')
class NativeFixtureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.fixture = Path(cls.directory.name) / 'fixture'
        subprocess.run(['rustc', '--edition=2024', str(Path(__file__).with_name('marmish_fixture.rs')),
                        '-o', str(cls.fixture)], check=True, capture_output=True)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def test_native_special_argv_and_binary_streams(self):
        arguments = [b'', b'\xff', b'line\nline', b'$(touch forbidden)', b'"single\'']
        result = subprocess.run([os.fsencode(self.fixture), b'argv', *arguments], capture_output=True)
        self.assertEqual(result.returncode, 7)
        self.assertEqual(result.stdout, b''.join(struct.pack('!I', len(x)) + x for x in arguments))
        self.assertEqual(result.stderr, b'stderr\0\xff')
        data = bytes(range(256)) * 2
        result = subprocess.run([self.fixture, 'binary'], input=data, capture_output=True)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, data, data[::-1]))

    def test_exclusive_side_effect_and_bounded_arguments(self):
        with tempfile.TemporaryDirectory() as directory:
            first = subprocess.run([self.fixture, 'count'], cwd=directory, capture_output=True)
            second = subprocess.run([self.fixture, 'count'], cwd=directory, capture_output=True)
            self.assertEqual((first.returncode, first.stdout), (0, b'once\n'))
            self.assertEqual(second.returncode, 125)
            self.assertEqual((Path(directory) / 'executed').read_bytes(), b'once\n')
        for args in [('sleep', '31'), ('marker', '../bad'), ('noop', 'extra')]:
            result = subprocess.run([self.fixture, *args], capture_output=True)
            self.assertEqual((result.returncode, result.stderr), (125, b'fixture_failed\n'))


if __name__ == '__main__':
    unittest.main()
