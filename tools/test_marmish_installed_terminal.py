"""Offline authenticated terminal delivery contracts; no application is launched."""
import copy
import hashlib
import io
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import marmish_installed_terminal as terminal
from marmish_installed_fixture import derive_pair, expected_streams, stdin_bytes
from marmish_installed_release import ROLES, RELAYS, canonical_bytes
from marmish_installed_rendezvous import pin_digest
from marmish_installed_runtime import _record_base, payload_record
from marmish_installed_state import read_private, write_private
from mdk_smoke import SmokeError
from test_marmish_installed_rendezvous import CTX, NOW, sample_pin, sample_records, ready_record


def bundle(raw, *, name='record.json', extra=b'', mode=0o100600):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w', zipfile.ZIP_DEFLATED) as archive:
        entry = zipfile.ZipInfo(name)
        entry.external_attr = mode << 16
        entry.extra = extra
        entry.compress_type = zipfile.ZIP_DEFLATED
        archive.writestr(entry, raw)
    data = output.getvalue()
    return data, 'sha256:' + hashlib.sha256(data).hexdigest()


class Archives(unittest.TestCase):
    def test_record_larger_than_phase_a_bound_is_accepted(self):
        record = {'data': 'x' * 9000}
        self.assertEqual(terminal.read_terminal_zip(*bundle(canonical_bytes(record))), record)

    def test_digest_mandatory_and_exact(self):
        data, _ = bundle(b'{}')
        for digest in (None, '', 'sha256:' + '0' * 64, 'SHA256:' + hashlib.sha256(data).hexdigest()):
            with self.subTest(digest=digest), self.assertRaises(SmokeError):
                terminal.read_terminal_zip(data, digest)

    def test_oversize_json_zip_symlink_zip64_and_paths_rejected(self):
        cases = [bundle(b'{' + b' ' * terminal.RECORD_LIMIT + b'}'),
                 bundle(b'{}', name='../record.json'), bundle(b'{}', mode=stat.S_IFLNK | 0o600),
                 bundle(b'{}', extra=b'\x01\x00\x00\x00'),
                 (b'x' * (terminal.ZIP_LIMIT + 1), 'sha256:' + 'a' * 64)]
        for case in cases:
            with self.subTest(size=len(case[0])), self.assertRaises(SmokeError):
                terminal.read_terminal_zip(*case)

    def test_duplicate_json_nonfinite_bom_and_multiple_entries_rejected(self):
        for raw in (b'{"x":1,"x":2}', b'{"x":NaN}', b'\xef\xbb\xbf{}', b'[]'):
            with self.subTest(raw=raw), self.assertRaises(SmokeError):
                terminal.read_terminal_zip(*bundle(raw))
        output = io.BytesIO()
        with zipfile.ZipFile(output, 'w') as archive:
            archive.writestr('record.json', b'{}')
            archive.writestr('other.json', b'{}')
        data = output.getvalue()
        with self.assertRaises(SmokeError):
            terminal.read_terminal_zip(data, 'sha256:' + hashlib.sha256(data).hexdigest())

    def test_redirect_never_receives_token(self):
        data, digest = bundle(b'{}')
        item = {'name': 'wanted', 'id': 1, 'size_in_bytes': len(data), 'expired': False, 'digest': digest}
        location = 'https://example.blob.core.windows.net/file?sig=public'
        with patch.object(terminal, 'request_bounded', side_effect=[(302, b'', location), (200, data, None)]) as request:
            self.assertEqual(terminal.download(item, 'wanted', 'secret', 100), {})
            self.assertEqual(request.call_args_list[0].args[1], 'secret')
            self.assertIsNone(request.call_args_list[1].args[1])
        for bad in ('https://evil.example/file', terminal.API + '/other'):
            with patch.object(terminal, 'request_bounded', return_value=(302, b'', bad)) as request:
                with self.assertRaises(SmokeError):
                    terminal.download(item, 'wanted', 'secret', 100)
                self.assertEqual(request.call_count, 1)


