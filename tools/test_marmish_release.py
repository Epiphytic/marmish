"""Offline release layout, isolated frontend and parser contract tests."""
import ast
import collections
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import marmish_app
import marmish_installed as installed
import marmish_release as release


def diagnostic_source_modules():
    """Map the harness's local import closure without importing those modules."""
    base = Path(__file__).resolve().parent
    modules, pending = {}, ['marmish_e2e']
    while pending:
        name = pending.pop()
        path = base / (name + '.py')
        if name in modules or not path.is_file():
            continue
        tree = modules[name] = ast.parse(path.read_text())
        pending.extend(local_import_names(tree))
    return modules


def local_import_names(tree):
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            yield from (item.name.split('.')[0] for item in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            yield node.module.split('.')[0]



def stable_ast_dump(node):
    options = {'include_attributes': False}
    if sys.version_info >= (3, 13):
        options['show_empty'] = True
    return ast.dump(node, **options)


def called_name(node):
    if isinstance(node.func, ast.Name):
        return node.func.id
    if isinstance(node.func, ast.Attribute):
        return node.func.attr
    return ''


class DiagnosticErrors(ast.NodeVisitor):
    """Collect fixed categories and fingerprint dynamic propagation separately."""
    def __init__(self, module):
        self.module = module
        self.errors = set()
        self.dynamic = collections.Counter()

    def record(self, category, label):
        if category is None:
            return
        if isinstance(category, ast.Constant) and isinstance(category.value, str):
            self.errors.add(category.value)
            return
        key = self.module + ':' + label + ':' + stable_ast_dump(category)
        self.dynamic[key] += 1

    def visit_Call(self, node):
        name = called_name(node)
        if name in ('require', 'SmokeError', 'RpcError', 'SandboxError'):
            position = 1 if name == 'require' else 0
            category = (node.args[position] if len(node.args) > position else
                        next((item.value for item in node.keywords
                              if item.arg in ('category', 'message', 'error')), None))
            self.record(category, name)
        self.generic_visit(node)

    def visit_FunctionDef(self, node):
        if node.name == 'require':
            defaults = list(zip(node.args.args[-len(node.args.defaults):], node.args.defaults))
            defaults += list(zip(node.args.kwonlyargs, node.args.kw_defaults))
            for argument, default in defaults:
                if argument.arg in ('category', 'message', 'error'):
                    self.record(default, 'require_default')
        self.generic_visit(node)

    visit_AsyncFunctionDef = visit_FunctionDef


def e2e_literal_vocabulary(tree, call):
    values = set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call) or called_name(node) != call:
            continue
        if not (node.args and isinstance(node.args[0], ast.Constant)
                and isinstance(node.args[0].value, str)):
            raise AssertionError('nonliteral E2E ' + call)
        values.add(node.args[0].value)
    return values


class StopBeforeProfile(Exception):
    pass


