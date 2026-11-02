"""Credential-isolated, bounded delivery of installed client result artifacts."""
import argparse
import hashlib
import io
import math
import os
from pathlib import Path
import re
import stat
import sys
import time
import urllib.parse
import zipfile

from marmish_installed_release import ROLES, canonical_bytes, load_pin
from marmish_installed_rendezvous import (
    API, TRANSIENT, _listing, _valid_url, context, identity_barrier, pin_digest,
    request_bounded, validate_server_ready,
)
from marmish_installed_state import load_state, read_private, write_private
from marmish_platform import continuous_time
from marmish_rpc import RpcError, strict_json
from mdk_smoke import SmokeError, require

ZIP_LIMIT = 131072
RECORD_LIMIT = 65536


def artifact_name(ctx, role):
    require(role in ROLES, 'invalid_artifact_name')
    return f"installed-interop-{ctx['run']}-{ctx['attempt']}-{role}-client-result"


def read_terminal_zip(data, digest):
    """Validate the authenticated ZIP before parsing its one bounded JSON record."""
    require(isinstance(data, bytes) and len(data) <= ZIP_LIMIT, 'artifact_oversized')
    require(isinstance(digest, str) and re.fullmatch(r'sha256:[0-9a-f]{64}', digest) and
            digest == 'sha256:' + hashlib.sha256(data).hexdigest(), 'artifact_digest_mismatch')
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            entries = archive.infolist()
            require(len(entries) == 1, 'invalid_artifact_archive')
            entry = entries[0]
            mode = entry.external_attr >> 16
            require(entry.filename == 'record.json' and not entry.is_dir() and
                    (not stat.S_IFMT(mode) or stat.S_ISREG(mode)) and
                    entry.file_size <= RECORD_LIMIT and entry.compress_size <= ZIP_LIMIT and
                    entry.extract_version < 45 and not entry.flag_bits & (1 | 64) and
                    entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                    'invalid_artifact_archive')
            extra = entry.extra
            while extra:
                require(len(extra) >= 4, 'invalid_artifact_archive')
                kind, size = int.from_bytes(extra[:2], 'little'), int.from_bytes(extra[2:4], 'little')
                require(kind != 1 and len(extra) >= size + 4, 'invalid_artifact_archive')
                extra = extra[size + 4:]
            with archive.open(entry) as source:
                raw = source.read(RECORD_LIMIT + 1)
            require(len(raw) <= RECORD_LIMIT, 'artifact_oversized')
            require(not raw.startswith(b'\xef\xbb\xbf'), 'invalid_artifact_archive')
            value = strict_json(raw)
            require(isinstance(value, dict), 'invalid_terminal_report')
            return value
    except (ValueError, OSError, zipfile.BadZipFile, RuntimeError, RpcError, EOFError):
        raise SmokeError('invalid_artifact_archive') from None


def download(item, wanted, token, deadline):
    require(item.get('name') == wanted and type(item.get('id')) is int and item['id'] > 0 and
            item.get('expired') is False and type(item.get('size_in_bytes')) is int and
            0 <= item['size_in_bytes'] <= ZIP_LIMIT and isinstance(item.get('digest'), str) and
            re.fullmatch(r'sha256:[0-9a-f]{64}', item['digest']), 'invalid_peer_artifact')
    status, raw, location = request_bounded(
        API + f"/actions/artifacts/{item['id']}/zip", token, ZIP_LIMIT, deadline)
    if status == 302:
        _valid_url(location, None)
        require(urllib.parse.urlsplit(location).hostname != 'api.github.com', 'invalid_artifact_redirect')
        status, raw, _unused = request_bounded(location, None, ZIP_LIMIT, deadline)
    require(status != 429 and not 500 <= status <= 599, 'artifact_download_transient')
    require(status == 200, 'artifact_download_failed')
    return read_terminal_zip(raw, item['digest'])


def validate_report(report, state, pin, role, identities, servers):
    from marmish_installed_fixture import derive_pair, expected_streams, stdin_bytes
    from marmish_installed_runtime import payload_record
    from marmish_nine_pair import validate_terminal
    validate_terminal(report, 'client-result', state['ctx'], pin, role, identities, servers)
    for server_role in ROLES:
        slot = report['slots'][server_role]
        pair = derive_pair(state['ctx'], state['pin_sha256'], identities[role], identities[server_role])
        require(all(slot[key] == pair[key] for key in ('context', 'invocation', 'probe_invocation')) and
                slot['client'] == identities[role]['client'] and
                slot['server'] == identities[server_role]['server'], 'terminal_pair_mismatch')
        stdout, stderr = expected_streams(servers[server_role]['fixtures'][role]['path'],
                                         pair['context'], pair['challenge'], pair['invocation'])
        if slot['payload'] is not None:
            require(slot['payload'] == payload_record(stdin_bytes(pair['context']), stdout, stderr),
                    'terminal_payload_mismatch')
        else:
            # A validated failure still needs delivery so servers can terminate
            # promptly. validate_terminal rejects absent payloads on success.
            require(slot['status'] != 'received' and slot['metrics'] is None,
                    'terminal_payload_mismatch')
    require(len(canonical_bytes(report)) <= RECORD_LIMIT, 'artifact_oversized')
    return report


