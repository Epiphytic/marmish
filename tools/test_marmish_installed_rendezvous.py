"""Offline contract and real HTTP watchdog tests for installed rendezvous."""
import copy
import errno
import hashlib
import http.server
import io
import json
import os
import signal
from types import SimpleNamespace
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import Mock, patch
import zipfile

import marmish_installed_rendezvous as rv
from marmish_installed_release import ROLE_TARGETS, ROLES, RELAYS, canonical_bytes
from marmish_platform import continuous_time
from mdk_smoke import SmokeError


CTX = {'run': '12', 'attempt': '1', 'sha': 'a' * 40}
NOW = 1800000000


def sample_pin():
    return {'version': '0.1.0-experimental.2', 'source_commit': 'b' * 40,
            'installer_sha256': 'c' * 64,
            'targets': {role: {'target': target, 'archive_sha256': 'd' * 64,
                              'manifest_sha256': 'e' * 64}
                        for role, target in ROLE_TARGETS.items()}}


def sample_records(pin):
    timing = {'unit': 'ns', 'clock': 'continuous+monotonic', 'stages': {
        name: {'elapsed_ns': 20, 'active_ns': 19, 'suspended_ns': 1, 'clock_valid': True}
        for name in rv.REQUIRED_STAGES}}
    return {role: {'format': 1, 'kind': 'identity-ready', **CTX,
                   'pin_sha256': rv.pin_digest(pin), 'role': role, 'target': ROLE_TARGETS[role],
                   'nonce': f'{index + 20:064x}', 'server': f'{index * 2 + 1:064x}',
                   'client': f'{index * 2 + 2:064x}', 'relays': RELAYS,
                   'installation': rv.installation_tuple(pin, role),
                   'interpreter': {'implementation': 'cpython', 'version': [3, 11, 0],
                                   'path_class': 'system'}, 'timing': copy.deepcopy(timing),
                   'published_at': NOW, 'expires_at': NOW + 4200}
            for index, role in enumerate(ROLES)}


def ready_record(records, role):
    root = ('/private/tmp/' if role == 'macos-arm64' else '/tmp/') + 'mci-abcdef'
    return {**copy.deepcopy(records[role]), 'kind': 'server-ready', 'ready': True,
            'server_started_at': NOW,
            'identity_ready_sha256': {key: hashlib.sha256(canonical_bytes(row)).hexdigest()
                                      for key, row in records.items()},
            'fixtures': {key: {'path': root + '/f/' + key + '/main', 'sha256': 'f' * 64}
                         for key in ROLES}}


def archive(record):
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w') as bundle:
        bundle.writestr('record.json', canonical_bytes(record))
    return output.getvalue()


class Schemas(unittest.TestCase):
    def setUp(self):
        self.pin = sample_pin()
        self.records = sample_records(self.pin)

    def test_complete_barrier_and_each_native_fixture(self):
        self.assertEqual(rv.identity_barrier(self.records, CTX, self.pin, now=NOW), self.records)
        for role in ROLES:
            rv.validate_server_ready(ready_record(self.records, role), CTX, self.pin, role,
                                     self.records, now=NOW)

    def test_closed_schema_boolean_expiry_pin_and_timing(self):
        for key, value in [('format', True), ('expires_at', NOW), ('pin_sha256', '0' * 64),
                           ('extra', 'secret')]:
            with self.subTest(key=key):
                record = copy.deepcopy(self.records[ROLES[0]])
                record[key] = value
                with self.assertRaises(SmokeError):
                    rv.validate_identity(record, CTX, self.pin, ROLES[0], now=NOW)
        record = copy.deepcopy(self.records[ROLES[0]])
        record['timing']['stages']['install']['elapsed_ns'] = True
        with self.assertRaises(SmokeError):
            rv.validate_identity(record, CTX, self.pin, ROLES[0], now=NOW)

    def test_all_six_keys_and_three_nonces_unique(self):
        for key in ('server', 'nonce'):
            records = copy.deepcopy(self.records)
            records[ROLES[1]][key] = records[ROLES[0]][key]
            with self.assertRaisesRegex(SmokeError, 'duplicate_barrier_identity'):
                rv.identity_barrier(records, CTX, self.pin, now=NOW)

    def test_server_digest_and_shared_fixture_root(self):
        for mutation in ('digest', 'root', 'escape', 'identity'):
            row = ready_record(self.records, ROLES[0])
            if mutation == 'digest':
                row['identity_ready_sha256'][ROLES[0]] = '0' * 64
            elif mutation == 'root':
                row['fixtures'][ROLES[1]]['path'] = '/tmp/mci-abcdefg/f/' + ROLES[1] + '/main'
            elif mutation == 'escape':
                row['fixtures'][ROLES[1]]['path'] = '/tmp/mci-abcdef/../f/' + ROLES[1] + '/main'
            else:
                row['nonce'] = 'f' * 64
            with self.subTest(mutation=mutation), self.assertRaises(SmokeError):
                rv.validate_server_ready(row, CTX, self.pin, ROLES[0], self.records, now=NOW)


