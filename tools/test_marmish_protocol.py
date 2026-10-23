"""Application boundary tests, independent of relays and MDK processes."""
import copy
import io
import os
import select
import sys
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

from mdk_smoke import SmokeError
from marmish_protocol import (Dispatcher, EngineBridge, History, decode, encode, envelope,
                              group_session, incoming, load_json, save_json, validate_frame)

CLIENT, SERVER, GROUP, SID = '11' * 32, '22' * 32, '33' * 32, '44' * 16


class FakeEngine:
    def __init__(self):
        self.calls = []

    def call(self, op, **body):
        self.calls.append((op, body))
        return {'token': '55' * 32} if op == 'token' else {'state': 'running'}


class ProtocolTests(unittest.TestCase):
    def setUp(self):
        self.engine = FakeEngine()
        self.dispatch = Dispatcher(self.engine, SERVER)
        self.serial = 0

    def frame(self, op='hello', body=None, challenge=None):
        self.serial += 1
        return envelope(SID, GROUP, CLIENT, SERVER, f'{self.serial:032x}', op,
                        {'nonce': '66' * 32} if body is None else body, challenge)

    def hello(self):
        reply = self.dispatch.dispatch(self.frame(), SID, GROUP, CLIENT)
        return reply['body']['result']['challenge']

    def test_binding_and_sender_checks_never_reach_engine(self):
        for field in ('protocol', 'group', 'client', 'server', 'session', 'type'):
            frame = self.frame()
            frame[field] = 'wrong'
            with self.assertRaises(SmokeError):
                self.dispatch.dispatch(frame, SID, GROUP, CLIENT)
        row = {'group_id': GROUP, 'from': CLIENT, 'direction': 'received',
               'kind': 30100, 'tags': [['marmish', 'v1']], 'plaintext': '{}'}
        self.assertTrue(incoming(row, GROUP, CLIENT))
        for key, value in [('from', SERVER), ('group_id', '77' * 32), ('direction', 'sent'),
                           ('kind', 9), ('tags', [])]:
            with self.subTest(key=key):
                bad = dict(row, **{key: value})
                self.assertFalse(incoming(bad, GROUP, CLIENT))
        self.assertEqual(self.engine.calls, [])

    def test_stale_restart_and_expired_challenges(self):
        with patch('marmish_protocol.continuous_time', return_value=100):
            challenge = self.hello()
            frame = self.frame('token', {}, challenge)
            self.dispatch.dispatch(frame, SID, GROUP, CLIENT)
        with patch('marmish_protocol.continuous_time', return_value=161):
            self.assertEqual(self.dispatch.dispatch(frame, SID, GROUP, CLIENT)['body'],
                             {'ok': False, 'error': 'stale_challenge'})
            frame['challenge'] = self.hello()
            with self.assertRaisesRegex(SmokeError, 'request_conflict'):
                self.dispatch.dispatch(frame, SID, GROUP, CLIENT)
        fresh = Dispatcher(self.engine, SERVER)
        self.assertEqual(fresh.dispatch(frame, SID, GROUP, CLIENT)['body'],
                         {'ok': False, 'error': 'stale_challenge'})
        self.assertEqual(len(self.engine.calls), 1)

    def test_exact_retry_cached_conflict_rejected_and_auth_derived(self):
        challenge = self.hello()
        frame = self.frame('token', {}, challenge)
        first = self.dispatch.dispatch(frame, SID, GROUP, CLIENT)
        self.assertEqual(first, self.dispatch.dispatch(frame, SID, GROUP, CLIENT))
        self.assertEqual(len(self.engine.calls), 1)
        self.assertEqual(self.engine.calls[0][1]['principal'], CLIENT)
        bad = copy.deepcopy(frame)
        bad['op'], bad['body'] = 'status', {'invocation': 'aa' * 16}
        with self.assertRaisesRegex(SmokeError, 'request_conflict'):
            self.dispatch.dispatch(bad, SID, GROUP, CLIENT)
        bad = self.frame('token', {'principal': SERVER}, challenge)
        with self.assertRaises(SmokeError):
            self.dispatch.dispatch(bad, SID, GROUP, CLIENT)

    def test_general_chat_room_cannot_be_enrolled(self):
        for group in ({}, {'profile': {'name': 'hello', 'description': 'chat'}},
                      {'profile': {'name': 'marmish/v1/' + SID, 'description': 'chat'}}):
            with self.assertRaises(SmokeError):
                group_session(group)
        self.assertEqual(group_session({'profile': {'name': 'marmish/v1/' + SID,
            'description': 'marmish/v1 command transport'}}), SID)

    def test_history_startup_baseline_retry_and_gap(self):
        history = History()
        hello_before_restart = {'message_id': '01' * 32}
        self.assertEqual(history.scan([hello_before_restart]), [])
        retried = {'message_id': '02' * 32}
        self.assertEqual(history.scan([hello_before_restart, retried]), [retried])
        self.assertEqual(history.scan([hello_before_restart, retried]), [])
        with self.assertRaisesRegex(SmokeError, 'history_gap'):
            history.scan([{'message_id': f'{i:064x}'} for i in range(100, 200)])
        initial_full = History()
        self.assertEqual(initial_full.scan([{'message_id': f'{i:064x}'} for i in range(100)]), [])
        self.assertEqual(initial_full.scan([{'message_id': f'{i:064x}'} for i in range(100)]), [])

    def test_oldest_first_full_window_has_overlap_and_preserves_delivery_order(self):
        history = History()
        rows = [{'message_id': f'{i:064x}'} for i in range(100)]
        self.assertEqual(history.scan(rows), [])
        for i in range(100, 250):
            new = {'message_id': f'{i:064x}'}
            rows = rows[1:] + [new]
            self.assertEqual(history.scan(rows), [new])
        # A delayed event inside the retained window is found by ID, not timestamp.
        delayed = {'message_id': 'ff' * 32}
        rows = rows[:20] + [delayed] + rows[21:]
        self.assertEqual(history.scan(rows), [delayed])

    def test_strict_json_and_private_atomic_journal(self):
        for value in ('{"x":1,"x":2}', '{"x":NaN}', '[]', '{', 'x' * 32769):
            with self.assertRaises(SmokeError):
                decode(value)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'journal.json'
            value = {'invocation': 'aa' * 16, 'command': {'argv': ['AA==']}}
            save_json(path, value)
            self.assertEqual(load_json(path), value)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            path.unlink()
            path.symlink_to('/etc/passwd')
            with self.assertRaises(OSError):
                load_json(path)

    def test_exponent_overflow_is_rejected_before_dispatch(self):
        for number in ('1e999', '-1e999'):
            raw = encode(self.frame('write_stdin', {'offset': 0})).replace(
                b'"offset":0', ('"offset":' + number).encode())
            with self.assertRaisesRegex(SmokeError, 'invalid_json'):
                decode(raw)
        frame = self.frame('token', {'nested': [float('inf')]})
        with self.assertRaisesRegex(SmokeError, 'invalid_json'):
            validate_frame(frame, SID, GROUP, CLIENT, SERVER)
        self.assertEqual(self.engine.calls, [])


class BridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        executable = Path(self.temp.name) / 'bridge'
        executable.write_text('#!' + str(Path(sys.executable).resolve()) + '''
import json, sys, time
for line in sys.stdin.buffer:
    request = json.loads(line)
    op = request['op']
    reply = {'id': request['id'], 'ok': True, 'result': {}}
    if op == 'slow':
        time.sleep(60)
    elif op == 'wrong_id':
        reply['id'] = '0' * 32
    elif op == 'bad_error':
        reply = {'id': request['id'], 'ok': False, 'error': []}
    elif op == 'busy':
        reply = {'id': request['id'], 'ok': False, 'error': {'code': 'busy'}}
    elif op == 'truncated':
        sys.stdout.write('{')
        sys.stdout.flush()
        sys.exit(0)
    print(json.dumps(reply), flush=True)
''')
        executable.chmod(0o700)
        self.bridge = EngineBridge(executable, {})
        self.addCleanup(self.bridge.close)

    def assert_broken(self):
        self.assertTrue(self.bridge.broken)
        self.assertIsNotNone(self.bridge.child.poll())
        with self.assertRaisesRegex(SmokeError, 'bridge_broken'):
            self.bridge.call('next')

    def test_timeout_terminates_stream_and_forbids_late_reply_reuse(self):
        real_select = select.select

        def timeout_read(readers, writers, errors, timeout):
            return ([], [], []) if readers else real_select(readers, writers, errors, timeout)

        with patch('marmish_protocol.select.select', side_effect=timeout_read):
            with self.assertRaisesRegex(SmokeError, 'bridge_timeout'):
                self.bridge.call('slow')
        self.assert_broken()

    def test_partial_write_failure_permanently_closes_stream(self):
        real_write = os.write
        calls = []

        def partial_write(fd, data):
            calls.append(data)
            if len(calls) == 1:
                return real_write(fd, data[:5])
            raise OSError('synthetic write failure')

        with patch('marmish_protocol.os.write', side_effect=partial_write):
            with self.assertRaisesRegex(SmokeError, 'bridge_io_failure'):
                self.bridge.call('next')
        self.assertEqual(len(calls), 2)
        self.assert_broken()

    def test_truncated_response_permanently_closes_stream(self):
        with self.assertRaisesRegex(SmokeError, 'bridge_closed'):
            self.bridge.call('truncated')
        self.assert_broken()

    def test_schema_errors_permanently_close_stream(self):
        with self.assertRaisesRegex(SmokeError, 'bridge_schema'):
            self.bridge.call('bad_error')
        self.assert_broken()

    def test_wrong_response_id_permanently_closes_stream(self):
        with self.assertRaisesRegex(SmokeError, 'bridge_schema'):
            self.bridge.call('wrong_id')
        self.assert_broken()

    def test_valid_engine_error_preserves_synchronization(self):
        with self.assertRaisesRegex(SmokeError, 'busy'):
            self.bridge.call('busy')
        self.assertFalse(self.bridge.broken)
        self.assertEqual(self.bridge.call('next'), {})


if __name__ == '__main__':
    unittest.main()