def fixture(parent):
    version = '0.1.0-experimental.1'
    build_target = installed.target()
    root = parent / ('marmish-' + version + '-' + build_target)
    root.mkdir()
    for name in installed.payload_paths(build_target):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        if name.startswith('tools/'):
            path.write_bytes((release.REPO / name).read_bytes())
        elif name.startswith('bin/'):
            path.write_text(release.launcher('server_main' if name.endswith('marmishd') else 'client_main'))
        else:
            path.write_bytes(b'public test fixture\n')
        path.chmod(installed.payload_mode(name))
    for graph in ('marmish', 'mdk'):
        path = root / 'licenses' / graph / 'manifest.json'
        path.parent.mkdir(parents=True)
        path.write_text('{}\n')
    record = {'format': 1, 'version': version, 'target': build_target,
              'source_commit': 'a' * 40, 'experimental': True, 'limitations': ['Test only.'],
              'prerequisites': {'python': '3.11', 'glibc': '2.17' if sys.platform.startswith('linux') else None,
                                'shared_libraries': ['libc.so.6'] if sys.platform.startswith('linux') else ['/usr/lib/libSystem.B.dylib']},
              'files': {str(path.relative_to(root)): {'sha256': installed.file_digest(path),
                        'mode': installed.payload_mode(str(path.relative_to(root)))}
                        for path in root.rglob('*') if path.is_file()}}
    (root / 'release.json').write_text(json.dumps(record))
    (root / 'release.json').chmod(0o644)
    root.chmod(0o755)
    for path in root.rglob('*'):
        if path.is_dir():
            path.chmod(0o755)
        elif str(path.relative_to(root)).startswith('licenses/'):
            path.chmod(0o644)
    return root, record


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.original_umask = os.umask(0o077)

    def tearDown(self):
        os.umask(self.original_umask)

    def test_layout_is_complete_and_strict(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, record = fixture(Path(temporary).resolve())
            self.assertEqual(installed.validate_record(record), record)
            record['files']['tools/test_fixture.py'] = {'sha256': 'a' * 64, 'mode': 420}
            with self.assertRaises(ValueError):
                installed.validate_record(record)
            del record['files']['tools/test_fixture.py']
            del record['files']['libexec/marmish/marmish-keychain']
            with self.assertRaises(ValueError):
                installed.validate_record(record)

    def test_installed_defaults_absolute_and_tamper_detection(self):
        with tempfile.TemporaryDirectory(prefix='marmish space $ ` ') as temporary:
            root, _ = fixture(Path(temporary).resolve())
            with patch.object(installed, '__file__', str(root / 'tools/marmish_installed.py')):
                defaults = installed.installed_defaults()
                self.assertEqual(defaults['keychain_helper'], root / 'libexec/marmish/marmish-keychain')
                self.assertTrue(all(value.is_absolute() for key, value in defaults.items() if key != 'wnd_sha256'))
                (root / 'libexec/marmish/wnd').write_bytes(b'tampered')
                with self.assertRaises(ValueError):
                    installed.installed_defaults()

    def test_symlink_assets_and_writable_ancestor_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, _ = fixture(Path(temporary).resolve())
            with patch.object(installed, '__file__', str(root / 'tools/marmish_installed.py')):
                binary = root / 'libexec/marmish/marmish-keychain'
                data = binary.read_bytes()
                binary.unlink()
                target = root / 'decoy'
                target.write_bytes(data)
                binary.symlink_to(target)
                with self.assertRaises(ValueError):
                    installed.installed_defaults()
                binary.unlink()
                binary.write_bytes(data)
                binary.chmod(0o755)
                root.chmod(0o777)
                try:
                    with self.assertRaises(ValueError):
                        installed.installed_defaults()
                finally:
                    root.chmod(0o755)

    def test_frontend_help_imports_are_isolated(self):
        with tempfile.TemporaryDirectory(prefix='marmish space $ ` ') as temporary:
            parent = Path(temporary).resolve()
            root, _ = fixture(parent)
            decoy = parent / 'decoy'
            decoy.mkdir()
            (decoy / 'marmish_app.py').write_text('raise RuntimeError("untrusted import")')
            (decoy / 'marmish-keychain').write_text('#!/bin/sh\nexit 99\n')
            (decoy / 'marmish-keychain').chmod(0o755)
            environment = dict(os.environ, PYTHONPATH=str(decoy), PATH=str(decoy) + ':' + os.environ['PATH'])
            for command in ('marmish', 'marmishd'):
                result = subprocess.run([root / 'bin' / command, '--help'], cwd=decoy,
                                        env=environment, capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('--doctor', result.stdout)
            self.assertFalse(list(root.rglob('__pycache__')))

    def test_explicit_parser_options_abbreviations_and_command_divider(self):
        defaults = {'wnd': Path('/bundled/wnd'), 'wnd_sha256': 'b' * 64,
                    'keychain_helper': Path('/bundled/keychain'), 'wnd_manifest': Path('/bundled/wnd.json')}
        captured = []
        def stop(args):
            captured.append(args)
            raise StopBeforeProfile()
        with patch.object(installed, 'installed_defaults', return_value=defaults), patch.object(marmish_app, 'Profile', side_effect=stop):
            with self.assertRaises(StopBeforeProfile):
                marmish_app.client_main(['-i', '/profile', '--relay', 'wss://example.org/',
                                         '--wnd=/explicit/wnd', '--keychain-h=/explicit/keychain',
                                         'npub@relay', '--', '/bin/echo', '--wnd=remote literal $ `'])
        self.assertEqual(captured[0].wnd, Path('/explicit/wnd'))
        self.assertEqual(captured[0].keychain_helper, Path('/explicit/keychain'))
        self.assertEqual(captured[0].wnd_manifest, Path('/bundled/wnd.json'))

    def test_source_checkout_does_not_gain_implicit_paths(self):
        self.assertEqual(installed.installed_defaults(), {})
        with self.assertRaises(SystemExit) as error, contextlib.redirect_stderr(io.StringIO()):
            marmish_app.client_main(['-i', '/profile', '--relay', 'wss://example.org/', '--init'])
        self.assertEqual(error.exception.code, 2)

    def test_doctor_exits_before_required_options_and_profile(self):
        report = {'ok': True, 'checks': {'test': True}, 'note': 'static'}
        with patch.object(installed, 'doctor', return_value=report), patch.object(marmish_app, 'Profile') as profile:
            for entry in (marmish_app.client_main, marmish_app.server_main):
                with self.assertRaises(SystemExit) as error, contextlib.redirect_stdout(io.StringIO()):
                    entry(['--doctor'])
                self.assertEqual(error.exception.code, 0)
            profile.assert_not_called()

    def test_archive_deterministic_ustar_modes_and_allowlist(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary).resolve()
            root, record = fixture(parent)
            a, b = parent / 'a.tar.gz', parent / 'b.tar.gz'
            release.archive(root, a)
            release.archive(root, b)
            self.assertEqual(a.read_bytes(), b.read_bytes())
            with tarfile.open(a) as archive:
                members = archive.getmembers()
            self.assertTrue(all(not member.pax_headers and (member.isfile() or member.isdir()) for member in members))
            self.assertEqual({member.name[len(root.name)+1:] for member in members if member.isfile()},
                             set(record['files']) | {'release.json'})

    def test_stager_verifies_the_packaged_tree_and_normalizes_private_umask(self):
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary).resolve()
            source, original = fixture(parent)
            runtime = parent / 'runtime'
            runtime.mkdir()
            for path in (source / 'libexec/marmish').iterdir():
                (runtime / path.name).write_bytes(path.read_bytes())
            import shutil
            shutil.copytree(source / 'licenses', runtime / 'licenses')
            destination = parent / 'staged' / source.name
            destination.parent.mkdir()
            with patch.object(release, 'REPO', source), patch.object(release, 'verify_staged') as verify, \
                    patch.object(release, 'shared_dependencies', return_value=original['prerequisites']), \
                    patch('marmish_release_licenses.validate') as licenses:
                record = release.stage(destination, runtime, original['version'],
                                       original['target'], original['source_commit'])
            verify.assert_called_once_with(destination)
            licenses.assert_called_once_with(runtime / 'licenses')
            self.assertEqual(record['files'].keys(), original['files'].keys())
            self.assertEqual(destination.stat().st_mode & 0o777, 0o755)
            self.assertTrue(all(path.stat().st_mode & 0o777 == 0o755
                                for path in destination.rglob('*') if path.is_dir()))

    def test_builder_archive_installs_and_runs_isolated_frontends(self):
        from test_marmish_install import load_installer
        installer = load_installer()
        with tempfile.TemporaryDirectory(prefix="marmish integration space '$ ") as temporary:
            parent = Path(temporary).resolve()
            root, record = fixture(parent)
            archive = parent / 'release.tar.gz'
            release.archive(root, archive)
            prefix = parent / 'installed'
            launchers = parent / 'bin'
            installed_record = installer.install_archive(archive, prefix, launchers,
                                                         record['version'], record['target'])
            self.assertEqual(installed_record, record)
            self.assertEqual(installer.install_archive(archive, prefix, launchers,
                                                       record['version'], record['target']), record)
            for name in ('marmish', 'marmishd'):
                result = subprocess.run([launchers / name, '--help'], cwd=parent,
                                        env=dict(os.environ, PYTHONPATH='/nonexistent'),
                                        capture_output=True, text=True, timeout=10)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn('--doctor', result.stdout)

    def test_library_requirements_reject_untrusted_paths_before_dlopen(self):
        accepted = ('libc.so.6', 'ld-linux-x86-64.so.2', '/usr/lib/libSystem.B.dylib',
                    '/System/Library/Frameworks/Security.framework/Versions/A/Security')
        rejected = ('', '.', '..', '../evil.so', 'relative/evil.so', '/tmp/evil.so',
                    '/usr/libexec/evil.so', '/usr/lib/../evil.so', '/usr/lib//evil.so',
                    '/usr/lib/./evil.so', '/System/Library/', '/usr/lib/evil\n.so',
                    '/usr/lib/libevil.so?query', '@rpath/evil.dylib', 1, None)
        for name in accepted:
            self.assertTrue(installed.valid_library(name), name)
        for name in rejected:
            self.assertFalse(installed.valid_library(name), name)
        with tempfile.TemporaryDirectory() as temporary:
            root, record = fixture(Path(temporary).resolve())
            record['prerequisites']['shared_libraries'] = ['/tmp/evil.so']
            (root / 'release.json').write_text(json.dumps(record))
            with patch.object(installed, '__file__', str(root / 'tools/marmish_installed.py')), \
                    patch.object(installed.ctypes, 'CDLL') as load:
                report = installed.doctor()
                self.assertFalse(report['ok'])
                self.assertEqual(report['error'], 'invalid_local_release')
                load.assert_not_called()

    def test_doctor_corrupt_manifest_has_fixed_sanitized_error(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, _ = fixture(Path(temporary).resolve())
            (root / 'release.json').write_text('private diagnostic sentinel invalid JSON')
            with patch.object(installed, '__file__', str(root / 'tools/marmish_installed.py')):
                report = installed.doctor()
            self.assertFalse(report['ok'])
            self.assertEqual(report['error'], 'invalid_local_release')
            self.assertNotIn('sentinel', json.dumps(report))

    def test_live_evidence_binds_archive_and_sanitizes_failures(self):
        import marmish_release_acceptance as acceptance
        manifest = {'source_commit': 'a' * 40, 'target': installed.target()}
        digest = 'b' * 64
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary).resolve() / 'installed-e2e.json'
            cases = [subprocess.TimeoutExpired(['secret command'], 960, output='private sentinel'),
                     subprocess.CompletedProcess([], 0, 'non-JSON private sentinel'),
                     subprocess.CompletedProcess([], 1, '{"outcome":"failed","secret":"private sentinel"}'),
                     subprocess.CompletedProcess([], 0, '[]')]
            for completed in cases:
                options = {'side_effect': completed} if isinstance(completed, Exception) else {'return_value': completed}
                with patch.object(acceptance.subprocess, 'run', **options):
                    with self.assertRaisesRegex(ValueError, '^installed_live_acceptance_failed$'):
                        acceptance.live_acceptance(['ignored'], manifest, digest, evidence)
                result = json.loads(evidence.read_text())
                self.assertEqual(result['outcome'], 'failed')
                self.assertEqual(result['distribution_archive_sha256'], digest)
                self.assertEqual(result['distribution_source_commit'], manifest['source_commit'])
                self.assertNotIn('sentinel', evidence.read_text())
            with patch.object(acceptance.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, '{"outcome":"passed","assertions":["checked"]}')):
                acceptance.live_acceptance(['ignored'], manifest, digest, evidence)
            result = json.loads(evidence.read_text())
            self.assertEqual(result['distribution_archive_sha256'], digest)
            self.assertEqual(result['assertions'], ['checked'])


    def test_failure_diagnostic_validates_fields_and_preserves_failure_binding(self):
        import marmish_release_acceptance as acceptance
        manifest = {'source_commit': 'a' * 40, 'target': installed.target()}
        assertion = 'installed_launchers_and_bundled_defaults_preserve_special_character_argv'
        report = {'outcome': 'passed', 'stage': 'identity_setup',
                  'error': 'namespace_start_failed', 'assertions': [assertion, assertion],
                  'local_cleanup': 'complete', 'distribution_source_commit': 'forged',
                  'distribution_archive_sha256': 'forged', 'target': 'forged',
                  'stdout': 'private sentinel', 'stderr': 'private sentinel'}
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary).resolve() / 'installed-e2e.json'
            with patch.object(acceptance.subprocess, 'run', return_value=
                    subprocess.CompletedProcess([], 1, json.dumps(report))):
                with self.assertRaisesRegex(ValueError, '^installed_live_acceptance_failed$'):
                    acceptance.live_acceptance(['ignored'], manifest, 'b' * 64, evidence)
            result = json.loads(evidence.read_text())
            self.assertEqual(result, {'outcome': 'failed', 'error': 'installed_live_command_failed',
                'distribution_source_commit': manifest['source_commit'],
                'distribution_archive_sha256': 'b' * 64, 'target': manifest['target'],
                'diagnostic': {'format': 1, 'stage': 'identity_setup',
                    'error': 'namespace_start_failed', 'assertions': [assertion],
                    'local_cleanup': 'complete'}})
            self.assertNotIn('sentinel', evidence.read_text())
        from test_marmish_publish import PublicationTests
        fixture = PublicationTests()
        fixture.setUp()
        try:
            fixture.report.write_text(json.dumps(result))
            with self.assertRaisesRegex(ValueError, '^invalid_installed_live_evidence$'):
                fixture.aggregate()
        finally:
            fixture.tearDown()
            fixture.doCleanups()

    def test_failure_diagnostic_rejects_arbitrary_values_without_type_errors(self):
        import marmish_release_acceptance as acceptance
        assertion = next(iter(acceptance.DIAGNOSTIC_ASSERTIONS))
        secret = 'S' * 60
        for value in (None, secret, {}, [], 23, True):
            with self.subTest(value_type=type(value).__name__):
                result = acceptance.failure_diagnostic({'stage': value, 'error': value,
                    'local_cleanup': value, 'assertions': [{}, [], 3, secret, assertion, assertion]})
                self.assertEqual(result, {'format': 1, 'stage': 'unknown',
                    'error': 'unrecognized_error', 'local_cleanup': 'unknown',
                    'assertions': [assertion]})
                self.assertNotIn(secret, json.dumps(result))
        for value in (None, {}, assertion, [assertion] * 65):
            self.assertEqual(acceptance.failure_diagnostic({'assertions': value})['assertions'], [])
        manifest = {'source_commit': 'a' * 40, 'target': installed.target()}
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary).resolve() / 'installed-e2e.json'
            report = {'outcome': 'failed', 'stage': secret, 'error': secret,
                      'assertions': [secret], 'local_cleanup': secret, 'secret': secret}
            with patch.object(acceptance.subprocess, 'run', return_value=
                    subprocess.CompletedProcess([], 0, json.dumps(report))):
                with self.assertRaises(ValueError):
                    acceptance.live_acceptance(['ignored'], manifest, 'b' * 64, evidence)
            self.assertNotIn(secret, evidence.read_text())

    def test_diagnostic_ast_dump_preserves_empty_fields_across_python_versions(self):
        node = ast.parse('str(error)', mode='eval').body
        expected = ("Call(func=Name(id='str', ctx=Load()), "
                    "args=[Name(id='error', ctx=Load())], keywords=[])")
        self.assertEqual(stable_ast_dump(node), expected)

    def test_diagnostic_frozen_vocabularies_cover_source_and_reviewed_dynamic_calls(self):
        import marmish_release_acceptance as acceptance
        modules = diagnostic_source_modules()
        errors, dynamic = set(), collections.Counter()
        for name, tree in modules.items():
            collector = DiagnosticErrors(name)
            collector.visit(tree)
            errors.update(collector.errors)
            dynamic.update(collector.dynamic)
        self.assertEqual(set(modules), acceptance.DIAGNOSTIC_MODULES)
        self.assertFalse(errors - acceptance.DIAGNOSTIC_ERRORS)
        # Existing dynamic propagation is intentionally not telemetry authorization.
        # Exact structural fingerprints/counts make each new dynamic category call
        # an explicit review event without depending on unrelated source line shifts.
        reviewed_dynamic = {"marmish_app:SmokeError:Name(id='error', ctx=Load())": 1,
 "marmish_e2e:SmokeError:Name(id='category', ctx=Load())": 1,
 "marmish_keys:SmokeError:Subscript(value=Name(id='response', ctx=Load()), slice=Constant(value='error'), ctx=Load())": 1,
 "marmish_native_validation:SmokeError:Subscript(value=Name(id='response', ctx=Load()), slice=Constant(value='error'), ctx=Load())": 1,
 "marmish_protocol:SmokeError:Name(id='error', ctx=Load())": 1,
 "marmish_rpc:RpcError:Call(func=Name(id='str', ctx=Load()), args=[Name(id='error', ctx=Load())], keywords=[])": 1,
 "marmish_rpc:RpcError:Name(id='category', ctx=Load())": 1,
 "marmish_transport_macos:SandboxError:BinOp(left=Constant(value='native_control_'), op=Add(), right=Name(id='label', ctx=Load()))": 1,
 "marmish_transport_macos:SandboxError:BinOp(left=Constant(value='native_probe_'), op=Add(), right=Name(id='label', ctx=Load()))": 1,
 "mdk_smoke:SmokeError:Name(id='category', ctx=Load())": 1}
        self.assertEqual(dict(dynamic), reviewed_dynamic)
    def test_diagnostic_stages_and_assertions_cover_e2e_literals(self):
        import marmish_release_acceptance as acceptance
        tree = diagnostic_source_modules()['marmish_e2e']
        stages = e2e_literal_vocabulary(tree, 'checkpoint') | {'setup', 'complete'}
        assertions = e2e_literal_vocabulary(tree, 'passed')
        self.assertFalse(stages - acceptance.DIAGNOSTIC_STAGES)
        self.assertFalse(assertions - acceptance.DIAGNOSTIC_ASSERTIONS)
        file_assertions = {'actual_client_and_server_cli_processes_execute_command_via_marmot',
 'byte_exact_binary_stdin_stdout_stderr_eof',
 'exact_admitted_retry_and_status_first_retry_execute_once_conflict_rejected',
 'fresh_independent_mls_group_configurable_retention_same_principal_reattach',
 'frontend_disconnect_does_not_cancel_running_worker',
 'installed_launchers_and_bundled_defaults_preserve_special_character_argv',
 'literal_argv_spaces_unicode_quotes_newline_dollar_backticks_leading_dash_nonutf8',
 'live_additive_default_and_per_key_denials_path_and_shell_refusal',
 'live_cancellation_reports_signal_and_remains_queryable',
 'live_ordinary_room_and_unallowlisted_identity_invites_remain_unaccepted',
 'live_stale_challenge_wrong_group_and_chat_payload_never_execute',
 'persisted_mls_room_and_command_output_recovered_after_frontend_restart',
 'policy_denials_create_no_job_and_preserve_token_for_allowed_execution',
 'previous_boot_valid_control_frame_rejected_after_restart',
 'separate_binary_stderr_and_nonzero_exit'}
        self.assertEqual(len(file_assertions), 15)
        self.assertLessEqual(file_assertions, acceptance.DIAGNOSTIC_ASSERTIONS)

    def test_glibc_patch_version_is_preserved(self):
        with tempfile.TemporaryDirectory() as temporary:
            root, record = fixture(Path(temporary).resolve())
            with patch.object(release, 'output', side_effect=lambda command:
                              '(NEEDED) Shared library: [libc.so.6]' if '-d' in command
                              else 'Name: GLIBC_2.2.5 Name: GLIBC_2.2.3'):
                prerequisites = release.shared_dependencies(root, 'x86_64-unknown-linux-gnu')
            self.assertEqual(prerequisites['glibc'], '2.2.5')
            if record['target'].endswith('linux-gnu'):
                record['prerequisites'] = prerequisites
                installed.validate_record(record)

    def test_shared_library_and_glibc_measurement(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            (root / 'libexec/marmish').mkdir(parents=True)
            (root / 'libexec/marmish/wnd').touch()
            with patch.object(release, 'output', side_effect=[' (NEEDED) Shared library: [libc.so.6]\n',
                                                             'Name: GLIBC_2.9 Name: GLIBC_2.38 Name: GLIBC_2.17']):
                record = release.shared_dependencies(root, 'x86_64-unknown-linux-gnu')
            self.assertEqual(record['glibc'], '2.38')
            self.assertEqual(record['shared_libraries'], ['libc.so.6'])


if __name__ == '__main__':
    unittest.main()
