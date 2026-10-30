"""Offline security contracts for the disposable managed packaging proof."""
import copy
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import mock_open, patch

import marmish_managed_runtime_spike as spike


class ManagedRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.fixture = json.loads(spike.FIXTURE.read_text())
        self.row = self.fixture['targets']['x86_64-unknown-linux-gnu']

    def test_all_asset_pins_and_metadata_are_complete(self):
        self.assertEqual(len(self.fixture['targets']), 3)
        for row in self.fixture['targets'].values():
            for kind in spike.CAPS:
                spike.checked_url(row[kind]['url'])
                self.assertRegex(row[kind]['sha256'], '^[0-9a-f]{64}$')
                self.assertGreater(row[kind]['size'], 0)
                self.assertLessEqual(row[kind]['size'], spike.CAPS[kind])
            self.assertEqual(row['python_entry']['sha256'], row['python']['sha256'])
            self.assertEqual(row['python_entry']['url'], row['python']['url'])
            self.assertEqual([row['python_entry'][k] for k in ('major','minor','patch')], [3,13,13])
            self.assertIsNone(row['python_entry']['variant'])
            self.assertEqual(row['python_entry']['prerelease'], '')

    def test_url_refuses_wrong_scheme_host_credentials_port_and_fragment(self):
        for url in ['http://github.com/a', 'https://github.com.evil/a', 'https://evil/a',
                    'https://user@github.com/a', 'https://github.com:80/a',
                    'https://github.com/a#x', 'https://github.com/a\nb']:
            with self.subTest(url=url), self.assertRaises(spike.Refused):
                spike.checked_url(url)

    def test_environment_cannot_inherit_hostile_configuration(self):
        with patch.dict(os.environ, {'UV_PYTHON_CACHE_DIR':'/evil', 'PYTHONPATH':'/evil',
                                     'LD_PRELOAD':'/evil', 'PIP_INDEX_URL':'https://evil'}):
            env = spike.environment(self.root)
        self.assertEqual(set(env), {'PATH','LANG','LC_ALL','HOME','XDG_CACHE_HOME',
                                    'XDG_CONFIG_HOME','XDG_DATA_HOME','TMPDIR'})
        for key in ('HOME','XDG_CACHE_HOME','XDG_CONFIG_HOME','XDG_DATA_HOME','TMPDIR'):
            self.assertTrue(Path(env[key]).is_relative_to(self.root))

    def test_metadata_changes_only_url_and_requires_hash(self):
        output = self.root/'downloads.json'
        archive = self.root/'python.tar.gz'
        spike.local_metadata(self.row, archive, output)
        actual=json.loads(output.read_text())
        self.assertEqual(list(actual), [self.row['metadata_key']])
        entry=actual[self.row['metadata_key']]
        self.assertEqual(entry.pop('url'),archive.as_uri())
        original=copy.deepcopy(self.row['python_entry']);original.pop('url')
        self.assertEqual(entry,original)
        for bad in (None,'','0'*63,'g'*64):
            row=copy.deepcopy(self.row);row['python_entry']['sha256']=bad
            with self.subTest(bad=bad),self.assertRaisesRegex(spike.Refused,'metadata_hash_required'):
                spike.local_metadata(row,archive,output)

    def test_existing_interpreter_slot_refused_before_uv(self):
        (self.root/'python').mkdir()
        with patch.object(spike,'run') as run, self.assertRaisesRegex(spike.Refused,'runtime_slot_exists'):
            spike.install_python('uv',self.row,self.root/'archive',self.root,self.root/'metadata',{})
        run.assert_not_called()

    def archive(self, name, member_type=tarfile.REGTYPE):
        path=self.root/'archive.tar.gz'
        with tarfile.open(path,'w:gz') as tar:
            info=tarfile.TarInfo(name);info.type=member_type
            if member_type==tarfile.REGTYPE:
                info.size=3;tar.addfile(info,io.BytesIO(b'abc'))
            else:
                info.linkname='/etc/passwd';tar.addfile(info)
        return path

    def test_uv_extraction_rejects_paths_links_and_unexpected_member(self):
        for name,kind in [('../uv',tarfile.REGTYPE),('uv-x/uv',tarfile.SYMTYPE),
                          ('uv-x/uv',tarfile.LNKTYPE),('uv-x/other',tarfile.REGTYPE)]:
            with self.subTest(name=name,kind=kind):
                path=self.archive(name,kind)
                with self.assertRaises(spike.Refused):
                    spike.extract_uv(path,'uv-x/uv',self.root/'uv')
                path.unlink()
        self.assertFalse((self.root/'uv').exists())

    def test_uv_extraction_copies_only_expected_regular_member(self):
        path=self.archive('uv-x/uv')
        spike.extract_uv(path,'uv-x/uv',self.root/'uv')
        self.assertEqual((self.root/'uv').read_bytes(),b'abc')

    def test_asset_rejects_wrong_size_digest_and_missing_digest(self):
        path=self.root/'asset';path.write_bytes(b'abc')
        (self.root/'env').mkdir()
        env=spike.environment(self.root/'env')
        for record,category in [({'size':4,'sha256':'0'*64},'asset_size_mismatch'),
                                ({'size':3,'sha256':None},'asset_hash_required'),
                                ({'size':3,'sha256':'0'*64},'asset_hash_mismatch')]:
            with self.subTest(category=category), self.assertRaisesRegex(spike.Refused,category):
                spike.verify_asset(path,record,'uv',env)

    def test_embedded_verifier_byte_exact(self):
        output=self.root/'verifier.py'
        spike.embedded_verifier(output)
        body=(spike.REPO/'scripts/install.sh').read_bytes().split(b'# BEGIN MARMISH INSTALLER PYTHON\n')[1].split(b'# END MARMISH INSTALLER PYTHON')[0]
        self.assertEqual(output.read_bytes(),body)

    def test_launcher_substitution_preserves_template(self):
        from marmish_release import launcher
        interpreter=Path('/private/some prefix/python3.13')
        directory=spike.make_launchers(self.root,interpreter)
        actual=(directory/'marmish').read_text()
        self.assertEqual(actual.replace("exec '/private/some prefix/python3.13' ",'exec python3 '),launcher('client_main'))
        self.assertIn('-I -B',actual)

    def test_report_refuses_root_and_is_private_exclusive(self):
        path=self.root/'report.json'
        with patch.object(spike.os,'geteuid',return_value=0):
            self.assertEqual(spike.main(['--report',str(path)]),1)
        report=json.loads(path.read_text())
        self.assertEqual(report['failure'],'root_refused')
        self.assertEqual(path.stat().st_mode & 0o777,0o600)
        with patch.object(spike.os,'geteuid',return_value=0),self.assertRaises(FileExistsError):
            spike.main(['--report',str(path)])


    def test_download_validates_each_redirect_and_preserves_caps(self):
        from types import SimpleNamespace
        env=spike.environment(self.root)
        responses=[('302', 'https://release-assets.githubusercontent.com/a'),('200','')]
        seen=[]
        def fake(command, **options):
            seen.append(command)
            code,url=responses.pop(0)
            Path(command[command.index('--output')+1]).write_bytes(b'abc')
            options['stdout'].write((code+'\n'+url+'\n').encode())
            self.assertTrue(callable(options['preexec_fn']))
            self.assertEqual(options['timeout'],190)
            return SimpleNamespace(returncode=0)
        with patch.object(spike.subprocess,'run',side_effect=fake):
            spike.download('https://github.com/a',self.root/'result',16,env)
        self.assertEqual((self.root/'result').read_bytes(),b'abc')
        self.assertEqual(len(seen),2)
        for command in seen:
            self.assertEqual(command[0],'/usr/bin/curl')
            self.assertNotIn('-L',command)
            self.assertNotIn('--location',command)
            self.assertEqual(command[command.index('--max-filesize')+1],'16')

    def test_download_rejects_external_redirect_and_oversize(self):
        from types import SimpleNamespace
        env=spike.environment(self.root)
        for body,response,category in [(b'a',b'302\nhttps://evil/a\n','download_url_refused'),
                                       (b'12345',b'200\n\n','download_size_limit')]:
            def fake(command,**options):
                Path(command[command.index('--output')+1]).write_bytes(body)
                options['stdout'].write(response)
                return SimpleNamespace(returncode=0)
            with self.subTest(category=category),patch.object(spike.subprocess,'run',side_effect=fake):
                with self.assertRaisesRegex(spike.Refused,category):
                    spike.download('https://github.com/a',self.root/'result',4,env)
            self.assertFalse((self.root/'result').exists())
            self.assertEqual(list(self.root.glob('fetch-*')),[])

    def test_native_architecture_checked_before_execution(self):
        binary=self.root/'uv'
        header=bytearray(32);header[:6]=b'\x7fELF\x02\x01';header[18:20]=(62).to_bytes(2,'little')
        binary.write_bytes(header)
        spike.verify_architecture(binary,'x86_64-unknown-linux-gnu')
        with self.assertRaisesRegex(spike.Refused,'native_architecture_mismatch'):
            spike.verify_architecture(binary,'aarch64-unknown-linux-gnu')

    def test_process_timeout_and_output_limits_reap_owned_child(self):
        env=spike.environment(self.root)
        with self.assertRaisesRegex(spike.Refused,'child_timeout'):
            spike.run(['/bin/sleep','10'],env,timeout=0.01)
        with self.assertRaisesRegex(spike.Refused,'child_output_limit'):
            spike.run(['/usr/bin/yes'],env,timeout=5)


    def test_minor_directory_link_and_full_version_resolve_to_single_interpreter(self):
        root=self.root/'python';binary=root/'cpython-full/bin/python3.13'
        binary.parent.mkdir(parents=True);binary.write_bytes(b'fixture')
        (root/'cpython-minor').symlink_to('cpython-full',target_is_directory=True)
        self.assertEqual(spike.discover_interpreter(self.root),binary)
        (root/'escape').symlink_to('/etc',target_is_directory=True)
        with self.assertRaisesRegex(spike.Refused,'runtime_link_escape'):
            spike.discover_interpreter(self.root)

    def test_home_snapshot_uses_metadata_without_reading_user_contents(self):
        directory=self.root/'.cache/uv';directory.mkdir(parents=True)
        file=directory/'opaque';file.write_bytes(b'user data')
        with patch.object(spike,'digest',side_effect=AssertionError('must not read content')):
            before=spike.home_snapshot(self.root)
            file.write_bytes(b'changed')
            after=spike.home_snapshot(self.root)
        self.assertNotEqual(before,after)


    def test_monitor_tolerates_only_disappearing_owned_staging_files(self):
        from types import SimpleNamespace
        missing=self.root/'removed'
        with patch.object(Path,'rglob',return_value=iter([missing])):
            self.assertEqual(spike.total_bytes(self.root),0)
        present=self.root/'present';present.write_bytes(b'abc')
        with patch.object(Path,'rglob',return_value=iter([present])):
            self.assertEqual(spike.total_bytes(self.root),3)
        with patch.object(Path,'rglob',return_value=iter([present])), \
                patch.object(Path,'lstat',side_effect=PermissionError):
            with self.assertRaises(PermissionError):spike.total_bytes(self.root)

    def test_suite_failure_report_contains_only_identifiers_and_optional_private_log(self):
        raw = (b'ERROR: test_contract (test_packaging.PackagingTests.test_contract)\n'
               b'Exception: disposable-secret-value\nRan 7 tests in 0.1s\n')
        report = {'assertions': []}
        private = self.root / 'private.log'
        with patch.object(spike, 'run', return_value=(1, raw)), \
                patch.object(spike, 'test_compiler_directory', return_value=self.root):
            with self.assertRaisesRegex(spike.Refused, 'full_tools_suite_failed'):
                spike.full_tools_acceptance(self.root / 'python', {}, self.root, report, private)
        self.assertNotIn('disposable-secret-value', json.dumps(report))
        self.assertEqual(report['full_tools_tests']['failed_tests'][0]['method'], 'test_contract')
        self.assertEqual(private.stat().st_mode & 0o777, 0o600)
        self.assertEqual(private.read_bytes(), raw)

    def test_compiler_resolves_rustup_without_inheriting_ambient_environment(self):
        shim = self.root / 'rustup'
        shim.write_bytes(b'fixture')
        compiler = self.root / 'toolchain/bin/rustc'
        compiler.parent.mkdir(parents=True)
        compiler.write_bytes(b'fixture')
        compiler.chmod(0o700)
        with patch.object(spike.shutil, 'which', return_value=str(shim)), \
                patch.object(spike, 'compiler_lookup', return_value=(0, str(compiler).encode())) as lookup:
            self.assertEqual(spike.test_compiler_directory(), compiler.parent)
        command, env = lookup.call_args.args
        self.assertEqual(command, shim)
        self.assertEqual(set(env), {'HOME', 'PATH', 'LANG', 'LC_ALL'})

    def test_direct_compiler_ignores_unrelated_rustup(self):
        compiler = self.root / 'toolchain/bin/rustc'
        compiler.parent.mkdir(parents=True)
        compiler.write_bytes(b'fixture')
        compiler.chmod(0o700)
        unrelated = self.root / 'rustup'
        unrelated.write_bytes(b'fixture')
        with patch.object(spike.shutil, 'which', side_effect=[str(compiler), str(unrelated)]), \
                patch.object(spike, 'compiler_lookup') as lookup:
            self.assertEqual(spike.test_compiler_directory(), compiler.parent)
        lookup.assert_not_called()

    def test_no_exposure_checks_entire_interrupted_case(self):
        slot = self.root / 'slot'
        slot.mkdir()
        staging = self.root / 'tmp/staging'
        staging.mkdir(parents=True)
        (staging / 'python3').write_bytes(b'partial')
        spike.require_no_exposure(slot, staging, self.root)
        (self.root / 'escaped/python3').parent.mkdir()
        (self.root / 'escaped/python3').write_bytes(b'partial')
        with self.assertRaisesRegex(spike.Refused, 'negative_interpreter_exposed'):
            spike.require_no_exposure(slot, staging, self.root)

    def test_no_exposure_covers_other_python_versions_and_tmpdir(self):
        slot = self.root / 'slot'
        slot.mkdir()
        temporary = self.root / 'tmp'
        temporary.mkdir()
        for name in ('python', 'python3', 'python3.14', 'python2.7', 'python3.13.13'):
            with self.subTest(name=name):
                path = temporary / name
                path.write_bytes(b'interpreter')
                with self.assertRaisesRegex(spike.Refused, 'negative_interpreter_exposed'):
                    spike.require_no_exposure(slot, scope=self.root)
                path.unlink()

    def test_retry_isolation_rejects_links_to_removed_case_and_external_environment(self):
        retry = self.root / 'retry'
        retry.mkdir()
        env = spike.environment(retry)
        fresh = retry / 'fresh'
        fresh.mkdir()
        previous = self.root / 'removed-case'
        spike.require_fresh_isolation(fresh, env, previous)
        link = fresh / 'stale-runtime'
        link.symlink_to(previous / 'python', target_is_directory=True)
        with self.assertRaisesRegex(spike.Refused, 'retry_slot_escape'):
            spike.require_fresh_isolation(fresh, env, previous)
        link.unlink()
        external = self.root / 'outside'
        external.mkdir()
        with self.assertRaisesRegex(spike.Refused, 'retry_environment_escape'):
            spike.require_fresh_isolation(fresh, dict(env, TMPDIR=str(external)), previous)

    def test_setup_class_failure_identifier_is_safely_reported(self):
        report = {'assertions': []}
        raw = b'ERROR: setUpClass (test_fixture.NativeTests)\nRan 2 tests in 0.1s\n'
        with patch.object(spike, 'run', return_value=(1, raw)), \
                patch.object(spike, 'test_compiler_directory', return_value=self.root):
            with self.assertRaises(spike.Refused):
                spike.full_tools_acceptance(self.root / 'python', {}, self.root, report)
        self.assertEqual(report['full_tools_tests']['failed_tests'],
                         [{'method': 'setUpClass', 'class': 'test_fixture.NativeTests'}])

    def test_uv_negative_requires_specific_failure_and_matching_digest(self):
        raw = (b'Caused by: Hash mismatch for `cpython` Expected: ' + b'0' * 64
               + b' Computed: ' + self.row['python']['sha256'].encode())
        spike.require_uv_negative_signature('bad-digest', raw, self.row, self.root)
        for wrong in (b'unrelated failure', raw.replace(b'Computed:', b'unknown')):
            with self.assertRaisesRegex(spike.Refused, 'uv_negative_wrong_failure'):
                spike.require_uv_negative_signature('bad-digest', wrong, self.row, self.root)
        missing = (b'failed to query metadata of file `' + str(self.root / 'absent.tar.gz').encode()
                   + b'`: No such file or directory (os error 2)')
        spike.require_uv_negative_signature('missing-local', missing, self.row, self.root)
        with self.assertRaises(spike.Refused):
            spike.require_uv_negative_signature('missing-local', raw, self.row, self.root)

    @unittest.skipUnless(spike.platform.system() == 'Linux', 'Linux namespace prefix')
    def test_offline_namespace_preserves_identity_and_owns_asset_ancestors(self):
        env = spike.environment(self.root)
        with patch.object(spike, 'run', return_value=(0, b'')) as execute:
            prefix, category = spike.network_prefix(self.root / 'python', env)
        self.assertEqual(category, 'network_namespace_enforced')
        self.assertEqual(prefix[prefix.index('--uid') + 1], str(os.geteuid()))
        self.assertEqual(prefix[prefix.index('--gid') + 1], str(os.getegid()))
        self.assertIn('--tmpfs', prefix)
        self.assertNotIn(str(Path.home()), prefix)
        self.assertNotIn(str(spike.REPO), prefix)
        command = execute.call_args.args[0]
        self.assertIn('os.stat("/").st_uid', command[command.index('-c') + 1])
        with patch.object(spike, 'run', side_effect=FileNotFoundError):
            self.assertEqual(spike.network_prefix(self.root / 'python', env)[1],
                             'network_namespace_unavailable')

    def test_exited_leader_still_triggers_owned_group_cleanup(self):
        env = spike.environment(self.root)
        real_kill = os.killpg
        with patch.object(spike.os, 'killpg', wraps=real_kill) as cleanup:
            status, _ = spike.run(['/bin/sh', '-c', 'exit 0'], env)
        self.assertEqual(status, 0)
        cleanup.assert_called_once()
        self.assertEqual(cleanup.call_args.args[1], spike.signal.SIGKILL)

    def test_unexpected_exception_has_fixed_sanitized_report(self):
        report = self.root / 'report.json'
        with patch.object(spike, 'proof', side_effect=KeyError('secret-sentinel')), \
                patch.object(spike, 'home_snapshot', return_value={}):
            self.assertEqual(spike.main(['--report', str(report)]), 1)
        result = json.loads(report.read_text())
        self.assertEqual(result['failure'], 'unexpected_local_failure')
        self.assertEqual(result['error_type'], 'KeyError')
        self.assertNotIn('secret-sentinel', report.read_text())
        self.assertEqual(result['cleanup'], 'complete')

    def test_frozen_pins_match_recorded_official_checksum_crosscheck(self):
        evidence = json.loads((spike.REPO / 'docs/validation/managed-runtime-checksums.json').read_text())
        actual = {(row['target'], row['kind']): row for row in evidence['artifacts']}
        expected = {(target, kind) for target in self.fixture['targets'] for kind in spike.CAPS}
        self.assertEqual(set(actual), expected)
        self.assertEqual(len(evidence['artifacts']), 9)
        for target, kind in expected:
            record = actual[(target, kind)]
            frozen = self.fixture['targets'][target][kind]
            self.assertTrue(record['checksum_matches'])
            self.assertEqual(record['sha256'], frozen['sha256'])
            filename = spike.urllib.parse.unquote(spike.urllib.parse.urlsplit(frozen['url']).path.rsplit('/', 1)[1])
            self.assertEqual(record['filename'], filename)
        for name, frozen in self.fixture['checksum_sources'].items():
            recorded = dict(evidence['sources'][name])
            self.assertTrue(recorded.pop('verified'))
            self.assertEqual(recorded, frozen)

    def native_slots(self):
        slots = []
        for index in (1, 2):
            app = self.root / ('app' + str(index))
            (app / 'libexec/marmish').mkdir(parents=True)
            patch_file = app / 'patches/mdk/marmish-restricted.patch'
            patch_file.parent.mkdir(parents=True)
            patch_file.write_bytes(b'verified-fixture-patch')
            manifest = {'patch_sha256': spike.digest(patch_file), 'restricted_protocol': 1}
            (app / 'libexec/marmish/wnd.manifest.json').write_text(json.dumps(manifest))
            slots.append((self.root, self.root / 'python', app))
        return slots

    def test_darwin_native_contract_exact_marker_and_both_slots(self):
        slots = self.native_slots()
        marker = b'marmish_restricted_start_failed\n'
        for code, output, accepted in [(1, marker, True), (0, marker, False),
                                      (1, b'wrong', False), (1, marker + b' ', False),
                                      (-6, marker, False)]:
            report = {}
            def invoke(command, *args, **kwargs):
                if command[-1] == '--doctor': return 0, b'{"ok":true}'
                if Path(command[0]).name == 'wnd': return code, output
                return 0, b''
            with self.subTest(code=code, output=output), \
                    patch.object(spike.platform, 'system', return_value='Darwin'), \
                    patch.object(spike, 'run', side_effect=invoke):
                if accepted:
                    spike.installed_acceptance(slots, self.root, {}, {}, report)
                    self.assertEqual(report['native_probe_mode'], 'restricted_startup_guard_refusal')
                else:
                    with self.assertRaisesRegex(spike.Refused, 'native_probe_failed'):
                        spike.installed_acceptance(slots, self.root, {}, {}, report)
                self.assertEqual(len(report['native_probe_results']), 2)
                self.assertNotIn('wrong', json.dumps(report))
                self.assertEqual(report['native_probe_results'][0]['signal'], 6 if code == -6 else None)

    def test_native_missing_tool_and_timeout_fixed_private_diagnostics(self):
        for error, category in [(FileNotFoundError(), 'missing_tool'),
                                (spike.Refused('child_timeout'), 'child_timeout')]:
            with patch.object(spike, 'run', side_effect=error):
                row = spike.native_probe(self.root, {}, 1)
            self.assertEqual(row['category'], category)
            self.assertEqual(set(row), {'slot', 'exit_code', 'signal', 'marker_exact', 'output_bytes', 'category'})

    def test_namespace_ladder_uses_suite_environment_and_darwin_never_probes(self):
        env = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin'}
        report = {}
        with patch.object(spike.platform, 'system', return_value='Linux'), \
                patch.object(spike, 'bwrap_executable', return_value=Path('/verified/helper')) as selected, \
                patch('marmish_transport_sandbox.validated_bwrap', side_effect=AssertionError('host validator used')), \
                patch('builtins.open', mock_open(read_data=b'1\n')) as policies, \
                patch.object(spike, 'namespace_probe', return_value={'exit_code': 1}) as probes:
            spike.linux_namespace_preflight(env, report)
        selected.assert_called_once_with()
        self.assertEqual(policies.call_count, 3)
        self.assertEqual(probes.call_count, 3)
        for call in probes.call_args_list:
            self.assertIs(call.args[1], env)
        bwrap = probes.call_args_list[1].args[0]
        self.assertEqual(bwrap[0], '/verified/helper')
        for flag in ('--unshare-user', '--unshare-pid', '--uid', '--gid', '--cap-drop', '--proc', '--dev'):
            self.assertIn(flag, bwrap)
        nested = probes.call_args_list[2].args[0]
        self.assertIn('ip link set lo up', nested[nested.index('-c') + 1])
        self.assertIn('sysctl -q -w', nested[nested.index('-c') + 1])
        with patch.object(spike.platform, 'system', return_value='Darwin'), \
                patch.object(spike, 'namespace_probe') as probes:
            spike.linux_namespace_preflight(env, {})
            probes.assert_not_called()

    def test_namespace_failure_exports_only_fixed_denial_hint(self):
        with patch.object(spike, 'run', return_value=(1, b'private-secret Operation not permitted')):
            result = spike.namespace_probe(['/usr/bin/true'], {})
        self.assertEqual(result['denied_operation_hint'], 'operation_not_permitted')
        self.assertNotIn('private-secret', json.dumps(result))

    def test_linux_native_help_requires_zero_even_with_darwin_marker(self):
        slots = self.native_slots()
        for code, output in ((0, b'help'), (1, b'marmish_restricted_start_failed\n')):
            report = {}
            def invoke(command, *args, **kwargs):
                if command[-1] == '--doctor': return 0, b'{"ok":true}'
                if Path(command[0]).name == 'wnd': return code, output
                return 0, b''
            with patch.object(spike.platform, 'system', return_value='Linux'), \
                    patch.object(spike, 'run', side_effect=invoke):
                if code == 0:
                    spike.installed_acceptance(slots, self.root, {}, {}, report)
                    self.assertEqual(report['native_probe_mode'], 'native_help_and_manifest_provenance')
                else:
                    with self.assertRaisesRegex(spike.Refused, 'native_probe_failed'):
                        spike.installed_acceptance(slots, self.root, {}, {}, report)
                self.assertEqual(len(report['native_probe_results']), 2)

    def test_darwin_native_contract_rejects_patch_digest_and_protocol_mismatch(self):
        slots = self.native_slots()
        manifest_path = slots[0][2] / 'libexec/marmish/wnd.manifest.json'
        original = json.loads(manifest_path.read_text())
        for field, bad in (('patch_sha256', '0' * 64), ('restricted_protocol', 2),
                           ('restricted_protocol', True)):
            record = dict(original, **{field: bad})
            manifest_path.write_text(json.dumps(record))
            report = {}
            def invoke(command, *args, **kwargs):
                if command[-1] == '--doctor': return 0, b'{"ok":true}'
                if Path(command[0]).name == 'wnd': return 1, b'marmish_restricted_start_failed\n'
                return 0, b''
            with patch.object(spike.platform, 'system', return_value='Darwin'), \
                    patch.object(spike, 'run', side_effect=invoke):
                with self.assertRaisesRegex(spike.Refused, 'native_manifest_contract_mismatch'):
                    spike.installed_acceptance(slots, self.root, {}, {}, report)
            self.assertEqual(len(report['native_probe_results']), 2)
            self.assertNotIn('native_probe_mode', report)

    def test_native_manifest_malformed_nonobject_or_missing_is_false(self):
        app = self.native_slots()[0][2]
        manifest = app / 'libexec/marmish/wnd.manifest.json'
        with patch.object(spike.platform, 'system', return_value='Darwin'):
            for raw in (b'{broken', b'[]', b'null', b'"text"'):
                manifest.write_bytes(raw)
                self.assertFalse(spike.native_manifest_contract(app))
            manifest.unlink()
            self.assertFalse(spike.native_manifest_contract(app))

    def test_probe_permission_error_stays_private_and_fixed(self):
        with patch.object(spike, 'run', side_effect=PermissionError('private-exception')):
            native = spike.native_probe(self.root, {}, 1)
            namespace = spike.namespace_probe(['/usr/bin/true'], {})
        self.assertEqual(native['category'], 'spawn_failed')
        self.assertEqual(namespace['category'], 'spawn_failed')
        self.assertNotIn('private-exception', json.dumps([native, namespace]))

    def test_darwin_native_timeout_outputlimit_and_permission_collect_both_slots(self):
        slots = self.native_slots()
        for category in ('child_timeout', 'child_output_limit', 'spawn_failed'):
            report = {}
            def invoke(command, *args, **kwargs):
                if command[-1] == '--doctor': return 0, b'{"ok":true}'
                if Path(command[0]).name == 'wnd':
                    if category == 'spawn_failed': raise PermissionError('private-secret')
                    raise spike.Refused(category)
                return 0, b''
            with patch.object(spike.platform, 'system', return_value='Darwin'), \
                    patch.object(spike, 'run', side_effect=invoke):
                with self.assertRaisesRegex(spike.Refused, 'native_probe_failed'):
                    spike.installed_acceptance(slots, self.root, {}, {}, report)
            self.assertEqual(len(report['native_probe_results']), 2)
            self.assertTrue(all(row['category'] == category for row in report['native_probe_results']))
            self.assertNotIn('private-secret', json.dumps(report))

    def test_owned_cwd_refuses_relative_symlink_and_outside_before_spawn(self):
        env = spike.environment(self.root)
        link = self.root / 'link'
        link.symlink_to(Path(env['HOME']), target_is_directory=True)
        for cwd in (Path('relative'), link, Path('/')):
            with self.subTest(cwd=cwd), patch.object(spike.subprocess, 'Popen') as spawn:
                with self.assertRaisesRegex(spike.Refused, 'child_cwd_refused'):
                    spike.run(['/usr/bin/true'], env, cwd=cwd)
                spawn.assert_not_called()

    def test_observer_kills_only_live_child_and_requires_sigkill(self):
        import sys
        env = spike.environment(self.root)
        slot = self.root / 'slot'
        slot.mkdir()
        marker = Path(env['TMPDIR']) / 'unpacked/lib/test.py'
        code = ('import pathlib,time; p=pathlib.Path(' + repr(str(marker))
                + ');p.parent.mkdir(parents=True);p.write_text("fixture");time.sleep(30)')
        observer = spike.ExtractionObserver(slot, Path(env['TMPDIR']))
        status, _ = spike.run([sys.executable, '-I', '-c', code], env, monitor=self.root,
                              observer=observer, timeout=5)
        self.assertEqual(status, -spike.signal.SIGKILL)
        self.assertTrue(observer.observed)
        self.assertEqual(observer.staging, marker.parents[1])

    def test_early_completion_or_natural_error_does_not_count_as_interruption(self):
        env = spike.environment(self.root)
        for code in ('0', '7'):
            observer = spike.ExtractionObserver(self.root / 'slot', Path(env['TMPDIR']))
            with self.subTest(code=code), self.assertRaisesRegex(spike.Refused, 'finished_before_interruption'):
                spike.run(['/bin/sh', '-c', 'exit ' + code], env, monitor=self.root, observer=observer)

    def test_observer_exception_fails_closed_and_cleans_group(self):
        env = spike.environment(self.root)
        marker = self.root / 'observed.py'
        marker.write_text('fixture')
        class Broken:
            observed = False
            def __call__(self, path, info): raise ValueError('private-observer-error')
        with patch.object(spike.os, 'killpg', wraps=os.killpg) as cleanup:
            with self.assertRaisesRegex(spike.Refused, 'interruption_observer_failed'):
                spike.run(['/bin/sleep', '30'], env, monitor=self.root, observer=Broken())
        self.assertTrue(cleanup.called)

    def test_surviving_group_is_detected_and_walk_is_finite(self):
        with patch.object(spike.os, 'killpg'), patch.object(spike.time, 'monotonic', side_effect=[0, 1, 3]), \
                patch.object(spike.time, 'sleep'):
            with self.assertRaisesRegex(spike.Refused, 'interruption_group_survived'):
                spike.require_group_gone(123)
        file = self.root / 'file'
        file.write_bytes(b'x')
        with patch.object(Path, 'rglob', return_value=iter([file] * 50001)):
            with self.assertRaisesRegex(spike.Refused, 'runtime_entry_count_limit'):
                spike.total_bytes(self.root)

    def test_gzip_substitution_changes_only_header_and_equal_bounded_stream(self):
        import gzip
        original, replacement = self.root / 'original.gz', self.root / 'replacement.gz'
        original.write_bytes(gzip.compress(b'valid distribution fixture', mtime=0))
        computed = spike.substituted_gzip(original, replacement)
        first, second = original.read_bytes(), replacement.read_bytes()
        self.assertEqual(first[:4] + first[8:], second[:4] + second[8:])
        self.assertNotEqual(spike.digest(original), computed)
        self.assertEqual(gzip.decompress(first), gzip.decompress(second))
        self.assertEqual(replacement.stat().st_mode & 0o777, 0o400)
        invalid = self.root / 'invalid.gz'
        invalid.write_bytes(b'not gzip')
        with self.assertRaisesRegex(spike.Refused, 'substitute_invalid_gzip'):
            spike.substituted_gzip(invalid, self.root / 'invalid-copy.gz')

    def test_substitution_requires_exact_expected_and_computed_digests(self):
        raw = b'Caused by: Hash mismatch for `python` Expected: ' + b'1' * 64 + b' Computed: ' + b'2' * 64
        spike.require_uv_hash_mismatch(raw, '1' * 64, '2' * 64)
        for expected, computed in [('3' * 64, '2' * 64), ('1' * 64, '4' * 64)]:
            with self.assertRaisesRegex(spike.Refused, 'uv_negative_wrong_failure'):
                spike.require_uv_hash_mismatch(raw, expected, computed)

    def test_substitution_uncalled_hook_or_acceptance_fails(self):
        archive = self.root / 'original.tar.gz'
        archive.write_bytes(b'fixture')
        raw = (b'Caused by: Hash mismatch for `python` Expected: '
               + self.row['python']['sha256'].encode() + b' Computed: ' + b'1' * 64)
        for index, (effect, category) in enumerate([(spike.UvInstallFailure(raw), 'substitution_callback_not_called_once'),
                                                    (None, 'negative_accepted')]):
            root = self.root / str(index)
            root.mkdir()
            with patch.object(spike, 'substituted_gzip', return_value='1' * 64), \
                    patch.object(spike, 'install_python', side_effect=effect):
                with self.assertRaisesRegex(spike.Refused, category):
                    spike.substitution_acceptance(self.root / 'uv', self.row, archive, root, {})

    def test_after_rehash_hook_runs_once_before_uv_and_private_failure(self):
        archive = self.root / 'python.tar.gz'
        archive.write_bytes(b'fixture')
        env = spike.environment(self.root)
        slot = self.root / 'slot'
        slot.mkdir()
        events = []
        with patch.object(spike, 'verify_asset', side_effect=lambda *args: events.append('verify')), \
                patch.object(spike, 'run', return_value=(1, b'private failure')):
            with self.assertRaises(spike.UvInstallFailure) as failure:
                spike.install_python(self.root / 'uv', self.row, archive, slot, self.root / 'meta.json', env,
                                     after_rehash=lambda: events.append('hook'))
        self.assertEqual(events, ['verify', 'hook'])
        self.assertEqual(failure.exception.output, b'private failure')
        self.assertEqual(str(failure.exception), 'uv_install_failed')

    def test_hostile_ambient_restores_environment_and_cwd_on_success_and_error(self):
        before = dict(os.environ)
        original = Path.cwd()
        for index, fail in enumerate((False, True)):
            root = self.root / str(index)
            root.mkdir()
            env = spike.environment(root)
            try:
                with spike.hostile_ambient(root, env) as cwd:
                    self.assertEqual(Path.cwd(), cwd)
                    self.assertEqual(os.environ['UV_CACHE_DIR'], str(root / 'hostile-case/unused-cache'))
                    self.assertEqual(env['HOME'], str(root / 'home'))
                    if fail: raise ValueError('intentional')
            except ValueError:
                self.assertTrue(fail)
            self.assertEqual(dict(os.environ), before)
            self.assertEqual(Path.cwd(), original)

    def test_restoration_failure_is_not_accepted(self):
        env = spike.environment(self.root)
        descriptor = os.open('.', os.O_RDONLY | os.O_DIRECTORY)
        restore = os.fchdir
        try:
            with patch.object(spike.os, 'fchdir', side_effect=OSError('private-error')):
                with self.assertRaisesRegex(spike.Refused, 'ambient_restore_failed'):
                    with spike.hostile_ambient(self.root, env): pass
        finally:
            restore(descriptor)
            os.close(descriptor)

    def test_relative_report_is_resolved_before_caller_cwd_changes(self):
        descriptor = os.open('.', os.O_RDONLY | os.O_DIRECTORY)
        elsewhere = self.root / 'elsewhere'
        elsewhere.mkdir()
        try:
            os.chdir(self.root)
            with patch.object(spike, 'proof', side_effect=lambda *args: os.chdir(elsewhere)), \
                    patch.object(spike, 'home_snapshot', return_value={}):
                self.assertEqual(spike.main(['--report', 'report.json']), 0)
            self.assertTrue((self.root / 'report.json').is_file())
            self.assertFalse((elsewhere / 'report.json').exists())
        finally:
            os.fchdir(descriptor)
            os.close(descriptor)


class SelfHostedRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()

    def test_ci_context_exact_grammar_and_native_target(self):
        valid = 'a' * 40 + ':123:2:linux-x64'
        result = spike.ci_context(valid, 'x86_64-unknown-linux-gnu')
        self.assertEqual(result['workflow'], 'native-validation')
        self.assertEqual(result['runner_class'], ['self-hosted', 'Linux', 'X64', 'bel-ci'])
        for raw in (valid + ':extra', valid.upper(), valid.replace(':123:', ':0123:'),
                    valid.replace(':2:', ':0:'), valid.replace(':123:', ':' + '1'*21 + ':'),
                    valid.replace(':2:', ':1234567:'), valid + ' ', valid.replace('linux-x64', 'macos-arm64')):
            with self.subTest(raw=raw), self.assertRaisesRegex(spike.Refused, 'ci_context_invalid'):
                spike.ci_context(raw, 'x86_64-unknown-linux-gnu')

    def test_selection_fixed_categories_and_unknown_redaction(self):
        from marmish_transport_sandbox import SandboxError
        for category in (*spike.BWRAP_CATEGORIES, 'private-path-secret'):
            with self.subTest(category=category), \
                    patch('marmish_transport_sandbox.validated_bwrap', side_effect=SandboxError(category)):
                expected = category if category in spike.BWRAP_CATEGORIES else 'unknown'
                with self.assertRaisesRegex(spike.Refused, '^bwrap_selection:' + expected + '$'):
                    spike.bwrap_executable()

    def test_required_helper_and_darwin_refuse_configuration(self):
        with patch.object(spike.platform, 'system', return_value='Linux'), \
                patch.object(spike, 'verified_helper_pair', return_value={}), \
                patch.object(spike, 'bwrap_executable', return_value=Path('/usr/bin/bwrap')):
            with self.assertRaisesRegex(spike.Refused, 'bwrap_helper_required'):
                spike.initialize_helper(True, {})
            report = {}
            spike.initialize_helper(False, report)
            self.assertEqual(report, {'bwrap_source': 'system'})
        with patch.object(spike.platform, 'system', return_value='Darwin'), patch.dict(os.environ, {}, clear=True):
            spike.initialize_helper(False, {})
            with self.assertRaisesRegex(spike.Refused, 'bwrap_configuration_on_darwin'):
                spike.initialize_helper(True, {})
            with patch.dict(os.environ, {'MARMISH_BWRAP_UNRECOGNIZED': 'sentinel'}):
                with self.assertRaisesRegex(spike.Refused, 'bwrap_configuration_on_darwin'):
                    spike.initialize_helper(False, {})

    def selected_fixture(self):
        binary = self.root / 'helper/bin/bwrap'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'not executed')
        binary.chmod(0o755)
        pair = {'MARMISH_BWRAP': str(binary), 'MARMISH_BWRAP_SHA256': spike.digest(binary)}
        provenance = {'suite': 'noble', 'architecture': 'amd64', 'package_version': '0.9.0-1ubuntu0.3',
                      'keyring_sha256': 'a'*64, 'package_sha256': 'b'*64,
                      'executable_sha256': pair['MARMISH_BWRAP_SHA256'], 'version': 'bubblewrap 0.9.0',
                      'private_path': 'must not export'}
        path = binary.parent.parent / 'provenance.json'
        path.write_text(json.dumps(provenance))
        return binary, pair, provenance, path

    def test_helper_provenance_is_bounded_digest_bound_and_allowlisted(self):
        binary, pair, original, path = self.selected_fixture()
        with patch.object(spike.platform, 'machine', return_value='x86_64'), patch.dict(os.environ, pair):
            result = spike.helper_provenance(pair)
            self.assertNotIn('private_path', result)
            for key, value in [('executable_sha256', '0'*64), ('architecture', 'arm64'),
                               ('version', 'private raw output'), ('suite', ['noble']),
                               ('package_version', 7)]:
                invalid = dict(original); invalid[key] = value
                path.write_text(json.dumps(invalid))
                with self.subTest(key=key), self.assertRaisesRegex(spike.Refused, 'bwrap_provenance_invalid'):
                    spike.helper_provenance(pair)
            path.write_bytes(b'x' * 65537)
            with self.assertRaisesRegex(spike.Refused, 'bwrap_provenance_invalid'):
                spike.helper_provenance(pair)
            path.unlink(); path.symlink_to(self.root / 'missing')
            with self.assertRaisesRegex(spike.Refused, 'bwrap_provenance_invalid'):
                spike.helper_provenance(pair)

    def test_actual_helper_validation_rejects_half_pair_mutation_permissions_and_symlink(self):
        binary, pair, _, _ = self.selected_fixture()
        with patch.dict(os.environ, pair):
            self.assertEqual(spike.bwrap_executable(), binary)
            binary.write_bytes(b'altered')
            with self.assertRaisesRegex(spike.Refused, 'bwrap_selection:bwrap_hash_mismatch'):
                spike.bwrap_executable()
            binary.chmod(0o777)
            with self.assertRaisesRegex(spike.Refused, 'bwrap_selection:unsafe_bwrap_executable'):
                spike.bwrap_executable()
            binary.chmod(0o755)
            binary.rename(binary.with_name('actual')); binary.symlink_to('actual')
            with self.assertRaisesRegex(spike.Refused, 'bwrap_selection:unsafe_bwrap_path'):
                spike.bwrap_executable()
        with patch.dict(os.environ, {'MARMISH_BWRAP': str(binary)}, clear=True):
            with self.assertRaisesRegex(spike.Refused, 'bwrap_selection:invalid_bwrap_selection'):
                spike.verified_helper_pair()

    def test_pair_reaches_only_full_suite_and_never_probe_or_application_environment(self):
        pair = {'MARMISH_BWRAP': '/verified/helper', 'MARMISH_BWRAP_SHA256': 'a'*64}
        env = spike.environment(self.root)
        with patch.object(spike.platform, 'system', return_value='Linux'), \
                patch.object(spike, 'verified_helper_pair', return_value=pair), \
                patch.object(spike, 'test_compiler_directory', return_value=self.root), \
                patch.object(spike, 'linux_namespace_preflight') as probe, \
                patch.object(spike, 'run', return_value=(0, b'Ran 1 test in 0.1s')) as run:
            spike.full_tools_acceptance(self.root / 'python', env, self.root, {'assertions': []})
        suite_env = run.call_args.args[1]
        self.assertEqual({key:suite_env[key] for key in pair}, pair)
        self.assertFalse(set(pair) & probe.call_args.args[0].keys())
        self.assertFalse(set(pair) & env.keys())
        self.assertNotIn('EPIPHYTIC_EXECUTION_ID', suite_env)

    def test_diagnostic_and_offline_builders_use_verified_absolute_executable_without_pair(self):
        env = spike.environment(self.root)
        report = {}
        with patch.object(spike.platform, 'system', return_value='Linux'), \
                patch.object(spike, 'bwrap_executable', return_value=Path('/verified/helper')), \
                patch('marmish_transport_sandbox.validated_bwrap', side_effect=AssertionError('host validator used')), \
                patch('builtins.open', mock_open(read_data=b'1\n')) as policies, \
                patch.object(spike.os, 'readlink', return_value='net:[fixture]') as namespace, \
                patch.object(spike, 'run', return_value=(0, b'')) as run:
            prefix, mode = spike.network_prefix(self.root / 'python', env)
            self.assertEqual(prefix[0], '/verified/helper')
            self.assertEqual(mode, 'network_namespace_enforced')
            spike.linux_namespace_preflight(env, report)
        namespace.assert_called_once_with('/proc/self/ns/net')
        self.assertEqual(policies.call_count, 3)
        for call in run.call_args_list:
            self.assertFalse({'MARMISH_BWRAP','MARMISH_BWRAP_SHA256'} & call.args[1].keys())
        self.assertEqual(run.call_args_list[-2].args[0][0], '/verified/helper')
        with patch.object(spike.platform, 'system', return_value='Linux'), \
                patch('builtins.open', mock_open(read_data=b'1\n')), \
                patch.object(spike.os, 'readlink', side_effect=AssertionError('unexpected host namespace read')), \
                patch.object(spike, 'bwrap_executable', side_effect=spike.Refused('bwrap_selection:unsafe_bwrap_path')):
            with self.assertRaisesRegex(spike.Refused, 'bwrap_selection:unsafe_bwrap_path'):
                spike.network_prefix(self.root / 'python', env)
            with self.assertRaises(spike.Refused):
                spike.linux_namespace_preflight(env, report)
            self.assertEqual(report['linux_namespace_preflight']['selection_failure'], 'bwrap_selection:unsafe_bwrap_path')

    def test_real_interruptions_reap_child_cleanup_and_emit_report(self):
        import signal
        import subprocess
        import sys
        import time
        for mode in ('term', 'keyboard', 'deadline'):
            with self.subTest(mode=mode):
                marker = self.root / (mode + '.pid')
                report = self.root / (mode + '.json')
                code = ('import sys,os;sys.path.insert(0,' + repr(str(spike.REPO/'tools')) + ');'
                        'import marmish_managed_runtime_spike as s;'
                        's.home_snapshot=lambda p:{};s.initialize_helper=lambda *a:None;'
                        's.proof=lambda root,*a:s.run(["/bin/sh","-c",'
                        '"echo $$ > "+' + repr(str(marker)) + '+"; exec /bin/sleep 30"],s.environment(root));'
                        'raise SystemExit(s.main(["--report",' + repr(str(report)) + ',"--total-timeout","1" if '
                        + repr(mode) + '=="deadline" else "10"]))')
                child = subprocess.Popen([sys.executable, '-I', '-B', '-c', code],
                                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                try:
                    deadline = time.monotonic() + 5
                    while not marker.exists() and child.poll() is None and time.monotonic() < deadline:
                        time.sleep(0.01)
                    self.assertTrue(marker.exists())
                    owned_pid = int(marker.read_text())
                    if mode != 'deadline':
                        child.send_signal(signal.SIGTERM if mode == 'term' else signal.SIGINT)
                    self.assertEqual(child.wait(timeout=7), 1)
                    result = json.loads(report.read_text())
                    self.assertEqual(result['failure'], 'total_timeout' if mode == 'deadline' else 'interrupted')
                    self.assertEqual(result['cleanup'], 'complete')
                    self.assertFalse(result['ok'])
                    with self.assertRaises(ProcessLookupError):
                        os.kill(owned_pid, 0)
                finally:
                    if child.poll() is None: child.kill()
                    child.wait()

    def test_main_ci_context_and_helper_refusals_emit_exclusive_reports_before_proof(self):
        for arguments, category in [(['--ci-context', 'private-invalid-context'], 'ci_context_invalid'),
                                    (['--require-bwrap-helper'], 'bwrap_helper_required')]:
            report = self.root / (category + '.json')
            with patch.object(spike.platform, 'system', return_value='Linux'), \
                    patch.object(spike.platform, 'machine', return_value='x86_64'), \
                    patch.object(spike, 'verified_helper_pair', return_value={}), \
                    patch.object(spike, 'proof') as proof:
                self.assertEqual(spike.main(['--report', str(report), *arguments]), 1)
            proof.assert_not_called()
            result = json.loads(report.read_text())
            self.assertEqual(result['failure'], category)
            self.assertNotIn('private-invalid-context', report.read_text())
            self.assertEqual(report.stat().st_mode & 0o777, 0o600)

    def test_host_facts_keep_only_bounded_distribution_identifiers(self):
        with patch.object(spike.platform, 'system', return_value='Linux'), \
                patch.object(spike.platform, 'machine', return_value='x86_64'), \
                patch.object(spike.platform, 'freedesktop_os_release', return_value={
                    'ID':'ubuntu', 'VERSION_ID':'26.04', 'PRETTY_NAME':'private host label'}):
            result = spike.host_facts()
        self.assertEqual(result['distribution'], {'id':'ubuntu', 'version_id':'26.04'})
        self.assertNotIn('private', json.dumps(result))

    def test_handlers_and_existing_timer_are_restored(self):
        import signal
        old = {number:signal.getsignal(number) for number in (signal.SIGALRM, signal.SIGTERM)}
        state = spike.install_interrupt_handlers(5)
        spike.restore_interrupt_handlers(state)
        self.assertEqual({number:signal.getsignal(number) for number in old}, old)
        self.assertEqual(signal.getitimer(signal.ITIMER_REAL), (0.0, 0.0))
        for timeout in (0, -1, 1321):
            with self.assertRaisesRegex(spike.Refused, 'total_timeout_invalid'):
                spike.install_interrupt_handlers(timeout)


if __name__=='__main__':
    unittest.main()