class Delivery(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        root.chmod(0o700)
        output = root / 'public'
        output.mkdir(mode=0o700)
        self.pin = dict(sample_pin(), relays=RELAYS)
        self.identities = sample_records(self.pin)
        self.servers = {role: ready_record(self.identities, role) for role in ROLES}
        self.state = {'root': str(root), 'output': str(output), 'role': ROLES[0], 'ctx': CTX,
                      'pin_sha256': pin_digest(self.pin), 'overall_deadline': 2000}
        write_private(root / 'live-start.json', {'start': {
            'continuous_ns': 100_000_000_000, 'monotonic_ns': 100_000_000_000,
            'wall_ns': NOW * 1_000_000_000}, 'deadline': 1900, 'readiness_deadline': 400})
        self.clock = 101
        self.reports = {role: self.report(role) for role in ROLES}
        write_private(output / 'client-result.json', self.reports[ROLES[0]])
        self.items = [{'name': terminal.artifact_name(CTX, role), 'id': index + 1,
                       'digest': 'sha256:' + str(index) * 64, 'size_in_bytes': 100, 'expired': False}
                      for index, role in enumerate(ROLES)]

    def report(self, role):
        report = _record_base(dict(self.state, role=role), self.identities, 'client-result', self.servers)
        report.update(client=self.identities[role]['client'], timing={
            'elapsed_ns': 10, 'active_ns': 10, 'suspended_ns': 0, 'clock_valid': True})
        for server_role in ROLES:
            pair = derive_pair(CTX, self.state['pin_sha256'], self.identities[role], self.identities[server_role])
            out, err = expected_streams(self.servers[server_role]['fixtures'][role]['path'],
                                        pair['context'], pair['challenge'], pair['invocation'])
            report['slots'][server_role] = {key: pair[key] for key in (
                'context', 'invocation', 'probe_invocation')} | {
                'client': self.identities[role]['client'], 'server': self.identities[server_role]['server'],
                'status': 'failed', 'category': 'command_failed', 'exit': None,
                'verification': dict.fromkeys(('probe', 'stdout', 'stderr', 'exit'), False),
                'metrics': None, 'payload': payload_record(stdin_bytes(pair['context']), out, err)}
        return report

    def wait(self, listing):
        def download(item, wanted, token, deadline):
            self.assertEqual(token, 'token-private')
            return self.reports[ROLES[item['id'] - 1]]
        with patch.object(terminal.time, 'time', return_value=NOW), \
                patch.object(terminal, '_listing', side_effect=listing), \
                patch.object(terminal, 'download', side_effect=download):
            return terminal.wait_reports(self.state, self.pin, self.identities, self.servers,
                'token-private', clock=lambda: self.clock, sleep=self.sleep)

    def sleep(self, duration):
        self.clock += duration

    def test_each_report_is_delivered_before_all_artifacts_exist(self):
        calls = 0
        def listing(*args):
            nonlocal calls
            calls += 1
            if calls == 1:
                return self.items[:1]
            path = Path(self.state['output']) / 'client-inbox' / (ROLES[0] + '.json')
            self.assertEqual(read_private(path), canonical_bytes(self.reports[ROLES[0]]))
            return self.items
        self.assertEqual(self.wait(listing), self.reports)
        for role in ROLES:
            raw = read_private(Path(self.state['output']) / 'client-inbox' / (role + '.json'))
            self.assertNotIn(b'token-private', raw)

    def test_failed_reports_without_payload_are_delivered(self):
        for report in self.reports.values():
            for slot in report['slots'].values():
                slot['payload'] = None
        local = Path(self.state['output']) / 'client-result.json'
        local.unlink()
        write_private(local, self.reports[ROLES[0]])
        self.assertEqual(self.wait(lambda *args: self.items), self.reports)

    def test_missing_payload_cannot_claim_success(self):
        report = copy.deepcopy(self.reports[ROLES[0]])
        report['slots'][ROLES[0]].update(status='received', category=None,
                                      exit=7, payload=None)
        with self.assertRaises(SmokeError):
            terminal.validate_report(report, self.state, self.pin, ROLES[0],
                                     self.identities, self.servers)

    def test_duplicate_artifact_names_fail_before_any_delivery(self):
        with self.assertRaisesRegex(SmokeError, 'duplicate_peer_artifact'):
            self.wait(lambda *args: self.items + [dict(self.items[0], id=99)])
        self.assertFalse((Path(self.state['output']) / 'client-inbox').exists())

    def test_swapped_report_identity_context_server_digest_and_pair_fail(self):
        for key in ('role', 'nonce', 'run', 'server_ready_sha256', 'slots'):
            with self.subTest(key=key):
                report = copy.deepcopy(self.reports[ROLES[0]])
                report[key] = copy.deepcopy(self.reports[ROLES[1]][key])
                if key == 'server_ready_sha256':
                    report[key][ROLES[0]] = '0' * 64
                if key == 'run':
                    report[key] = '999'
                with self.assertRaises(SmokeError):
                    terminal.validate_report(report, self.state, self.pin, ROLES[0], self.identities, self.servers)

    def test_payload_and_boolean_exit_are_rejected(self):
        for key in ('payload', 'exit'):
            report = copy.deepcopy(self.reports[ROLES[0]])
            slot = report['slots'][ROLES[0]]
            if key == 'exit':
                slot[key] = True
            else:
                slot[key]['stdin']['bytes'] += 1
            with self.assertRaises(SmokeError):
                terminal.validate_report(report, self.state, self.pin, ROLES[0], self.identities, self.servers)

    def test_local_report_must_match_immutable_canonical_bytes(self):
        self.reports[ROLES[0]]['timing']['elapsed_ns'] = 11
        self.reports[ROLES[0]]['timing']['active_ns'] = 11
        with self.assertRaisesRegex(SmokeError, 'local_terminal_mismatch'):
            self.wait(lambda *args: self.items)

    def test_missing_report_obeys_original_continuous_deadline(self):
        self.clock = 1899
        with self.assertRaisesRegex(SmokeError, 'terminal_timeout'):
            self.wait(lambda *args: self.items[:1])
        self.assertEqual(self.clock, 1900)

    def test_changed_or_disappeared_report_is_not_replaced(self):
        for next_listing in ([], [dict(self.items[0], digest='sha256:' + 'f' * 64)]):
            calls = 0
            def listing(*args):
                nonlocal calls
                calls += 1
                return self.items[:1] if calls == 1 else next_listing
            with self.subTest(next_listing=next_listing), self.assertRaises(SmokeError):
                self.wait(listing)

    def test_deadline_cannot_be_reset_past_thirty_minutes(self):
        path = Path(self.state['root']) / 'live-start.json'
        path.unlink()
        write_private(path, {'start': {'continuous_ns': 1, 'monotonic_ns': 1, 'wall_ns': 1},
                             'deadline': 1900, 'readiness_deadline': 400})
        with self.assertRaises(SmokeError):
            terminal.live_deadline(self.state, clock=lambda: self.clock)

    def test_download_completion_after_deadline_cannot_publish(self):
        def late(*args):
            self.clock = 1901
            return self.reports[ROLES[0]]
        with patch.object(terminal.time, 'time', return_value=NOW), \
                patch.object(terminal, '_listing', return_value=self.items), \
                patch.object(terminal, 'download', side_effect=late):
            with self.assertRaisesRegex(SmokeError, 'terminal_timeout'):
                terminal.wait_reports(self.state, self.pin, self.identities, self.servers,
                    'token-private', clock=lambda: self.clock, sleep=self.sleep)
        self.assertFalse((Path(self.state['output']) / 'client-inbox').exists())

    def test_cli_removes_token_and_reports_only_finite_error(self):
        identities = Path(self.state['root']) / 'identities.json'
        servers = Path(self.state['root']) / 'servers.json'
        write_private(identities, self.identities)
        write_private(servers, self.servers)
        def failed(*args):
            self.assertNotIn('GITHUB_TOKEN', os.environ)
            self.assertEqual(args[4], 'do-not-disclose')
            raise RuntimeError('do-not-disclose')
        stdout = io.StringIO()
        with patch.dict(os.environ, {'GITHUB_TOKEN': 'do-not-disclose'}), \
                patch.object(terminal, 'load_state', return_value=self.state), \
                patch.object(terminal, 'context', return_value=CTX), \
                patch.object(terminal, 'load_pin', return_value=self.pin), \
                patch.object(terminal, 'wait_reports', side_effect=failed), \
                patch('sys.stdout', stdout):
            result = terminal.main(['--state', 'unused', '--pin-dir', 'unused',
                                    '--identities', str(identities), '--servers', str(servers)])
        self.assertEqual(result, 1)
        self.assertNotIn('do-not-disclose', stdout.getvalue())
        status = read_private(Path(self.state['output']) / 'client-inbox-status.json')
        self.assertIn(b'"error":"artifact_invalid"', status)
        self.assertNotIn(b'do-not-disclose', status)


if __name__ == '__main__':
    unittest.main()
