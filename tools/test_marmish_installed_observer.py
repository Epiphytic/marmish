"""Exact serialized bridge fields and adversarial authenticated histories."""
import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import marmish_installed_observer as observer_module

from marmish_installed_observer import DaemonObserver, PairObserver, validate_proof
from marmish_installed_release import canonical_bytes
from marmish_protocol import PURPOSE, b64, encode, envelope
from mdk_smoke import SmokeError

CLIENT, SERVER = '1' * 64, '2' * 64
SESSION, GROUP = '3' * 32, '4' * 64
INVOCATION, PROBE = '5' * 32, '6' * 32
CHALLENGE, TOKEN, GENERATION = '7' * 64, '8' * 64, '9' * 32
STDIN = bytes(range(256)) * 32
STDOUT = b'magic' + bytes(range(256)) * 32
STDERR = STDIN[::-1]
COMMAND = {'mode': 'exec', 'argv': [b64(b'/tmp/f/main'), b64(b'\xff\n$HOME')]}
SPEC = {'invocation': INVOCATION, 'probe_invocation': PROBE, 'command': COMMAND,
        'stdin': STDIN, 'stdout': STDOUT, 'stderr': STDERR}


def output_status(produced, closed=False):
    return {'retained': produced, 'produced': produced, 'closed': closed, 'gaps': []}


def input_status(size=8192, closed=True):
    return {'generation': GENERATION, 'accepted': size, 'delivered': size,
            'available_credit': 65536, 'health': 'closed' if closed else 'open', 'gap_count': 0}


def status(exited=False):
    return {'invocation': INVOCATION, 'state': 'exited' if exited else 'running',
            'outcome': {'kind': 'code', 'value': 7} if exited else None,
            'cancel_requested': False, 'input': input_status(),
            'stdout': output_status(len(STDOUT), exited),
            'stderr': output_status(len(STDERR), exited)}


def transcript():
    rows = []

    def exchange(op, body, result=None, error=None):
        rid = f'{len(rows) + 1:032x}'
        challenge = None if op == 'hello' else CHALLENGE
        request = envelope(SESSION, GROUP, CLIENT, SERVER, rid, op, body, challenge)
        response = envelope(SESSION, GROUP, CLIENT, SERVER, rid, op,
                            {'ok': False, 'error': error} if error else {'ok': True, 'result': result},
                            challenge, response=True)
        for frame, incoming in ((request, True), (response, False)):
            rows.append({'group_id': GROUP, 'message_id': f'{len(rows) + 1:064x}',
                         'from': CLIENT if incoming else SERVER,
                         'direction': 'received' if incoming else 'sent', 'kind': 30100,
                         'tags': [['marmish', 'v1']], 'plaintext': encode(frame).decode(),
                         'recorded_at': 0, 'received_at': 0})

    exchange('hello', {'nonce': 'a' * 64}, {'nonce': 'a' * 64, 'challenge': CHALLENGE, 'lifetime': 60})
    exchange('status', {'invocation': PROBE}, error='not_found')
    exchange('token', {}, {'token': TOKEN})
    exchange('submit', {'invocation': INVOCATION, 'token': TOKEN, 'command': COMMAND},
             {'disposition': 'accepted', 'status': status()})
    exchange('begin_input', {'invocation': INVOCATION}, input_status(0, False))
    for offset in (0, 4096):
        exchange('write_stdin', {'invocation': INVOCATION, 'generation': GENERATION,
                                'offset': offset, 'bytes': b64(STDIN[offset:offset + 4096]), 'fresh_ms': 60000},
                 {'accepted': offset + 4096, 'available_credit': 65536, 'duplicate': False})
    exchange('stdin_eof', {'invocation': INVOCATION, 'generation': GENERATION, 'final_offset': 8192}, input_status())
    for cursor in (0, 4096, 8192):
        cursors = {'stdout': min(cursor, len(STDOUT)), 'stderr': min(cursor, len(STDERR))}
        result = {'status': status(True)}
        for stream, data in (('stdout', STDOUT), ('stderr', STDERR)):
            offset = cursors[stream]
            chunk = data[offset:offset + 4096]
            result[stream] = {'bytes': b64(chunk), 'next_cursor': offset + len(chunk),
                              'status': output_status(len(data), True)}
        exchange('poll', {'invocation': INVOCATION, **{stream + '_cursor': offset for stream, offset in cursors.items()}}, result)
    return rows