class Artifacts(unittest.TestCase):
    def test_pagination_and_changing_snapshot_restart(self):
        items = [{'id': number, 'name': str(number), 'expired': False, 'size_in_bytes': 10,
                  'digest': 'sha256:' + 'a' * 64} for number in range(1, 102)]
        items[-1].update(name='installed-release-pin', size_in_bytes=100000)
        replies = [ {'total_count': 101, 'artifacts': items[:100]},
                    {'total_count': 102, 'artifacts': items[100:]},
                    {'total_count': 101, 'artifacts': items[:100]},
                    {'total_count': 101, 'artifacts': items[100:]} ]
        with patch.object(rv, 'request_bounded', side_effect=[
                (200, canonical_bytes(row), None) for row in replies]) as request:
            self.assertEqual(rv._listing(CTX, 'token', continuous_time() + 10), items)
            self.assertEqual(request.call_count, 4)

    def test_unstable_pagination_is_bounded(self):
        row = {'total_count': 1, 'artifacts': []}
        with patch.object(rv, 'request_bounded', return_value=(200, canonical_bytes(row), None)) as req:
            with self.assertRaisesRegex(SmokeError, 'artifact_listing_unstable'):
                rv._listing(CTX, 'token', continuous_time() + 10)
            self.assertEqual(req.call_count, 3)

    def test_malformed_listing_types_limits_and_duplicate_json(self):
        for row in ({'total_count': True, 'artifacts': []},
                    {'total_count': 1001, 'artifacts': []},
                    {'total_count': 1, 'artifacts': [{'id': True, 'name': 'peer'}]}):
            with self.subTest(row=row), patch.object(rv, 'request_bounded',
                    return_value=(200, canonical_bytes(row), None)):
                with self.assertRaises(SmokeError):
                    rv._listing(CTX, 'secret', continuous_time() + 1)
        for raw in (b'{"total_count":0,"total_count":0,"artifacts":[]}',
                    b'{"total_count":NaN,"artifacts":[]}'):
            with patch.object(rv, 'request_bounded', return_value=(200, raw, None)):
                with self.assertRaises(rv.RpcError):
                    rv._listing(CTX, 'secret', continuous_time() + 1)

    def test_signed_download_never_receives_token_and_digest_is_mandatory(self):
        raw = archive({'public': 'record'})
        item = {'id': 1, 'name': 'wanted', 'expired': False, 'size_in_bytes': len(raw),
                'digest': 'sha256:' + hashlib.sha256(raw).hexdigest()}
        destination = 'https://example.blob.core.windows.net/artifact?signed=yes'
        with patch.object(rv, 'request_bounded', side_effect=[
                (302, b'', destination), (200, raw, None)]) as req:
            self.assertEqual(rv._archive(item, 'wanted', 'secret', continuous_time() + 10),
                             {'public': 'record'})
            self.assertEqual(req.call_args_list[0].args[1], 'secret')
            self.assertIsNone(req.call_args_list[1].args[1])
        for bad in (None, 'sha256:' + '0' * 64, True):
            broken = {**item, 'digest': bad}
            with patch.object(rv, 'request_bounded', return_value=(200, raw, None)):
                with self.assertRaises(SmokeError):
                    rv._archive(broken, 'wanted', 'secret', continuous_time() + 10)

    def test_redirect_allowlist_and_api_token_boundary(self):
        for url, token in [('https://evil.example/file', None),
                           ('https://example.blob.core.windows.net/file', 'secret'),
                           ('https://api.github.com/repos/evil/marmish/file', 'secret'),
                           ('https://api.github.com:444/repos/Epiphytic/marmish/a', 'secret')]:
            with self.subTest(url=url), self.assertRaises(SmokeError):
                rv.request_bounded(url, token, 100, continuous_time() + 1)

    def test_duplicate_wanted_artifact_fails_before_download(self):
        name = rv.artifact_name(CTX, ROLES[0], 'identity-ready')
        with patch.object(rv, '_listing', return_value=[{'name': name}, {'name': name}]), \
                patch.object(rv, '_archive') as fetch:
            with self.assertRaisesRegex(SmokeError, 'duplicate_peer_artifact'):
                rv.wait_records(CTX, sample_pin(), 'identity-ready', ROLES,
                                deadline=continuous_time() + 1, output=Path('/unused'), token='secret')
            fetch.assert_not_called()

    def test_barrier_persists_only_complete_validated_map(self):
        pin = sample_pin()
        records = sample_records(pin)
        artifacts = [{'name': rv.artifact_name(CTX, role, 'identity-ready'), 'role': role}
                     for role in ROLES]
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'peers.json'
            with patch.object(rv, '_listing', return_value=artifacts), \
                    patch.object(rv, '_archive', side_effect=lambda row, *_args: records[row['role']]), \
                    patch.object(rv.time, 'time', return_value=NOW):
                self.assertEqual(rv.wait_records(CTX, pin, 'identity-ready', ROLES,
                    deadline=continuous_time() + 2, output=output, token='secret'), records)
            self.assertEqual(json.loads(output.read_bytes()), records)
            self.assertEqual(output.stat().st_mode & 0o777, 0o600)


