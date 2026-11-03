"""Offline fault contracts; these never claim real HTTPS install qualification."""
import argparse
import copy
import fcntl
import json
import os
from pathlib import Path
import tempfile
import time
import unittest
from unittest import mock

import marmish_native_https_check as check


def context():
    return {'schema': 1, 'qualification_run': '9', 'qualification_attempt': '1',
        'workflow_commit': 'b' * 40, 'candidate_run': '8', 'candidate_attempt': '2',
        'source_commit': 'a' * 40, 'bootstrap_commit': 'b' * 40,
        'bootstrap_sha256': 'c' * 64, 'version': '0.2.0',
        'manifest_pins': {target: 'd' * 64 for target in check.TARGETS}}


def report(value, target, source):
    result = {key: True for key in check.BOOL_KEYS}
    result.update(schema=1, kind='native_https_installer', context_sha256=check.sha(check.encoded(value)),
        qualification_run=value['qualification_run'], qualification_attempt=value['qualification_attempt'],
        source_commit=value['source_commit'], bootstrap_commit=value['bootstrap_commit'],
        bootstrap_sha256=value['bootstrap_sha256'], source=source, target=target, ok=True,
        stage='complete', failure=None, manifest_sha256=value['manifest_pins'][target], elapsed_ms=10)
    return result