def observer():
    return PairObserver(group=GROUP, session=SESSION, client=CLIENT, server=SERVER, **SPEC)


def change_frame(row, mutate):
    frame = json.loads(row['plaintext'])
    mutate(frame)
    row['plaintext'] = encode(frame).decode()


class PairTests(unittest.TestCase):
    def test_exact_serialized_status_and_binary_byte_proof(self):
        pair = observer()
        pair.scan(transcript())
        proof = pair.proof()
        self.assertEqual(proof['submit']['count'], 1)
        self.assertEqual(proof['input']['bytes'], 8192)
        self.assertEqual(proof['stdout']['bytes'], len(STDOUT))
        self.assertNotIn(CHALLENGE, canonical_bytes(proof).decode())
        self.assertNotIn(TOKEN, canonical_bytes(proof).decode())
        validate_proof(proof, client=CLIENT, **{key: SPEC[key] for key in
                       ('invocation', 'probe_invocation', 'stdin', 'stdout', 'stderr')})

    def test_response_before_request_and_identical_retransmission(self):
        rows = transcript()
        pair = observer()
        reordered = [row for row in rows if row['direction'] == 'sent'] + [
            row for row in rows if row['direction'] == 'received']
        pair.scan(reordered)
        proof = pair.proof()
        duplicate = copy.deepcopy(rows[6])
        duplicate['message_id'] = 'f' * 64
        pair.scan([*rows, duplicate])
        self.assertEqual(pair.proof()['submit']['count'], proof['submit']['count'])

    def test_extra_incoming_submit_fails_without_any_reply(self):
        pair = observer()
        rows = transcript()
        pair.scan(rows)
        extra = copy.deepcopy(rows[6])
        extra['message_id'] = 'e' * 64
        change_frame(extra, lambda frame: frame['body'].update(invocation='f' * 32))
        with self.assertRaisesRegex(SmokeError, 'unexpected_submission'):
            pair.scan([*rows, extra])
        with self.assertRaises(SmokeError):
            pair.proof()

    def test_incoming_expected_submit_requires_matching_successful_admission(self):
        rows = transcript()
        del rows[7]
        pair = observer()
        pair.scan(rows)
        with self.assertRaisesRegex(SmokeError, 'observer_incomplete'):
            pair.proof()

    def test_each_authenticated_binding_and_response_binding(self):
        mutations = [lambda row: row.update(direction='received'),
                     lambda row: row.update(kind=True), lambda row: row.update(tags=[]),
                     lambda row: row.update(group_id='a' * 64),
                     lambda row: row.update({'from': 'a' * 64}),
                     lambda row: change_frame(row, lambda frame: frame.update(client='a' * 64)),
                     lambda row: change_frame(row, lambda frame: frame.update(op='cancel'))]
        for mutate in mutations:
            rows = transcript()
            mutate(rows[3])
            with self.subTest(mutation=mutate), self.assertRaises(SmokeError):
                observer().scan(rows)

    def test_initial_full_window_and_later_lost_overlap_are_latched(self):
        def padded(count, start=0):
            template = transcript()[0]
            return [{**template, 'message_id': f'{index + 1000:064x}'}
                    for index in range(start, start + count)]
        with self.assertRaisesRegex(SmokeError, 'history_gap'):
            observer().scan(padded(100))
        pair = observer()
        pair.scan(padded(20))
        with self.assertRaisesRegex(SmokeError, 'history_gap'):
            pair.scan(padded(100, 100))
        with self.assertRaisesRegex(SmokeError, 'observer_previous_failure'):
            pair.scan(transcript())

    def test_rolling_full_window_overlap_retains_all_rows_and_overflow_fails(self):
        template = transcript()[0]
        rows = [{**template, 'message_id': f'{index + 1000:064x}'} for index in range(111)]
        pair = observer()
        pair.scan(rows[:20])
        pair.scan(rows[10:110])
        pair.scan(rows[11:111])
        self.assertEqual(len(pair.rows), 111)
        with patch.object(observer_module, 'MAX_ROWS', 2):
            with self.assertRaisesRegex(SmokeError, 'observer_history_overflow'):
                observer().scan(rows[:3])

    def test_unanswered_nonadmitting_frames_and_stale_challenge_are_benign(self):
        rows = transcript()
        unanswered_hello = copy.deepcopy(rows[0])
        unanswered_hello['message_id'] = 'b' * 64
        change_frame(unanswered_hello, lambda frame: frame.update(id='b' * 32))
        unanswered_token = copy.deepcopy(rows[4])
        unanswered_token['message_id'] = 'c' * 64
        change_frame(unanswered_token, lambda frame: frame.update(id='c' * 32))
        stale = copy.deepcopy(rows[5])
        stale['message_id'] = 'd' * 64
        change_frame(stale, lambda frame: frame.update(id='c' * 32,
                     body={'ok': False, 'error': 'stale_challenge'}))
        pair = observer()
        pair.scan([unanswered_hello, *rows, unanswered_token, stale])
        self.assertEqual(pair.proof()['submit']['count'], 1)

    def test_duplicate_row_mutation_but_not_mutable_timestamps_is_rejected(self):
        pair = observer()
        rows = transcript()
        pair.scan(rows)
        changed = copy.deepcopy(rows)
        changed[0].update(recorded_at=123, received_at=456)
        pair.scan(changed)
        changed[0]['tags'].append(['extra', 'changed'])
        with self.assertRaisesRegex(SmokeError, 'observer_row_conflict'):
            pair.scan(changed)

    def test_cursor_holes_wrong_bytes_bad_terminal_and_generation(self):
        for mutation in ('cursor', 'bytes', 'exit', 'generation', 'eof', 'gap'):
            rows = transcript()
            def modify(frame):
                result = frame['body']['result']
                if mutation == 'cursor':
                    result['stdout']['next_cursor'] += 1
                elif mutation == 'bytes':
                    result['stdout']['bytes'] = b64(b'wrong')
                elif mutation == 'exit':
                    result['status']['outcome']['value'] = True
                elif mutation == 'gap':
                    result['stdout']['status']['gaps'] = [{'start': 0, 'end': 1, 'reason': 'limit'}]
            if mutation == 'generation':
                change_frame(rows[10], lambda frame: frame['body'].update(generation='a' * 32))
            elif mutation == 'eof':
                change_frame(rows[14], lambda frame: frame['body'].update(final_offset=8191))
            else:
                change_frame(rows[-1], modify)
            pair = observer()
            with self.subTest(mutation=mutation), self.assertRaises(SmokeError):
                pair.scan(rows)
                pair.proof()

    def test_closed_public_proof_rejects_extra_bool_and_duplicated_row_refs(self):
        pair = observer()
        pair.scan(transcript())
        for mutate in (lambda proof: proof.update(raw_frame='secret'),
                       lambda proof: proof['submit'].update(count=True),
                       lambda proof: proof['input']['writes'][0].update(bytes=True),
                       lambda proof: proof['terminal'].update(request=proof['submit']['request'])):
            proof = pair.proof()
            mutate(proof)
            with self.assertRaises(SmokeError):
                validate_proof(proof, client=CLIENT, **{key: SPEC[key] for key in
                               ('invocation', 'probe_invocation', 'stdin', 'stdout', 'stderr')})


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.path = Path(self.directory.name) / 'profile.json'
        self.rpc = Mock()
        self.clock = [100.0]
        self.expected = {f'{index:064x}': SPEC for index in (11, 12, 13)}

    def tearDown(self):
        self.directory.cleanup()

    def write(self, value):
        self.path.write_bytes(canonical_bytes({'account': SERVER, **value}))
        self.path.chmod(0o600)

    def create(self):
        return DaemonObserver(self.rpc, account=SERVER, expected=self.expected,
                              profile_path=self.path, clock=lambda: self.clock[0])

    def test_quarantine_unknown_pending_and_unexpected_accepted_rooms(self):
        for profile in ({'server_quarantine': {'aa': 'room_fault'}},
                        {'server_quarantine_full': True},
                        {'server_pending': {'aa': {'peer': CLIENT}}},
                        {'server_groups': {'aa': {'peer': CLIENT, 'session': SESSION}}}):
            self.write(profile)
            with self.subTest(profile=profile), self.assertRaises(SmokeError):
                self.create().tick()
        self.rpc.history.assert_not_called()

    def test_purpose_membership_authentication_and_revalidation(self):
        peer = next(iter(self.expected))
        self.write({'server_groups': {GROUP: {'peer': peer, 'session': SESSION}}})
        self.rpc.show_group.return_value = {'group': {'pending_confirmation': False, 'profile': {
            'name': 'marmish/v1/' + SESSION, 'description': PURPOSE}}}
        self.rpc.members.return_value = [{'member_id': key, 'npub': key, 'local': key == SERVER}
                                         for key in (SERVER, peer)]
        self.rpc.history.return_value = []
        daemon = self.create()
        daemon.tick()
        self.clock[0] += .5
        daemon.tick()
        self.assertEqual(self.rpc.show_group.call_count, 1)
        daemon.tick(final=True)
        self.assertEqual(self.rpc.show_group.call_count, 2)
        self.rpc.members.return_value[1]['npub'] = CLIENT
        with self.assertRaisesRegex(SmokeError, 'membership_mismatch'):
            daemon.tick(final=True)
        with self.assertRaises(SmokeError):
            daemon.proofs()

    def test_sampling_gap_and_disappearing_group_fail_closed(self):
        self.write({})
        daemon = self.create()
        daemon.tick()
        self.clock[0] += 1.1
        daemon.tick()  # A single allowed RPC timeout is not evidence loss.
        self.clock[0] += 20.1
        with self.assertRaisesRegex(SmokeError, 'observer_sampling_gap'):
            daemon.tick()

    def test_all_three_authenticated_pairs_require_final_sample_and_no_disappearing_room(self):
        bindings = {f'{index + 50:064x}': {'peer': peer, 'session': SESSION}
                    for index, peer in enumerate(self.expected)}
        histories = {}
        for group, binding in bindings.items():
            peer = binding['peer']
            rows = transcript()
            for row in rows:
                row['group_id'] = group
                if row['direction'] == 'received':
                    row['from'] = peer
                change_frame(row, lambda frame: frame.update(group=group, client=peer))
            histories[group] = rows
        self.write({'server_groups': bindings})
        self.rpc.show_group.return_value = {'group': {'pending_confirmation': False, 'profile': {
            'name': 'marmish/v1/' + SESSION, 'description': PURPOSE}}}
        self.rpc.members.side_effect = lambda account, group: [
            {'member_id': key, 'npub': key, 'local': key == SERVER}
            for key in (SERVER, bindings[group]['peer'])]
        self.rpc.history.side_effect = lambda account, group: histories[group]
        daemon = self.create()
        daemon.tick()
        with self.assertRaisesRegex(SmokeError, 'observer_incomplete'):
            daemon.proofs()
        self.clock[0] += .2
        daemon.tick(final=True)
        self.assertEqual(set(daemon.proofs()), set(self.expected))
        self.write({'server_groups': {}})
        with self.assertRaisesRegex(SmokeError, 'observer_disappeared_group'):
            daemon.tick(final=True)
        with self.assertRaises(SmokeError):
            daemon.proofs()

    def test_pair_proof_does_not_require_other_pairs_to_complete(self):
        daemon = self.create()
        complete, incomplete = observer(), observer()
        complete.scan(transcript())
        incomplete.client = next(iter(self.expected))
        daemon.pairs = {'complete': complete, 'incomplete': incomplete}
        with self.assertRaisesRegex(SmokeError, 'observer_incomplete'):
            daemon.proof_for(CLIENT)
        daemon.finalized = True
        self.assertEqual(daemon.proof_for(CLIENT), complete.proof())
        with self.assertRaises(SmokeError):
            daemon.proof_for(incomplete.client)
        with self.assertRaises(SmokeError):
            daemon.proofs()


if __name__ == '__main__':
    unittest.main()