class WorkerCleanup(unittest.TestCase):
    def setUp(self):
        self.child = SimpleNamespace(pid=4321, returncode=None)
        self.child.wait = Mock(side_effect=self.reap)
        self.status = SimpleNamespace(si_pid=4321, si_code=os.CLD_EXITED, si_status=7)
        self.platform = SimpleNamespace(platform='darwin', executable=rv.sys.executable)
        self.enterContext(patch.object(rv, 'sys', self.platform))
        self.waitid = self.enterContext(patch.object(os, 'waitid', return_value=self.status))
        self.calls = []
        self.kill = self.enterContext(patch.object(os, 'killpg', side_effect=self.signal))
        self.enterContext(patch.object(rv, 'continuous_time', side_effect=iter(n / 2 for n in range(20))))
        self.enterContext(patch.object(rv.time, 'sleep'))

    def reap(self, **_kwargs):
        self.child.returncode = 7
        return 7

    def signal(self, pid, number):
        self.assertEqual(pid, self.child.pid)
        self.calls.append((number, self.child.returncode))
        raise OSError(errno.EPERM if number else errno.ESRCH, 'private')

    def refused(self):
        with self.assertRaisesRegex(SmokeError, '^artifact_worker_cleanup_failed$'):
            rv._finish_request_worker(self.child)
        self.child.wait.assert_called_once_with(timeout=2)

    def test_confirmed_exit_then_absence_preserves_reserved_order(self):
        rv._finish_request_worker(self.child)
        self.assertEqual(self.calls, [(signal.SIGKILL, None), (0, 7)])
        self.assertEqual(self.waitid.call_count, 2)
        self.child.wait.assert_called_once_with(timeout=2)

    def test_live_proof_is_not_permission_to_ignore_denial(self):
        self.waitid.return_value = None
        self.refused()
        self.assertEqual(self.calls, [(signal.SIGKILL, None)])

    def test_changed_reaped_or_unobservable_leader_refuses(self):
        for returned, statuses in [
            (7, []),
            (None, [self.status, None]),
            (None, [self.status, SimpleNamespace(si_pid=4321, si_code=os.CLD_EXITED, si_status=8)]),
            (None, OSError(errno.ECHILD, 'private')),
        ]:
            with self.subTest(returned=returned, statuses=statuses):
                self.child.returncode = returned
                self.child.wait.reset_mock()
                self.waitid.side_effect = statuses
                self.calls.clear()
                self.refused()
                self.assertEqual(len(self.calls), 1)

    def test_wrong_platform_or_errno_refuses_but_reaps(self):
        for platform, error in [('linux', errno.EPERM), ('darwin', errno.EACCES)]:
            with self.subTest(platform=platform, error=error):
                self.platform.platform = platform
                self.child.returncode = None
                self.child.wait.reset_mock()
                self.kill.side_effect = OSError(error, 'private')
                self.refused()
        self.waitid.assert_not_called()

    def test_waited_status_must_match_proof(self):
        self.child.wait.side_effect = lambda **_kwargs: 8
        self.refused()
        self.assertEqual(self.calls, [(signal.SIGKILL, None)])

    def test_wait_timeout_remains_cleanup_failure(self):
        self.child.wait.side_effect = rv.subprocess.TimeoutExpired('private', 2)
        self.refused()
        self.assertEqual(self.calls, [(signal.SIGKILL, None)])

    def test_surviving_or_unobservable_group_fails_after_reap(self):
        for result in (None, OSError(errno.EPERM, 'private')):
            with self.subTest(result=result):
                self.child.returncode = None
                self.child.wait.reset_mock()
                self.calls.clear()
                self.enterContext(patch.object(rv, 'continuous_time', side_effect=iter(n / 2 for n in range(20))))
                def kill(pid, number):
                    self.calls.append((number, self.child.returncode))
                    if number:
                        raise OSError(errno.EPERM, 'private')
                    if result is not None:
                        raise result
                self.kill.side_effect = kill
                self.refused()
                self.assertEqual(self.calls[0], (signal.SIGKILL, None))
                self.assertTrue(all(number == 0 and code == 7 for number, code in self.calls[1:]))

    def test_transient_darwin_denial_then_absence_and_linux_denial(self):
        self.kill.side_effect = [OSError(errno.EPERM, 'private'),
                                 OSError(errno.EPERM, 'private'),
                                 ProcessLookupError(errno.ESRCH, 'private')]
        rv._finish_request_worker(self.child)
        self.assertEqual([call.args[1] for call in self.kill.call_args_list], [signal.SIGKILL, 0, 0])
        self.platform.platform = 'linux'
        self.child.returncode = None
        self.child.wait.reset_mock()
        self.kill.reset_mock()
        self.kill.side_effect = [None, OSError(errno.EPERM, 'private')]
        self.refused()
        self.assertEqual([call.args[1] for call in self.kill.call_args_list], [signal.SIGKILL, 0])


