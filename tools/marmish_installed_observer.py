"""Read-only authenticated MDK history evidence for installed acceptance."""
import hashlib
import copy
import re

from marmish_installed_release import canonical_bytes
from marmish_installed_state import read_private
from marmish_platform import continuous_time
from marmish_protocol import (OPS, check_members, decode, encode, group_session, hex_id,
                              unb64, validate_frame)
from marmish_rpc import strict_json
from mdk_smoke import identifier, public_key, require

ROW_FIELDS = ('group_id', 'message_id', 'from', 'direction', 'kind', 'tags', 'plaintext')
MAX_ROWS = 8192
INPUT_KEYS = {'generation', 'accepted', 'delivered', 'available_credit', 'health', 'gap_count'}
OUTPUT_KEYS = {'retained', 'produced', 'closed', 'gaps'}
STATUS_KEYS = {'invocation', 'state', 'outcome', 'cancel_requested', 'input', 'stdout', 'stderr'}


def digest(value):
    return hashlib.sha256(canonical_bytes(value)).hexdigest()


def byte_proof(data):
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}


def _uint(value, maximum=2**63 - 1):
    require(type(value) is int and 0 <= value <= maximum, 'observer_schema')
    return value


def _keys(value, keys):
    require(isinstance(value, dict) and set(value) == set(keys), 'observer_schema')


def _row_proof(row):
    return {'message_id': row['message_id'],
            'sha256': digest({key: row[key] for key in ROW_FIELDS})}


def _input(value):
    _keys(value, INPUT_KEYS)
    if value['generation'] is not None:
        hex_id(value['generation'])
    for key in ('accepted', 'delivered', 'available_credit', 'gap_count'):
        _uint(value[key])
    require(value['delivered'] <= value['accepted'] <= 8192 and value['gap_count'] == 0 and
            value['health'] in ('open', 'eof_pending', 'closed'), 'observer_input_status')


def _output(value, limit):
    _keys(value, OUTPUT_KEYS)
    _uint(value['retained'], limit)
    _uint(value['produced'], limit)
    require(value['retained'] <= value['produced'] and type(value['closed']) is bool and
            value['gaps'] == [], 'observer_output_status')


def _status(value, invocation, limits):
    _keys(value, STATUS_KEYS)
    require(value['invocation'] == invocation and value['state'] in
            ('accepted', 'launching', 'running', 'exited') and
            value['cancel_requested'] is False, 'observer_execution_status')
    _input(value['input'])
    for stream in ('stdout', 'stderr'):
        _output(value[stream], limits[stream])
    if value['state'] == 'exited':
        require(value['outcome'] == {'kind': 'code', 'value': 7} and
                type(value['outcome'].get('value')) is int, 'observer_exit_status')
    else:
        require(value['outcome'] is None, 'observer_exit_status')