def live_deadline(state, *, clock=continuous_time, allow_expired=False):
    value = strict_json(read_private(Path(state['root']) / 'live-start.json', 2048))
    require(isinstance(value, dict) and set(value) == {'start', 'deadline', 'readiness_deadline'} and
            isinstance(value['start'], dict) and set(value['start']) == {
                'continuous_ns', 'monotonic_ns', 'wall_ns'} and all(
                type(part) is int and part > 0 for part in value['start'].values()) and all(
                type(value[key]) in (int, float) and math.isfinite(value[key])
                for key in ('deadline', 'readiness_deadline')), 'invalid_live_deadline')
    start = value['start']['continuous_ns'] / 1e9
    deadline = value['deadline']
    require(start <= clock() and start < value['readiness_deadline'] < deadline and
            deadline <= start + 1800 and deadline <= state['overall_deadline'] and
            (allow_expired or clock() < deadline), 'terminal_timeout')
    return deadline


def _deliver(state, role, report):
    raw = canonical_bytes(report)
    output = Path(state['output'])
    if role == state['role']:
        require(read_private(output / 'client-result.json', RECORD_LIMIT) == raw,
                'local_terminal_mismatch')
    inbox = output / 'client-inbox'
    inbox.mkdir(mode=0o700, exist_ok=True)
    metadata = inbox.lstat()
    require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid == os.getuid() and
            stat.S_IMODE(metadata.st_mode) == 0o700, 'unsafe_state_path')
    target = inbox / (role + '.json')
    if os.path.lexists(target):
        require(read_private(target, RECORD_LIMIT) == raw, 'terminal_changed')
    else:
        write_private(target, report)


def wait_reports(state, pin, identities, servers, token, *, clock=continuous_time, sleep=time.sleep):
    require(isinstance(token, str) and 0 < len(token) <= 4096 and
            '\n' not in token and '\r' not in token, 'missing_artifact_token')
    require(state['pin_sha256'] == pin_digest(pin), 'pin_identity_changed')
    identity_barrier(identities, state['ctx'], pin, now=int(time.time()))
    require(isinstance(servers, dict) and set(servers) == set(ROLES), 'invalid_server_record')
    for role in ROLES:
        validate_server_ready(servers[role], state['ctx'], pin, role, identities, now=int(time.time()))
    deadline = live_deadline(state, clock=clock)
    delivered, artifacts_seen = {}, {}
    while clock() < deadline:
        try:
            artifacts = _listing(state['ctx'], token, deadline)
            matches = {role: [item for item in artifacts if item['name'] == artifact_name(state['ctx'], role)]
                       for role in ROLES}
            require(all(len(items) <= 1 for items in matches.values()), 'duplicate_peer_artifact')
            for role, items in matches.items():
                require(clock() < deadline, 'terminal_timeout')
                if not items:
                    require(role not in delivered, 'terminal_artifact_disappeared')
                    continue
                item = items[0]
                fingerprint = tuple(item[key] for key in ('id', 'digest', 'size_in_bytes'))
                if role in delivered:
                    require(artifacts_seen[role] == fingerprint, 'terminal_changed')
                    continue
                report = download(item, artifact_name(state['ctx'], role), token, deadline)
                validate_report(report, state, pin, role, identities, servers)
                require(clock() < deadline, 'terminal_timeout')
                _deliver(state, role, report)
                delivered[role], artifacts_seen[role] = report, fingerprint
            if len(delivered) == len(ROLES):
                return delivered
        except SmokeError as error:
            if str(error) not in TRANSIENT:
                raise
        sleep(min(3, max(0, deadline - clock())))
    raise SmokeError('terminal_timeout')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('state', 'pin-dir', 'identities', 'servers'):
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args(argv)
    status, path = None, None
    try:
        state = load_state(args.state)
        ctx = context()
        require(state['ctx'] == ctx, 'invalid_ci_context')
        pin = load_pin(args.pin_dir, ctx)
        path = Path(state['output']) / 'client-inbox-status.json'
        status = {'format': 1, 'kind': 'adapter-result', **ctx, 'pin_sha256': pin_digest(pin),
                  'role': state['role'], 'stage': 'client-result', 'outcome': 'passed', 'error': None}
        identities = strict_json(read_private(args.identities, 32768))
        servers = strict_json(read_private(args.servers, 32768))
        token = os.environ.pop('GITHUB_TOKEN', None)
        wait_reports(state, pin, identities, servers, token)
        write_private(path, status)
        return 0
    except Exception as error:
        if status is not None:
            category = str(error) if isinstance(error, SmokeError) else 'artifact_invalid'
            if category not in {'terminal_timeout', 'artifact_network_failed', 'artifact_listing_unstable'}:
                category = 'artifact_invalid'
            status.update(outcome='failed', error=category)
            try:
                write_private(path, status)
            except Exception:
                pass  # Missing writable trusted state remains missing failure evidence.
        print('{"outcome":"failed","error":"terminal_adapter_failed"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
