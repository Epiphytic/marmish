import io
import json
import os
from pathlib import Path
import socket
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
from contextlib import redirect_stdout

import marmish_installed_terminal_baseline as baseline
import marmish_local_qualification as qualification


class InstalledTerminalBaselineTests(unittest.TestCase):
    def test_build_mode_excludes_any_external_route_or_assets(self):
        base = ['qualification', '--source', '.', '--commit', 'a' * 40,
                '--build-installed-terminal', '--licenses', 'some-file']
        for more in (['--live-relay', 'wss://example.invalid/'],
                     ['--version', '0.3.1'], ['--assets', '.'], ['--terminal', 'wss']):
            with (self.subTest(more=more), patch('sys.argv', base + more),
                  patch.object(qualification.os, 'geteuid', return_value=501),
                  patch.object(qualification.signal, 'signal'),
                  patch.object(qualification, 'verify_checkout') as checkout,
                  redirect_stdout(io.StringIO()) as output):
                self.assertEqual(qualification.main(), 1)
                checkout.assert_not_called()
                report = json.loads(output.getvalue())
                self.assertEqual(report['failure'], 'build_mode_arguments')
                self.assertEqual(report['stock_private_wss'], 'unrun')
                self.assertNotIn('example.invalid', json.dumps(report))

    def test_exact_cargo_controller_identity_and_location(self):
        with tempfile.TemporaryDirectory() as path:
            source = Path(path).resolve()
            target = source / 'target'
            controller = target / 'debug/deps/marmish_native-0123456789abcdef'
            controller.parent.mkdir(parents=True)
            controller.touch(mode=0o700)
            package = 'path+' + (source / 'crates/marmish-native').as_uri() + '#0.3.1'
            row = {'reason': 'compiler-artifact', 'profile': {'test': True},
                   'package_id': package, 'target': {'name': 'marmish_native', 'kind': ['lib']},
                   'executable': str(controller)}
            def encode(*rows):
                return b'\n'.join(json.dumps(item).encode() for item in rows)

            self.assertEqual(baseline.executable(encode(row), source, target, '0.3.1'), controller)
            for raw in (
                b'',
                encode(row, row),
                encode(dict(row, package_id=package.replace('#0.3.1', '#0.3.0'))),
                encode(dict(row, profile={'test': False})),
                encode(dict(row, target={'name': 'marmish_native', 'kind': ['bin']})),
            ):
                with self.assertRaisesRegex(qualification.Refused, '^fixture_controller_count$'):
                    baseline.executable(raw, source, target, '0.3.1')
            with self.assertRaisesRegex(qualification.Refused, '^fixture_controller_path$'):
                baseline.executable(encode(dict(row, executable=str(controller.with_name('wrong')))),
                                    source, target, '0.3.1')

    def test_signing_and_abi_contract_is_fixed_by_target(self):
        for target in ('aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu'):
            self.assertEqual(baseline.package_contract(target), ('none', 'glibc=2.39'))
        self.assertEqual(baseline.package_contract('aarch64-apple-darwin'), ('adhoc', 'macos=15.0'))

    def test_package_producer_creates_fresh_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            release = root / 'target/release'
            release.mkdir(parents=True)
            for name in baseline.PAYLOADS:
                (release / name).write_bytes(name.encode())
            target = 'x86_64-unknown-linux-gnu'
            report = {}
            calls = []

            def produce(argv, _environment, *, timeout, success=True):
                self.assertLessEqual(timeout, 120)
                calls.append(Path(argv[0]).name)
                if not success:
                    self.assertEqual(argv, ['/bin/sh', root / 'scripts/v02-inspect-native.sh',
                                            root / 'inputs'])
                    return 0, b''
                self.assertEqual(argv, [release / 'marmish-release-tool', 'package',
                    '--input-dir', root / 'inputs', '--output-dir', root / 'assets',
                    '--licenses', root / 'licenses', '--signing', 'none', '--minimum', 'glibc=2.39'])
                output = root / 'assets'
                self.assertFalse(output.exists())
                output.mkdir()
                stem = f'-0.3.1-{target}'
                (output / f'marmish{stem}.manifest').write_text(
                    '1\n2\n3\n4\nsigning none notarized=no\nminimum glibc=2.39\n')
                for name in (*baseline.PAYLOADS, 'THIRD-PARTY-LICENSES'):
                    (output / f'{name}{stem}').write_bytes(name.encode())
                return 0, b''

            with patch.object(baseline.supervisor, 'execute', side_effect=produce):
                output = baseline.package(root, root, '0.3.1', target, {},
                                          SimpleNamespace(remaining=lambda _cap: 42), report)
            self.assertEqual(calls, ['sh', 'marmish-release-tool'])
            self.assertEqual(output, root / 'assets')
            self.assertEqual(set(report['package_asset_sha256']),
                             set(baseline.PAYLOADS) | {'THIRD-PARTY-LICENSES'})
            self.assertEqual(report['package_asset_sha256']['marmish'],
                             report['package_inputs_sha256']['marmish'])

    def test_cleanup_only_names_the_exact_owned_tmux_socket(self):
        with tempfile.TemporaryDirectory(prefix='mq-', dir=Path('/tmp').resolve()) as path:
            root = Path(path)
            slot = root / 'ir-123/s/t' / ('a' * 32)
            for directory in reversed(slot.relative_to(root).parents):
                if directory != Path('.'):
                    (root / directory).mkdir(mode=0o700)
            slot.mkdir(mode=0o700)
            sock = slot / 's'
            with socket.socket(socket.AF_UNIX) as owned:
                owned.bind(str(sock))
            tmux = Path('/usr/bin/tmux')
            env = {'LC_ALL': 'C'}
            with patch.object(baseline.supervisor, 'execute', side_effect=lambda *_a, **_k: sock.unlink()) as run:
                baseline.cleanup_tmux(root, tmux, env)
            run.assert_called_once_with([tmux, '-S', sock, 'kill-server'], env, timeout=3)
            sock.symlink_to('/unrelated/s')
            with patch.object(baseline.supervisor, 'execute') as run:
                with self.assertRaisesRegex(qualification.Refused, '^fixture_cleanup_incomplete$'):
                    baseline.cleanup_tmux(root, tmux, env)
            run.assert_not_called()

    def test_environment_does_not_inherit_transport_or_compilation_overrides(self):
        source = Path(__file__).resolve().parent.parent
        account = SimpleNamespace(pw_dir='/operator/home', pw_name='operator')
        with patch.dict(os.environ, {'RUSTFLAGS': 'override', 'RUSTC_WRAPPER': 'override',
                                    'CARGO_BUILD_TARGET': 'override', 'http_proxy': 'override',
                                    'CARGO_TARGET_DIR': 'override', 'RUNNER_TRACKING_ID': 'keep'},
                        clear=True):
            env = baseline.environment(Path('/private/test'), source, 'a' * 40, account,
                                       'aarch64-apple-darwin')
        self.assertEqual(env['MACOSX_DEPLOYMENT_TARGET'], '15.0')
        self.assertEqual(env['RUNNER_TRACKING_ID'], 'keep')
        self.assertEqual(env['CARGO_TARGET_DIR'], '/private/test/target')
        self.assertNotIn('RUSTFLAGS', env)
        self.assertNotIn('RUSTC_WRAPPER', env)
        self.assertNotIn('CARGO_BUILD_TARGET', env)
        self.assertNotIn('http_proxy', env)


if __name__ == '__main__':
    unittest.main()