class PairObserver:
    """Never discard a first window or infer admission from a reply alone."""
    def __init__(self, *, group, session, client, server, invocation, probe_invocation,
                 command, stdin, stdout, stderr):
        self.group, self.session = identifier(group), hex_id(session)
        self.client, self.server = public_key(client), public_key(server)
        self.invocation, self.probe = hex_id(invocation), hex_id(probe_invocation)
        require(self.invocation != self.probe and self.client != self.server, 'observer_binding')
        require(all(isinstance(value, bytes) and len(value) <= 131072 for value in
                    (stdin, stdout, stderr)), 'observer_payload_limit')
        self.command, self.expected = copy.deepcopy(command), {'stdin': stdin, 'stdout': stdout, 'stderr': stderr}
        self.rows, self.previous = {}, None
        self.requests, self.responses, self.done = {}, {}, set()
        self.success, self.hellos, self.tokens = {}, {}, set()
        self.streams = {key: bytearray() for key in self.expected}
        self.produced = {'stdout': 0, 'stderr': 0}
        self.generation, self.eof, self.terminal, self.admission, self.submit_body = None, None, None, None, None
        self.writes, self.cold, self.cancelled = {}, None, False
        self.failed = False
        self.unexpected_submissions = 0

    def scan(self, rows):
        require(not self.failed, 'observer_previous_failure')
        try:
            self._scan(rows)
        except Exception:
            self.failed = True
            raise

    def _scan(self, rows):
        require(isinstance(rows, list) and len(rows) <= 100, 'observer_history')
        ids = [identifier(row.get('message_id')) for row in rows if isinstance(row, dict)]
        require(len(ids) == len(rows) and len(set(ids)) == len(ids), 'observer_history')
        if len(ids) == 100:
            require(self.previous is not None and bool(set(ids) & self.previous), 'history_gap')
        for row in rows:
            require(all(key in row for key in ROW_FIELDS) and row['group_id'] == self.group and
                    type(row['kind']) is int and row['kind'] == 30100 and
                    isinstance(row['tags'], list) and ['marmish', 'v1'] in row['tags'] and
                    all(isinstance(tag, list) and all(isinstance(item, str) for item in tag)
                        for tag in row['tags']), 'observer_authenticated_row')
            received = row['direction'] == 'received'
            require((received and row['from'] == self.client) or
                    (row['direction'] == 'sent' and row['from'] == self.server),
                    'observer_authenticated_row')
            proof = _row_proof(row)
            mid = row['message_id']
            if mid in self.rows:
                require(self.rows[mid] == proof['sha256'], 'observer_row_conflict')
                continue
            require(len(self.rows) < MAX_ROWS, 'observer_history_overflow')
            frame = validate_frame(decode(row['plaintext']), self.session, self.group,
                                   self.client, self.server, response=not received)
            self.rows[mid] = proof['sha256']
            if received:
                self._request(frame, proof)
            else:
                self._response(frame, proof)
        self.previous = set(ids)
        self._correlate()

    def _request(self, frame, proof):
        op, body, rid = frame['op'], frame['body'], frame['id']
        _keys(body, {'nonce'} if op == 'hello' else OPS[op])
        if op == 'hello':
            hex_id(body['nonce'], 32)
            require(frame['challenge'] is None, 'observer_challenge')
        else:
            hex_id(frame['challenge'], 32)
        if op not in ('hello', 'token'):
            hex_id(body['invocation'])
            if op == 'submit':
                # A possible admission is counted before any reply can hide it.
                if body['invocation'] != self.invocation or body['command'] != self.command:
                    self.unexpected_submissions += 1
                require(body['invocation'] == self.invocation, 'unexpected_submission')
                require(body['command'] == self.command, 'observer_command')
                hex_id(body['token'], 32)
                require(self.submit_body is None or self.submit_body == body, 'observer_submit_conflict')
                self.submit_body = body
            else:
                require(body['invocation'] == self.invocation or
                        op == 'status' and body['invocation'] == self.probe, 'observer_invocation')
            if op == 'cancel':
                self.cancelled = True
            if op == 'write_stdin':
                hex_id(body['generation'])
                _uint(body['offset'], len(self.expected['stdin']))
                require(1 <= _uint(body['fresh_ms'], 60000), 'observer_input')
                unb64(body['bytes'])
            elif op == 'stdin_eof':
                hex_id(body['generation'])
                _uint(body['final_offset'], len(self.expected['stdin']))
            elif op == 'poll':
                for stream in ('stdout', 'stderr'):
                    _uint(body[stream + '_cursor'], len(self.expected[stream]))
        if rid in self.requests:
            require(encode(self.requests[rid][0]) == encode(frame), 'observer_request_conflict')
        else:
            require(len(self.requests) < 4096, 'observer_request_overflow')
            self.requests[rid] = (frame, proof)

    def _response(self, frame, proof):
        body = frame['body']
        require(type(body.get('ok')) is bool, 'observer_response')
        _keys(body, {'ok', 'result'} if body['ok'] else {'ok', 'error'})
        if body['ok']:
            require(isinstance(body['result'], dict), 'observer_response')
        else:
            require(isinstance(body['error'], str) and re.fullmatch('[a-z_]{1,48}', body['error']),
                    'observer_response')
        values = self.responses.setdefault(frame['id'], [])
        fingerprint = digest(frame)
        if not any(digest(item[0]) == fingerprint for item in values):
            require(len(values) < 3 and len(self.responses) <= 4096, 'observer_response_overflow')
            values.append((frame, proof))

    def _correlate(self):
        for rid, (request, request_row) in self.requests.items():
            if rid not in self.responses:
                continue
            for response, response_row in self.responses[rid]:
                key = (rid, digest(response))
                if key in self.done:
                    continue
                require(response['op'] == request['op'] and
                        response['challenge'] == request['challenge'], 'observer_response_binding')
                body, op = response['body'], request['op']
                if not body['ok']:
                    error = body['error']
                    if op == 'status' and request['body']['invocation'] == self.probe:
                        require(error in ('not_found', 'stale_challenge'), 'observer_probe')
                        if error == 'not_found':
                            self.cold = (request, request_row, response_row)
                    else:
                        require(op in ('hello', 'token') or error == 'stale_challenge', 'observer_operation_failed')
                else:
                    require(rid not in self.success or self.success[rid] == digest(response),
                            'observer_response_conflict')
                    self.success[rid] = digest(response)
                    self._accepted(request, body['result'], request_row, response_row)
                self.done.add(key)

    def _accepted(self, request, result, request_row, response_row):
        op, body = request['op'], request['body']
        evidence = (request, request_row, response_row)
        limits = {stream: len(self.expected[stream]) for stream in ('stdout', 'stderr')}
        if op == 'hello':
            _keys(result, {'nonce', 'challenge', 'lifetime'})
            require(result['nonce'] == body['nonce'] and type(result['lifetime']) is int and
                    result['lifetime'] == 60, 'observer_hello')
            challenge = hex_id(result['challenge'], 32)
            require(challenge not in self.hellos or
                    self.hellos[challenge][0] == request, 'observer_challenge_conflict')
            self.hellos[challenge] = evidence
        elif op == 'token':
            _keys(result, {'token'})
            self.tokens.add(hex_id(result['token'], 32))
        elif op == 'submit':
            _keys(result, {'disposition', 'status'})
            require(result['disposition'] in ('accepted', 'existing'), 'observer_admission')
            _status(result['status'], self.invocation, limits)
            if result['disposition'] == 'accepted':
                self.admission = evidence
        elif op == 'status':
            require(body['invocation'] != self.probe, 'observer_probe_existed')
            _status(result, self.invocation, limits)
        elif op == 'begin_input':
            _input(result)
            generation = hex_id(result['generation'])
            require(self.generation is None or self.generation == generation, 'observer_input_generation')
            self.generation = generation
        elif op == 'write_stdin':
            _keys(result, {'accepted', 'available_credit', 'duplicate'})
            data = unb64(body['bytes'])
            end = body['offset'] + len(data)
            _uint(result['accepted'], len(self.expected['stdin']))
            _uint(result['available_credit'])
            require(type(result['duplicate']) is bool and result['accepted'] >= end, 'observer_input_ack')
            self._append('stdin', body['offset'], data)
            entry = {'offset': body['offset'], **byte_proof(data), 'request': request_row, 'response': response_row}
            key = (body['generation'], body['offset'])
            if key in self.writes:
                require(self.writes[key]['sha256'] == entry['sha256'] and
                        self.writes[key]['bytes'] == entry['bytes'], 'observer_input_conflict')
            else:
                require(len(self.writes) < 16, 'observer_input_overflow')
                self.writes[key] = entry
        elif op == 'stdin_eof':
            _input(result)
            require(result['generation'] == body['generation'] and
                    body['final_offset'] == len(self.expected['stdin']) and
                    result['accepted'] == len(self.expected['stdin']), 'observer_eof')
            self.eof = evidence
        elif op == 'poll':
            _keys(result, {'status', 'stdout', 'stderr'})
            _status(result['status'], self.invocation, limits)
            finished = result['status']['state'] == 'exited'
            for stream in ('stdout', 'stderr'):
                chunk = result[stream]
                _keys(chunk, {'bytes', 'next_cursor', 'status'})
                _output(chunk['status'], limits[stream])
                data = unb64(chunk['bytes'])
                cursor = body[stream + '_cursor']
                require(type(chunk['next_cursor']) is int and chunk['next_cursor'] == cursor + len(data)
                        and chunk['status']['produced'] >= chunk['next_cursor'] and
                        chunk['status']['produced'] >= self.produced[stream], 'observer_output_cursor')
                self.produced[stream] = chunk['status']['produced']
                self._append(stream, cursor, data)
                finished &= chunk['status']['closed'] and chunk['next_cursor'] == limits[stream] == chunk['status']['produced']
            if finished:
                require(result['status']['input']['accepted'] == len(self.expected['stdin']) and
                        result['status']['input']['delivered'] == len(self.expected['stdin']) and
                        result['status']['input']['health'] == 'closed', 'observer_terminal_input')
                self.terminal = evidence
        elif op == 'cancel':
            self.cancelled = True

    def _append(self, stream, cursor, data):
        existing, expected = self.streams[stream], self.expected[stream]
        require(cursor <= len(existing) and cursor + len(data) <= len(expected) and
                expected[cursor:cursor + len(data)] == data, 'observer_stream_bytes')
        overlap = min(len(data), len(existing) - cursor)
        require(existing[cursor:cursor + overlap] == data[:overlap], 'observer_stream_conflict')
        existing.extend(data[overlap:])

    def proof(self):
        require(not self.failed and not self.cancelled and self.cold and self.admission and self.eof and self.terminal
                and self.generation and self.submit_body and self.submit_body['token'] in self.tokens,
                'observer_incomplete')
        for rid in self.responses:
            require(rid in self.requests, 'observer_orphan_response')
        for rid in self.success:
            request = self.requests[rid][0]
            require(request['op'] == 'hello' or request['challenge'] in self.hellos,
                    'observer_unproven_challenge')
        for evidence in (self.cold, self.admission, self.eof, self.terminal):
            require(evidence[0]['challenge'] in self.hellos, 'observer_unproven_challenge')
        require(all(bytes(self.streams[key]) == value for key, value in self.expected.items()) and
                all(generation == self.generation for generation, _offset in self.writes) and
                self.eof[0]['body']['generation'] == self.generation, 'observer_incomplete')
        hello = self.hellos[self.cold[0]['challenge']]
        proof = {'principal': self.client, 'group': self.group, 'session': self.session,
                 'invocation': self.invocation,
                 'probe': {'invocation': self.probe, 'hello_request': hello[1], 'hello_response': hello[2],
                           'request': self.cold[1], 'response': self.cold[2]},
                 'submit': {'request': self.admission[1], 'response': self.admission[2], 'count': 1},
                 'input': {**byte_proof(self.expected['stdin']), 'eof_offset': len(self.expected['stdin']),
                           'generation_sha256': hashlib.sha256(self.generation.encode()).hexdigest(),
                           'writes': list(self.writes.values()), 'eof_request': self.eof[1], 'eof_response': self.eof[2]},
                 **{stream: byte_proof(self.expected[stream]) for stream in ('stdout', 'stderr')},
                 'terminal': {'request': self.terminal[1], 'response': self.terminal[2],
                              'outcome': {'kind': 'code', 'value': 7},
                              **{stream + '_cursor': len(self.expected[stream]) for stream in ('stdout', 'stderr')}},
                 'rows': {'count': len(self.rows), 'sha256': digest(self.rows)}, 'unexpected_submissions': 0}
        validate_proof(proof, client=self.client, invocation=self.invocation, probe_invocation=self.probe,
                       **self.expected)
        return copy.deepcopy(proof)