class Watchdog(unittest.TestCase):
    def test_unreapable_worker_is_a_finite_cleanup_failure(self):
        original = rv.subprocess.Popen
        children = []

        def spawn(argv, **kwargs):
            child = original(argv, **kwargs)
            wait = child.wait
            def delayed_reap(**options):
                wait(**options)
                raise rv.subprocess.TimeoutExpired(argv, options['timeout'])
            child.wait = delayed_reap
            children.append(child)
            return child

        close = rv.selectors.DefaultSelector.close
        with patch.object(rv.subprocess, 'Popen', side_effect=spawn), \
                patch.object(rv.selectors.DefaultSelector, 'close', autospec=True, side_effect=close) as closed:
            with self.assertRaisesRegex(SmokeError, 'artifact_worker_cleanup_failed'):
                rv._request_process({'url': 'https://api.github.com/repos/Epiphytic/marmish/a',
                    'token': None, 'limit': 100, 'deadline': continuous_time() - 1})
        closed.assert_called_once()
        self.assertTrue(children[0].stdin.closed and children[0].stdout.closed)
        self.assertIsNotNone(children[0].returncode)

    def test_real_worker_success_limit_and_credential_pipe(self):
        received = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                received.append(self.headers.get('Authorization'))
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b'public result')

            def log_message(self, *_args):
                pass

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        original = rv.subprocess.Popen
        children = []

        def spawn(argv, **kwargs):
            self.assertNotIn('private-token', repr(argv))
            self.assertNotIn('private-token', repr(kwargs['env']))
            self.assertNotIn('GITHUB_TOKEN', kwargs['env'])
            child = original(argv, **kwargs)
            children.append(child)
            return child

        try:
            with patch.object(rv.subprocess, 'Popen', side_effect=spawn), \
                    patch.dict(os.environ, {'GITHUB_TOKEN': 'ambient-token'}):
                config = {'url': f'http://127.0.0.1:{server.server_port}/',
                          'token': 'private-token', 'limit': 100,
                          'deadline': continuous_time() + 3}
                self.assertEqual(rv._request_process(config), (200, b'public result', None))
                with self.assertRaisesRegex(SmokeError, 'artifact_oversized'):
                    rv._request_process({**config, 'limit': 2})
            self.assertEqual(received, ['Bearer private-token', 'Bearer private-token'])
            for child in children:
                self.assertIsNotNone(child.returncode)
                with self.assertRaises(ProcessLookupError):
                    os.killpg(child.pid, 0)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)

    def test_darwin_exited_worker_permission_denial_is_reaped(self):
        original_spawn, original_kill = rv.subprocess.Popen, os.killpg
        children, denied = [], []

        def spawn(argv, **kwargs):
            child = original_spawn(argv, **kwargs)
            children.append(child)
            return child

        def kill(pid, number):
            if number == signal.SIGKILL:
                status = os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
                if status is not None:
                    self.assertEqual(status.si_pid, pid)
                    denied.append(pid)
                    raise PermissionError(errno.EPERM, 'simulated exited group')
            return original_kill(pid, number)

        try:
            with patch.object(rv, 'sys', SimpleNamespace(platform='darwin', executable=rv.sys.executable)), \
                    patch.object(rv.subprocess, 'Popen', side_effect=spawn), \
                    patch.object(rv.os, 'killpg', side_effect=kill):
                self.test_real_worker_success_limit_and_credential_pipe()
            self.assertTrue(denied)
            for child in children:
                self.assertIsNotNone(child.returncode)
                self.assertTrue(child.stdin.closed and child.stdout.closed)
        finally:
            # RED leaves an already-exited owned worker unreaped; reclaim only it.
            for child in children:
                child.wait(timeout=2)
                child.stdin.close()
                child.stdout.close()

    def test_real_slow_headers_and_body_have_total_parent_deadline(self):
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                try:
                    if self.path == '/headers':
                        self.wfile.write(b'HTTP/1.1 200 OK\r\nX-Slow: ')
                        for _ in range(100):
                            self.wfile.write(b'a')
                            self.wfile.flush()
                            time.sleep(.03)
                    else:
                        self.send_response(200)
                        self.end_headers()
                        for _ in range(100):
                            self.wfile.write(b'x')
                            self.wfile.flush()
                            time.sleep(.03)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def log_message(self, *_args):
                pass

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        server.daemon_threads = True
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            for path in ('headers', 'listing', 'redirected.zip'):
                start = continuous_time()
                config = {'url': f'http://127.0.0.1:{server.server_port}/{path}',
                          'token': None, 'limit': 65536, 'deadline': start + .4}
                with self.subTest(path=path), self.assertRaisesRegex(SmokeError, 'artifact_request_timeout'):
                    rv._request_process(config)
                self.assertLess(continuous_time() - start, 1.5)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)


if __name__ == '__main__':
    unittest.main()