class HttpsInstallerContracts(unittest.TestCase):
    def test_native_versions_only_and_bounded(self):
        for value in ('0.2.0', '0.2.8-rc.1', '0.2.18446744073709551615'):
            self.assertEqual(check.version_value(value), value)
        for value in ('0.1.9', 'latest', '0.2.01', '0.2.1-rc.0', '0.2.18446744073709551616',
                '0.2.1-rc.18446744073709551616', '0.2.' + '1' * 100):
            with self.subTest(value=value), self.assertRaises(check.Refused):
                check.version_value(value)

    def test_decimal_and_digest_no_shell_fragments(self):
        for value in ('0', '01', '-1', '1;echo', '18446744073709551616'):
            with self.subTest(value=value), self.assertRaises(check.Refused):
                check.decimal(value)
        for value in ('A' * 40, 'a' * 39, '$(secret)'):
            with self.subTest(value=value), self.assertRaises(check.Refused):
                check.hex_value(value, 40)

    def test_context_closed_and_canonical(self):
        self.assertEqual(check.validate_context(context()), context())
        for change in ({'extra': 1}, {'workflow_commit': 'e' * 40}, {'manifest_pins': {}},
                {'qualification_attempt': '0'}):
            value = context(); value.update(change)
            with self.subTest(change=change), self.assertRaises(check.Refused):
                check.validate_context(value)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'context'
            path.write_bytes(check.encoded(context()))
            self.assertEqual(check.load_context(path)[0], context())
            path.write_text(json.dumps(context(), indent=2))
            with self.assertRaisesRegex(check.Refused, 'context_encoding'):
                check.load_context(path)

    def test_duplicate_json_keys_rejected(self):
        with self.assertRaisesRegex(check.Refused, 'duplicate_json_key'):
            check.decode('{"schema":1,"schema":1}')

    def test_candidate_run_exact_attempt_head_branch_and_workflow(self):
        run = {'id': 8, 'run_attempt': 2, 'head_sha': 'a' * 40, 'head_branch': 'master',
            'event': 'workflow_dispatch', 'status': 'completed', 'conclusion': 'success',
            'path': '.github/workflows/v02-release.yml',
            'repository': {'full_name': 'Epiphytic/marmish'},
            'head_repository': {'full_name': 'Epiphytic/marmish'}}
        check.validate_run(run, context())
        for key, value in (('id', 9), ('run_attempt', 1), ('head_sha', 'b' * 40),
                ('head_branch', 'feature'), ('event', 'pull_request'), ('status', 'in_progress'),
                ('conclusion', 'failure'), ('path', 'other.yml'), ('repository', {}),
                ('head_repository', {})):
            invalid = copy.deepcopy(run); invalid[key] = value
            with self.subTest(key=key), self.assertRaisesRegex(check.Refused, 'candidate_run_binding'):
                check.validate_run(invalid, context())

    def test_resolve_pins_no_renames_and_rejects_extra_source_changes(self):
        with tempfile.TemporaryDirectory() as directory:
            value = context()
            options = argparse.Namespace(**{key: item for key, item in value.items()
                if key not in ('schema', 'manifest_pins')}, output=Path(directory) / 'context')
            for changed in (b'scripts/install.sh\n', b'crates/product.rs\nscripts/install.sh\n'):
                def git(arguments):
                    if arguments[0] == 'rev-parse':
                        return (value['bootstrap_commit'] + '\n').encode()
                    if arguments[0] == 'diff':
                        self.assertEqual(arguments, ['diff', '--no-renames', '--name-only',
                            value['source_commit'], value['bootstrap_commit'], '--'])
                        return changed
                    return b''
                artifact = dict(name='v02-pinned-bootstrap-8-2', expired=False, size_in_bytes=4000)
                with mock.patch.object(check, 'git', side_effect=git), \
                        mock.patch.object(check, 'validate_run'), \
                        mock.patch.object(check, 'api', return_value={'artifacts': [artifact]}), \
                        mock.patch.dict(os.environ, GH_TOKEN='offline-test-token'):
                    if changed == b'scripts/install.sh\n':
                        check.resolve(options)
                        self.assertTrue(options.output.exists())
                    else:
                        with self.assertRaisesRegex(check.Refused, 'bootstrap_changed_paths'):
                            check.resolve(options)

    def test_artifact_unique_unexpired_bounded(self):
        good = {'name': 'wanted', 'expired': False, 'size_in_bytes': 4000}
        check.validate_artifacts([good], 'wanted')
        for value in ([], [good, good], [dict(good, expired=True)],
                [dict(good, size_in_bytes=131073)], [dict(good, size_in_bytes=True)]):
            with self.subTest(value=value), self.assertRaises(check.Refused):
                check.validate_artifacts(value, 'wanted')

    def test_bootstrap_pin_render_is_exact_not_merely_regex_presence(self):
        template = (Path(__file__).parents[1] / 'scripts/install-native.sh').read_bytes()
        script = template
        for key, value in (('VERSION', '0.2.0'), ('SHA_AMD64', '1' * 64),
                ('SHA_ARM64', '2' * 64), ('SHA_MACOS', '3' * 64)):
            script = script.replace(('@MARMISH_' + key + '@').encode(), value.encode())
        self.assertEqual(check.pins_from_script(script, template, '0.2.0'),
            dict(zip(check.TARGETS, ('1' * 64, '2' * 64, '3' * 64))))
        for invalid in (script + b'# foreign\n', script.replace(b"version='0.2.0'", b"version='0.1.0'"),
                script + b"    sha_amd64='" + b'1' * 64 + b"'\n", template):
            with self.subTest(length=len(invalid)), self.assertRaises(check.Refused):
                check.pins_from_script(invalid, template, '0.2.0')

    def test_artifact_rejects_extra_and_symlink_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            context_path = root / 'context'
            context_path.write_bytes(check.encoded(context()))
            artifact = root / 'artifact'; artifact.mkdir()
            (artifact / 'extra').write_text('not installer')
            options = argparse.Namespace(context=context_path, artifact=artifact)
            with self.assertRaisesRegex(check.Refused, 'bootstrap_artifact_entries'):
                check.bind_context(options)
            (artifact / 'extra').unlink()
            (artifact / 'install.sh').symlink_to(context_path)
            with self.assertRaisesRegex(check.Refused, 'input_size_or_type'):
                check.bind_context(options)

    def test_original_budget_not_reset_and_reserves_cleanup(self):
        with mock.patch.object(check.time, 'monotonic', return_value=100):
            budget = check.Budget()
        with mock.patch.object(check.time, 'monotonic', return_value=200):
            self.assertEqual(budget.remaining(1020), 1020)
        with mock.patch.object(check.time, 'monotonic', return_value=1250):
            self.assertEqual(budget.remaining(1020), 50)
        with mock.patch.object(check.time, 'monotonic', return_value=1300):
            with self.assertRaises(check.Refused):
                budget.remaining(15)

    def test_pty_real_prompt_and_group_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            script = Path(directory) / 'prompt.sh'
            script.write_text('printf "Prompt: "\nread answer\nprintf "done:%s\\n" "$answer"\n')
            output = check.terminal([str(script)], {'PATH': '/usr/bin:/bin'}, 5,
                ((b'Prompt: ', b'yes\n'),))
            self.assertIn(b'done:yes', output)

    def test_terminal_rejects_descendant_after_natural_leader_exit(self):
        with self.assertRaisesRegex(check.Refused, 'owned_descendant_remained'):
            check.terminal(['-c', 'trap "" HUP; /bin/sleep 3 & exit 0'], {}, 5, ())

    def test_pty_timeout_reaps_exact_child(self):
        with mock.patch.object(check.supervisor, 'stop', wraps=check.supervisor.stop) as stop:
            with self.assertRaisesRegex(check.Refused, 'terminal_timeout'):
                check.terminal(['-c', 'exec /bin/sleep 5'], {}, 0.05, ())
        self.assertEqual(stop.call_count, 1)
        self.assertIsNotNone(stop.call_args.args[0].poll())

    def test_pty_output_limit_reaps_child(self):
        with mock.patch.object(check.supervisor, 'MAX_CAPTURE', 16):
            with self.assertRaisesRegex(check.Refused, 'terminal_output_limit'):
                check.terminal(['-c', 'printf "123456789012345678901234567890"'], {}, 5, ())

    def test_curl_real_https_only_and_script_size_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'script'; path.write_bytes(b'installer')
            with mock.patch.object(check.supervisor, 'execute', return_value=(0, b'')) as execute:
                self.assertEqual(check.fetch_script(check.SOURCES['iris'], path, {}, check.Budget()),
                    b'installer')
            args = execute.call_args.args[0]
            self.assertEqual(args[:2], ['curl', '-q'])
            self.assertIn('--proto-redir', args)
            self.assertEqual(args[-1], check.SOURCES['iris'])
            path.write_bytes(b'x' * 65537)
            with mock.patch.object(check.supervisor, 'execute', return_value=(0, b'')):
                with self.assertRaises(check.Refused):
                    check.fetch_script(check.SOURCES['github'], path, {}, check.Budget())

    def test_download_failure_propagates(self):
        with mock.patch.object(check.supervisor, 'execute', side_effect=check.Refused('operation_failed')):
            with self.assertRaisesRegex(check.Refused, 'operation_failed'):
                check.fetch_script(check.SOURCES['iris'], Path('unused'), {}, check.Budget())

    def test_setup_fresh_and_repeat_joined_contracts(self):
        value = dict(schema=1, role='both', inbound_enabled=False, backend='file', npub='npub1example',
            identity_created=True, owner_opened=True, config_changed=True, engine_stop_complete=True,
            mdk_stop_complete=True, runtime_cleanup_complete=True)
        check.setup_contract(value)
        repeated = dict(value, identity_created=False, owner_opened=False, config_changed=False)
        check.setup_contract(repeated, True, value)
        for key in ('engine_stop_complete', 'mdk_stop_complete', 'runtime_cleanup_complete',
                'identity_created', 'owner_opened'):
            invalid = dict(value, **{key: False})
            with self.subTest(key=key), self.assertRaises(check.Refused):
                check.setup_contract(invalid)
        with self.assertRaises(check.Refused):
            check.setup_contract(dict(repeated, npub='npub1other'), True, value)

    def test_release_exact_source_not_bootstrap_commit(self):
        value = {'schema': 1, 'version': '0.2.0', 'target': check.TARGETS[0],
            'commit': 'a' * 40, 'formats': [1, 5, 1, 1, 1]}
        check.release_contract(value, context(), check.TARGETS[0], value['formats'])
        for key, change in (('commit', 'b' * 40), ('version', '0.1.0'), ('target', check.TARGETS[1]),
                ('extra', True), ('formats', [1])):
            with self.subTest(key=key), self.assertRaises(check.Refused):
                check.release_contract(dict(value, **{key: change}), context(), check.TARGETS[0], [1, 5, 1, 1, 1])

    def test_unsafe_launcher_outside_version_root_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); bins = root / 'b'; bins.mkdir()
            prefix = root / 'p'; (prefix / 'versions').mkdir(parents=True)
            (bins / 'marmish').symlink_to('/bin/sh')
            with self.assertRaisesRegex(check.Refused, 'unsafe_launcher_target'):
                check.safe_version(prefix, bins)

    def test_status_exact_contract_and_independent_launcher_links(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); bins = root / 'b'; bins.mkdir(); prefix = root / 'p'
            for name in ('marmish', 'marmishd'):
                (bins / name).symlink_to(prefix / 'current' / name)
            value = dict(schema=1, operation='status', version='0.2.0', target=check.TARGETS[0],
                active=True, generation=1, prefix=str(prefix), bin_dir=str(bins))
            check.status_contract(value, context(), check.TARGETS[0], prefix, bins)
            with self.assertRaises(check.Refused):
                check.status_contract(dict(value, generation=2), context(), check.TARGETS[0], prefix, bins)
            (bins / 'marmishd').unlink(); (bins / 'marmishd').symlink_to('/bin/sh')
            with self.assertRaises(check.Refused):
                check.status_contract(value, context(), check.TARGETS[0], prefix, bins)

    def test_manifest_actual_bytes_modes_hashes_and_wrong_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            value = context()
            raw = self.create_manifest(root, value)
            self.assertEqual(check.check_manifest(raw, value, check.TARGETS[0], root), [1, 5, 1, 1, 1])
            with self.assertRaisesRegex(check.Refused, 'manifest_digest'):
                check.check_manifest(raw + b'foreign', value, check.TARGETS[0], root)
            (root / 'marmish').chmod(0o755)
            with self.assertRaisesRegex(check.Refused, 'installed_file_metadata'):
                check.check_manifest(raw, value, check.TARGETS[0], root)
            (root / 'marmish').chmod(0o755)
            (root / 'marmish').write_bytes(b'foreign')
            (root / 'marmish').chmod(0o555)
            with self.assertRaises(check.Refused):
                check.check_manifest(raw, value, check.TARGETS[0], root)

    @staticmethod
    def create_manifest(root, value):
        lines = ['marmish-manifest 1', 'version ' + value['version'],
            'target ' + check.TARGETS[0], 'commit ' + value['source_commit'],
            'signing none notarized=no', 'minimum glibc=2.39',
            'state profile=1 journal=5 engine=1 config=1 commands=1']
        for name in check.FILES:
            raw = name.encode()
            path = root / name; path.write_bytes(raw)
            mode = '0444' if name == check.FILES[-1] else '0555'
            path.chmod(int(mode, 8))
            lines.append('file ' + check.sha(raw) + ' ' + str(len(raw)) + ' ' + mode + ' ' + name)
        raw = ('\n'.join(lines) + '\nend\n').encode()
        value['manifest_pins'][check.TARGETS[0]] = check.sha(raw)
        return raw

    def test_cell_dispatch_cleanup_and_busy_lease_or_runtime_preserve_state(self):
        for source, locked, runtime in (('iris', False, False), ('github', False, False),
                ('iris', True, False), ('iris', False, True)):
            with self.subTest(source=source, locked=locked, runtime=runtime), \
                    tempfile.TemporaryDirectory() as directory:
                root = Path(directory); value = context()
                fixture = root / 'fixture'; fixture.mkdir()
                manifest = self.create_manifest(fixture, value)
                script = b'reviewed script'
                value['bootstrap_sha256'] = check.sha(script)
                context_path = root / 'context'; context_path.write_bytes(check.encoded(value))
                output_path = root / 'report'
                initial = dict(schema=1, role='both', inbound_enabled=False, backend='file',
                    npub='npub1private', identity_created=True, owner_opened=True, config_changed=True,
                    engine_stop_complete=True, mdk_stop_complete=True, runtime_cleanup_complete=True)
                paths, leases = {}, []
                def terminal(arguments, environment, timeout):
                    self.assertTrue(arguments[0].endswith('/' + source + '.sh'))
                    self.assertNotIn('--asset-dir', arguments)
                    self.assertIn('--quickstart', arguments)
                    self.assertEqual(set(environment), {'HOME', 'USER', 'LOGNAME', 'PATH', 'LC_ALL', 'TMPDIR'})
                    self.assertFalse((Path(environment['PATH']) / 'python3').exists())
                    prefix = Path(arguments[arguments.index('--prefix') + 1])
                    bins = Path(arguments[arguments.index('--bin-dir') + 1])
                    state = Path(arguments[arguments.index('--state-dir') + 1])
                    paths.update(prefix=prefix, bins=bins, state=state)
                    version = prefix / 'versions' / 'v'; version.mkdir(parents=True)
                    for name in check.FILES:
                        check.shutil.copyfile(fixture / name, version / name)
                        (version / name).chmod(0o444 if name == check.FILES[-1] else 0o555)
                    (version / 'manifest').write_bytes(manifest)
                    (prefix / 'current').symlink_to(version)
                    bins.mkdir()
                    for name in ('marmish', 'marmishd'):
                        (bins / name).symlink_to(prefix / 'current' / name)
                    (state / 'default').mkdir(parents=True)
                    lock = state / 'default' / 'owner.lock'
                    lock.touch()
                    if locked:
                        descriptor = os.open(lock, os.O_RDONLY)
                        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        leases.append(descriptor)
                    if runtime:
                        (Path(environment['TMPDIR']) / 'mr2-owned').mkdir()
                    return check.encoded(initial)
                def execute(arguments, environment, timeout):
                    if arguments[-1] == 'release-info':
                        result = dict(schema=1, version=value['version'], target=check.TARGETS[0],
                            commit=value['source_commit'], formats=[1, 5, 1, 1, 1])
                    elif arguments[-1] == '--version':
                        return 0, b'marmish-native 0.2.0\n'
                    elif arguments[-1] == '--help':
                        return 0, b'Usage: marmish'
                    elif 'install-status' in arguments:
                        result = dict(schema=1, operation='status', version=value['version'],
                            target=check.TARGETS[0], active=True, generation=1,
                            prefix=str(paths['prefix']), bin_dir=str(paths['bins']))
                    else:
                        result = dict(initial, identity_created=False, owner_opened=False,
                            config_changed=False)
                    return 0, check.encoded(result)
                account = argparse.Namespace(pw_dir=str(root), pw_name='test')
                with mock.patch.object(check.pwd, 'getpwuid', return_value=account), \
                        mock.patch.object(check, 'target_host', return_value=check.TARGETS[0]), \
                        mock.patch.object(check, 'fetch_script', return_value=script) as fetch, \
                        mock.patch.object(check, 'terminal', side_effect=terminal), \
                        mock.patch.object(check.supervisor, 'execute', side_effect=execute), \
                        mock.patch.dict(os.environ, GITHUB_RUN_ID='9', GITHUB_RUN_ATTEMPT='1',
                            GITHUB_SHA='b' * 40):
                    status = check.cell(argparse.Namespace(context=context_path, source=source,
                        target=check.TARGETS[0], report=output_path))
                report_bytes = output_path.read_bytes()
                for descriptor in leases:
                    os.close(descriptor)
                self.assertEqual(status, 1 if locked or runtime else 0, report_bytes)
                result = json.loads(report_bytes)
                self.assertEqual(result['owned_state_removed'], not (locked or runtime))
                self.assertEqual(fetch.call_count, 2)
                self.assertNotIn(str(root).encode(), report_bytes)
                self.assertNotIn(b'npub1private', report_bytes)
                self.assertEqual(paths['state'].exists(), locked or runtime)

    def test_rollup_requires_exact_six_successes_and_bindings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); path = root / 'context'; path.write_bytes(check.encoded(context()))
            reports = root / 'reports'; reports.mkdir(); out = root / 'rollup'
            records = []
            for target in check.TARGETS:
                for source in check.SOURCES:
                    record_path = reports / (target + '-' + source + '.json')
                    value = report(context(), target, source)
                    record_path.write_bytes(check.encoded(value)); records.append((record_path, value))
            check.rollup(path, reports, out)
            self.assertTrue(json.loads(out.read_bytes())['ok'])
            changed, original = records[0]
            for key, value in (('qualification_attempt', '2'), ('ok', False), ('failure', 'timeout'),
                    ('stage', 'cleanup'), ('owned_state_removed', False), ('context_sha256', 'e' * 64),
                    ('extra', True), ('source', 'other'), ('elapsed_ms', 1320001)):
                changed.write_bytes(check.encoded(dict(original, **{key: value})))
                with self.subTest(key=key), self.assertRaises(check.Refused):
                    check.rollup(path, reports, out)
            changed.unlink()
            with self.assertRaisesRegex(check.Refused, 'cell_report_count'):
                check.rollup(path, reports, out)

    def test_rollup_duplicate_cannot_replace_missing_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); path = root / 'context'; path.write_bytes(check.encoded(context()))
            reports = root / 'reports'; reports.mkdir()
            for index in range(6):
                (reports / str(index)).with_suffix('.json').write_bytes(
                    check.encoded(report(context(), check.TARGETS[0], 'iris')))
            with self.assertRaisesRegex(check.Refused, 'cell_report_duplicate'):
                check.rollup(path, reports, root / 'out')


if __name__ == '__main__':
    unittest.main()