def validate_proof(value, *, client, invocation, probe_invocation, stdin, stdout, stderr):
    """Closed sanitized proof contract shared with the nine-pair reconciler."""
    _keys(value, {'principal', 'group', 'session', 'invocation', 'probe', 'submit', 'input',
                  'stdout', 'stderr', 'terminal', 'rows', 'unexpected_submissions'})
    require(value['principal'] == client and value['invocation'] == invocation and
            type(value['unexpected_submissions']) is int and value['unexpected_submissions'] == 0,
            'observer_proof_binding')
    public_key(value['principal'])
    hex_id(value['invocation'])
    identifier(value['group'])
    hex_id(value['session'])
    _keys(value['probe'], {'invocation', 'hello_request', 'hello_response', 'request', 'response'})
    require(value['probe']['invocation'] == probe_invocation, 'observer_proof_binding')
    hex_id(value['probe']['invocation'])
    _keys(value['submit'], {'request', 'response', 'count'})
    require(type(value['submit']['count']) is int and value['submit']['count'] == 1, 'observer_proof_binding')
    _keys(value['terminal'], {'request', 'response', 'outcome', 'stdout_cursor', 'stderr_cursor'})
    _keys(value['terminal']['outcome'], {'kind', 'value'})
    require(value['terminal']['outcome'] == {'kind': 'code', 'value': 7} and
            type(value['terminal']['outcome'].get('value')) is int, 'observer_proof_binding')
    refs = [value['probe'][key] for key in ('hello_request', 'hello_response', 'request', 'response')]
    refs += [value[section][key] for section in ('submit', 'terminal') for key in ('request', 'response')]
    _keys(value['input'], {'bytes', 'sha256', 'eof_offset', 'generation_sha256', 'writes', 'eof_request', 'eof_response'})
    require({key: value['input'][key] for key in ('bytes', 'sha256')} == byte_proof(stdin) and
            type(value['input']['bytes']) is int and type(value['input']['eof_offset']) is int and
            value['input']['eof_offset'] == len(stdin), 'observer_proof_input')
    hex_id(value['input']['generation_sha256'], 32)
    writes = value['input']['writes']
    require(isinstance(writes, list) and 1 <= len(writes) <= 16 and
            all(isinstance(row, dict) for row in writes), 'observer_proof_input')
    for row in writes:
        _keys(row, {'offset', 'bytes', 'sha256', 'request', 'response'})
        _uint(row['offset'], len(stdin))
    offset = 0
    for row in sorted(writes, key=lambda row: row['offset']):
        require(type(row['offset']) is int and row['offset'] == offset and
                type(row['bytes']) is int and 0 < row['bytes'] <= 4096 and
                row['sha256'] == hashlib.sha256(stdin[offset:offset + row['bytes']]).hexdigest(), 'observer_proof_input')
        offset += row['bytes']
        refs.extend((row['request'], row['response']))
    require(offset == len(stdin), 'observer_proof_input')
    refs.extend((value['input']['eof_request'], value['input']['eof_response']))
    for ref in refs:
        _keys(ref, {'message_id', 'sha256'})
        identifier(ref['message_id'])
        hex_id(ref['sha256'], 32)
    require(len({ref['message_id'] for ref in refs}) == len(refs), 'observer_proof_rows')
    for stream, expected in (('stdout', stdout), ('stderr', stderr)):
        _keys(value[stream], {'bytes', 'sha256'})
        require(type(value[stream]['bytes']) is int and value[stream] == byte_proof(expected) and
                type(value['terminal'][stream + '_cursor']) is int and
                value['terminal'][stream + '_cursor'] == len(expected), 'observer_proof_output')
    _keys(value['rows'], {'count', 'sha256'})
    require(len(refs) <= _uint(value['rows']['count'], MAX_ROWS), 'observer_proof_rows')
    hex_id(value['rows']['sha256'], 32)
    require(len(canonical_bytes(value)) <= 32768, 'observer_proof_limit')
    return value


