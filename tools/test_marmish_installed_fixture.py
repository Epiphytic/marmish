"""Native CI fixture checks; these are not installed Marmot acceptance evidence."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import marmish_installed_fixture as fixture


class PairVectors(unittest.TestCase):
    def test_golden_pair(self):
        ctx = {'run': '1', 'attempt': '1', 'sha': 'a' * 40}
        client = {'role': 'linux-x64', 'client': 'b' * 64, 'nonce': 'c' * 64}
        server = {'role': 'macos-arm64', 'server': 'd' * 64, 'nonce': 'e' * 64}
        result = fixture.derive_pair(ctx, 'f' * 64, client, server)
        self.assertEqual(result, GOLDEN_PAIR)
        changed = fixture.derive_pair(ctx, 'f' * 64, dict(client, nonce='0' * 64), server)
        self.assertNotEqual(changed['context'], result['context'])
        self.assertNotEqual(result['invocation'], result['probe_invocation'])

    def test_payload_vectors(self):
        data = fixture.stdin_bytes('00' * 32)
        self.assertEqual(len(data), 8192)
        self.assertEqual(data[:49], b'marmish-stdin-v1\0' + bytes(32))
        self.assertEqual(set(data), set(range(256)))
        self.assertEqual(hashlib.sha256(data).hexdigest(),
                         '96e3cf3ceebb8902414f523663dab11bf73837c93ce47ab8211634502a3efaa1')


class CompilerPreparation(unittest.TestCase):
    def test_identity_cleanup_and_report_reserves(self):
        with patch.object(fixture, 'continuous_time', return_value=10):
            self.assertEqual(fixture.dependency_deadline({'preparation_deadline': 900}, 1000), 275)
        with patch.object(fixture, 'continuous_time', return_value=275):
            with self.assertRaisesRegex(ValueError, 'fixture_preparation_timeout'):
                fixture.dependency_deadline({'preparation_deadline': 900}, 1000)

    def test_compile_launch_requires_isolated_no_bytecode_python(self):
        config = {'version': 'rustc 1.97.1 fixture'}
        result = {'returncode': 0, 'timed_out': False, 'output_limited': False,
                  'cleanup': 'complete'}
        with patch.object(fixture, '_compiler_config', return_value=config), \
                patch('marmish_installed_state.run_bounded', return_value=result) as run:
            fixture._compile({'python': '/fixed/python'}, {}, fixture.continuous_time()+10)
        self.assertEqual(run.call_args.args[0], ['/fixed/python', '-I', '-B',
                         str(Path(fixture.__file__).resolve()), '--compile-worker'])
        self.assertIs(run.call_args.kwargs['build_scope'], True)

    def test_worker_explicit_environment_ignores_ambient_compiler_credentials(self):
        import io
        from types import SimpleNamespace
        config = dict(stage='fetch', source_sha256=fixture.source_sha256(),
                      cargo='/private/compiler/bin/cargo', rustc='/private/compiler/bin/rustc',
                      rustdoc='/private/compiler/bin/rustdoc', cargo_home='/private/cargo-home',
                      target='/private/target', triple='x86_64-unknown-linux-gnu',
                      linker='/usr/bin/cc', constants={})
        ambient = dict(PATH='host', HOME='/private/home', EPIPHYTIC_EXECUTION_ID='marker',
                       CARGO_HOME='SECRET', RUSTUP_HOME='SECRET', RUSTC_WRAPPER='SECRET',
                       CARGO_REGISTRY_TOKEN='SECRET', RUSTFLAGS='SECRET', GITHUB_TOKEN='SECRET')
        with patch.dict(os.environ, ambient, clear=True), \
                patch.object(fixture.sys, 'stdin', SimpleNamespace(buffer=io.BytesIO(json.dumps(config).encode()))), \
                patch.object(fixture.os, 'chdir'), \
                patch.object(fixture.os, 'execve', side_effect=RuntimeError('stop')) as execute:
            with self.assertRaisesRegex(RuntimeError, 'stop'):
                fixture._compile_worker()
        env = execute.call_args.args[2]
        self.assertEqual(env['CARGO_HOME'], '/private/cargo-home')
        self.assertEqual(env['RUSTC_WRAPPER'], '')
        self.assertEqual(env['RUSTC_WORKSPACE_WRAPPER'], '')
        self.assertEqual(env['RUSTC'], '/private/compiler/bin/rustc')
        self.assertEqual(env['CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER'], '/usr/bin/cc')
        self.assertFalse(any('SECRET' in value for value in env.values()))
        self.assertNotIn('RUSTUP_HOME', env)

    def test_source_and_private_cargo_configuration_are_refused(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary) / 'source'
            source.mkdir()
            home = Path(temporary) / 'cargo'
            home.mkdir()
            with patch.object(fixture, 'SOURCE', source):
                for path in (source / '.cargo/config.toml', home / 'config'):
                    path.parent.mkdir(exist_ok=True)
                    path.write_text('[build]\nrustc-wrapper="SECRET"')
                    with self.assertRaisesRegex(ValueError, 'fixture_cargo_config'):
                        fixture._no_cargo_config(home)
                    path.unlink()

    def test_fixture_intermediate_directories_are_private_with_permissive_umask(self):
        with tempfile.TemporaryDirectory(prefix='mci-', dir='/tmp') as temporary:
            root = Path(temporary)
            artifact = root / 'fixture-target/debug/marmish-installed-fixture'
            artifact.parent.mkdir(parents=True)
            artifact.write_bytes(b'fixed executable')
            old = os.umask(0)
            try:
                with patch.object(fixture, '_compile', return_value='rustc fixed'), \
                        patch('marmish_installed_state.register_executable'):
                    fixture.build_fixture({'root': str(root), 'ctx': {'sha': 'a' * 40}},
                        'linux-x64', dict(context='01'*32, challenge='23'*32, invocation='45'*16),
                        deadline=float('inf'))
            finally:
                os.umask(old)
            for directory in (root/'f', root/'f/linux-x64', root/'r'):
                self.assertEqual(directory.stat().st_mode & 0o777, 0o700)

    def test_consumption_requires_verified_copy_and_current_owned_source(self):
        from types import SimpleNamespace
        import marmish_installed_bootstrap as owned
        ctx = dict(run='123', attempt='1', sha='a'*40)
        for fails in (False, True):
            with self.subTest(copy_fails=fails), tempfile.TemporaryDirectory() as temporary:
                harness = Path(temporary).resolve()
                source = harness / 'bootstrap/toolchain'
                with patch.object(owned, 'bootstrap', side_effect=lambda path: path.mkdir(mode=0o700)):
                    owned.bootstrap_owned(source, ctx)
                root = harness / 'private'
                root.mkdir(mode=0o700)
                value = dict(root=str(root), state_file=str(harness/'state'), ctx=ctx,
                             role='linux-x64', preparation_deadline=fixture.continuous_time()+900)
                def copied(*args, **kwargs):
                    self.assertTrue(source.exists())
                    if fails:
                        raise ValueError('copy failed')
                    (root/'compiler').mkdir(mode=0o700)
                    return {}
                def version(argv, *args, **kwargs):
                    self.assertFalse(source.exists())
                    return dict(returncode=0, timed_out=False, output_limited=False,
                                cleanup='complete', stdout=(Path(argv[0]).name+' 1.97.1 test').encode())
                with patch.object(fixture, '_copy_toolchain', side_effect=copied), \
                        patch('marmish_installed_state.run_bounded', side_effect=version), \
                        patch.object(fixture, '_compile', return_value='compiler'):
                    if fails:
                        with self.assertRaisesRegex(ValueError, 'copy failed'):
                            fixture.prepare_dependencies(value, deadline=value['preparation_deadline'],
                                toolchain_source=source, consume_source=True)
                        self.assertTrue(source.exists())
                        self.assertEqual(owned.cleanup_bootstrap(source, ctx)['outcome'], 'complete')
                    else:
                        self.assertEqual(fixture.prepare_dependencies(value,
                            deadline=value['preparation_deadline'], toolchain_source=source,
                            consume_source=True), 'compiler')
                        self.assertFalse(source.exists())
                        self.assertEqual(owned.cleanup_bootstrap(source, ctx)['outcome'], 'absent')

    def test_managed_copy_rejects_links_and_changed_payload(self):
        from marmish_ci import RUST, MANIFEST_SHA256
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            (source / 'bin').mkdir(parents=True)
            for name in ('cargo', 'rustc', 'rustdoc'):
                (source / 'bin' / name).write_bytes(b'compiler')
                (source / 'bin' / name).chmod(0o755)
            hashes = {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                      for p in source.rglob('*') if p.is_file()}
            (source / '.marmish-toolchain.json').write_text(json.dumps(
                dict(managed=True, rust=RUST, manifest=MANIFEST_SHA256, hashes=hashes)))
            result = fixture._copy_toolchain(source, root / 'good', deadline=float('inf'))
            self.assertEqual(len(result), 3)
            self.assertEqual((root / 'good/bin').stat().st_mode & 0o777, 0o700)
            for mode in ('symlink', 'hardlink', 'changed'):
                path = source / 'bin/cargo'
                path.unlink()
                if mode == 'symlink':
                    path.symlink_to(source / 'bin/rustc')
                elif mode == 'hardlink':
                    os.link(source / 'bin/rustc', path)
                else:
                    path.write_bytes(b'changed')
                with self.subTest(mode=mode), self.assertRaises(ValueError):
                    fixture._copy_toolchain(source, root / mode, deadline=float('inf'))


class NativeFixture(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp(prefix='mci-', dir='/tmp')).resolve()
        cls.role = 'linux-x64'
        cls.path = cls.root / 'f' / cls.role / 'main'
        cls.path.parent.mkdir(parents=True)
        cls.receipt = cls.root / 'r' / (cls.role + '.json')
        cls.receipt.parent.mkdir()
        cls.context, cls.challenge, cls.invocation = '01' * 32, '23' * 32, '45' * 16
        cls.env = {key: os.environ[key] for key in ('PATH', 'HOME', 'CARGO_HOME', 'RUSTUP_HOME') if key in os.environ}
        cls.env['CARGO_TARGET_DIR'] = str(cls.root / 'target')
        cls.constants = {'ROLE': cls.role, 'CONTEXT': cls.context, 'CHALLENGE': cls.challenge,
                         'INVOCATION': cls.invocation, 'EXECUTABLE': str(cls.path), 'RECEIPT': str(cls.receipt)}
        cls.build(cls.constants)

    @classmethod
    def build(cls, constants):
        env = dict(cls.env, **{'MARMISH_FIXTURE_' + key: value for key, value in constants.items()})
        subprocess.run(['cargo', 'build', '--locked', '--offline', '--manifest-path',
                        str(fixture.SOURCE / 'Cargo.toml')], env=env,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True, timeout=120)
        shutil.copyfile(cls.root / 'target/debug/marmish-installed-fixture', cls.path)
        cls.path.chmod(0o700)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.root)

    def setUp(self):
        self.receipt.unlink(missing_ok=True)

    def invoke(self, args=None, data=None):
        return subprocess.run(args or fixture.command_argv(self.path, self.challenge, self.invocation),
                              input=fixture.stdin_bytes(self.context) if data is None else data,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)

    def test_binary_roundtrip_and_exclusive_receipt(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 7)
        self.assertEqual((result.stdout, result.stderr), fixture.expected_streams(
            self.path, self.context, self.challenge, self.invocation))
        self.assertEqual(json.loads(self.receipt.read_bytes()), fixture.expected_receipt(
            self.role, self.context, self.challenge, self.invocation, self.path))
        self.assertEqual(self.receipt.stat().st_mode & 0o777, 0o600)
        duplicate = self.invoke()
        self.assertEqual((duplicate.returncode, duplicate.stdout, duplicate.stderr), (125, b'', b''))

    def test_bad_arguments_and_input_leave_no_completion(self):
        for data in (b'', b'X' * 8192, fixture.stdin_bytes(self.context)[:-1],
                     fixture.stdin_bytes(self.context) + b'X'):
            with self.subTest(length=len(data)):
                self.receipt.unlink(missing_ok=True)
                result = self.invoke(data=data)
                self.assertEqual((result.returncode, result.stdout, result.stderr), (125, b'', b''))
                self.assertEqual(self.receipt.read_bytes(), b'')
        self.receipt.unlink()
        args = fixture.command_argv(self.path, self.challenge, self.invocation)
        args[-1] = b'wrong'
        self.assertEqual(self.invoke(args=args).returncode, 125)
        self.assertEqual(self.receipt.read_bytes(), b'')

    def test_argv0_mismatch_has_no_completion_receipt(self):
        args = fixture.command_argv(self.path, self.challenge, self.invocation)
        args[0] = b'wrong-argv-zero'
        result = subprocess.run(args, executable=self.path, input=fixture.stdin_bytes(self.context),
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=5)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (125, b'', b''))
        self.assertEqual(self.receipt.read_bytes(), b'')

    def test_receipt_symlink_and_oversized_argv_refused(self):
        destination = self.root / 'untouched'
        destination.write_bytes(b'keep')
        self.receipt.symlink_to(destination)
        result = self.invoke()
        self.assertEqual((result.returncode, result.stdout, result.stderr), (125, b'', b''))
        self.assertEqual(destination.read_bytes(), b'keep')
        self.receipt.unlink()
        args = fixture.command_argv(self.path, self.challenge, self.invocation) + [b'X' * 16384]
        result = self.invoke(args=args)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (125, b'', b''))
        self.assertEqual(self.receipt.read_bytes(), b'')

    def test_blocked_input_alarm_is_fatal(self):
        child = subprocess.Popen(fixture.command_argv(self.path, self.challenge, self.invocation),
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            import time
            until = time.monotonic() + 3
            while not self.receipt.exists() and time.monotonic() < until:
                time.sleep(.01)
            self.assertTrue(self.receipt.exists())
            child.send_signal(signal.SIGALRM)
            out, err = child.communicate(timeout=3)
            self.assertEqual((child.returncode, out, err), (-signal.SIGALRM, b'', b''))
            self.assertEqual(self.receipt.read_bytes(), b'')
        finally:
            if child.poll() is None:
                child.kill()
                child.communicate()

    def test_z_unconfigured_and_partial_fail_without_receipt(self):
        for constants in ({}, {'ROLE': self.role},
                          {key: value for key, value in self.constants.items() if key != 'ROLE'},
                          dict(self.constants, CONTEXT='INVALID'),
                          dict(self.constants, ROLE='invalid'),
                          dict(self.constants, RECEIPT=str(self.root / 'r/wrong.json')),
                          dict(self.constants, EXECUTABLE=str(self.root / 'f/wrong/main'))):
            self.build(constants)
            result = self.invoke()
            self.assertEqual((result.returncode, result.stdout, result.stderr), (125, b'', b''))
            self.assertFalse(self.receipt.exists())


GOLDEN_PAIR = {
    'context': '49c36436218fd0ca0c946e4f970487eb0e5839c254b062c73e111eb40918a8cb',
    'challenge': 'd422eaa9457acbac170d14189e4d642fe7ae21ae08dea983cc9630ba82419cd6',
    'invocation': '702abace9556c46a5471dd5a7fa70804',
    'probe_invocation': '242c26b77862b40fe60dd085be34ec69',
}

if __name__ == '__main__':
    unittest.main()
