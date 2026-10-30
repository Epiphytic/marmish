"""Offline security contracts for the disposable managed packaging proof."""
import copy
import io
import json
import os
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

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
                patch.object(spike, 'run', return_value=(0, str(compiler).encode())) as lookup:
            self.assertEqual(spike.test_compiler_directory(), compiler.parent)
        command, env = lookup.call_args.args
        self.assertEqual(command, [shim, 'which', 'rustc'])
        self.assertEqual(set(env), {'HOME', 'PATH', 'LANG', 'LC_ALL'})
        self.assertLessEqual(lookup.call_args.kwargs['timeout'], 10)

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


if __name__=='__main__':
    unittest.main()