class DaemonObserver:
    """Discovery metadata selects rooms; MDK authenticates every selected room."""
    def __init__(self, rpc, *, account, expected, profile_path, clock=continuous_time):
        self.rpc, self.account, self.expected = rpc, public_key(account), expected
        require(isinstance(expected, dict) and len(expected) == 3 and
                all(public_key(peer) != account for peer in expected), 'observer_expected_principals')
        self.profile_path, self.clock = profile_path, clock
        self.pairs, self.checked = {}, {}
        self.last_tick = None
        self.failed = False
        self.finalized = False

    def tick(self, final=False):
        require(not self.failed, 'observer_previous_failure')
        try:
            self._tick(final)
        except Exception:
            self.failed = True
            raise

    def _tick(self, final):
        require(type(final) is bool, 'observer_schema')
        self.finalized = False
        now = self.clock()
        # Nine serialized RPCs may each consume the two-second RPC timeout.
        # Actual loss remains guarded by each pair's retained history overlap.
        require(self.last_tick is None or 0 <= now - self.last_tick <= 20, 'observer_sampling_gap')
        self.last_tick = now
        profile = strict_json(read_private(self.profile_path, 1024 * 1024))
        require(isinstance(profile, dict) and profile.get('account') == self.account,
                'observer_profile_binding')
        quarantine, full = profile.get('server_quarantine', {}), profile.get('server_quarantine_full', False)
        require(isinstance(quarantine, dict) and not quarantine and type(full) is bool and
                not full, 'observer_quarantine')
        groups, pending = profile.get('server_groups', {}), profile.get('server_pending', {})
        require(isinstance(groups, dict) and len(groups) <= 3 and
                isinstance(pending, dict) and len(pending) <= 3, 'observer_group_limit')
        for group, binding in pending.items():
            identifier(group)
            _keys(binding, {'peer', 'session'})
            hex_id(binding['session'])
            require(binding['peer'] in self.expected, 'observer_unexpected_principal')
        require(set(self.pairs) <= set(groups), 'observer_disappeared_group')
        for group, binding in groups.items():
            _keys(binding, {'peer', 'session'})
            peer = binding['peer']
            require(peer in self.expected, 'observer_unexpected_principal')
            if group not in self.pairs:
                require(not any(pair.client == peer for pair in self.pairs.values()), 'observer_duplicate_room')
                self.pairs[group] = PairObserver(group=group, session=binding['session'], client=peer,
                                                 server=self.account, **self.expected[peer])
            pair = self.pairs[group]
            require(pair.client == peer and pair.session == binding['session'], 'observer_binding_changed')
            if final or group not in self.checked or now - self.checked[group] >= 15:
                shown = self.rpc.show_group(self.account, group)
                room = shown.get('group')
                require(group_session(room) == pair.session and
                        room.get('pending_confirmation') is False, 'observer_room')
                check_members(self.rpc.members(self.account, group), self.account, peer)
                self.checked[group] = now
            pair.scan(self.rpc.history(self.account, group))
        self.finalized = final

    def proofs(self):
        require(not self.failed and self.finalized and len(self.pairs) == 3, 'observer_incomplete')
        return {pair.client: pair.proof() for pair in self.pairs.values()}

    def proof_for(self, client):
        require(not self.failed and self.finalized, 'observer_incomplete')
        matches = [pair for pair in self.pairs.values() if pair.client == client]
        require(len(matches) == 1, 'observer_incomplete')
        return matches[0].proof()
