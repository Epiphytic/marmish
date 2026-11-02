"""Strict public-only three-platform rendezvous for installed acceptance."""
import argparse
import base64
import hashlib
import math
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

from marmish_installed_release import ROLE_TARGETS, ROLES, RELAYS, canonical_bytes, validate_context
from marmish_platform import continuous_time
from marmish_rendezvous import NoRedirect, read_zip
from marmish_rpc import RpcError, strict_json
from mdk_smoke import SmokeError, require

HEX = re.compile(r'[0-9a-f]{64}')
API = 'https://api.github.com/repos/Epiphytic/marmish'
IDENTITY_KEYS = {'format', 'kind', 'run', 'attempt', 'sha', 'pin_sha256', 'role',
                 'target', 'nonce', 'server', 'client', 'relays', 'installation',
                 'interpreter', 'timing', 'published_at', 'expires_at'}
READY_KEYS = {'ready', 'identity_ready_sha256', 'fixtures', 'server_started_at'}
TIMING_STAGES = {'install', 'verify', 'identity_server', 'identity_client', 'barrier'}
REQUIRED_STAGES = TIMING_STAGES - {'barrier'}
ROOT = re.compile(r'/(?:private/)?tmp/mci-[A-Za-z0-9_-]{6,16}')
TRANSIENT = {'artifact_network_failed', 'artifact_download_transient', 'artifact_request_timeout'}


def context():
    require(os.environ.get('GITHUB_REPOSITORY') == 'Epiphytic/marmish' and
            os.environ.get('GITHUB_REF') == 'refs/heads/master' and
            os.environ.get('GITHUB_EVENT_NAME') == 'workflow_dispatch', 'invalid_ci_context')
    value = {key: os.environ.get(env, '') for key, env in
             [('run', 'GITHUB_RUN_ID'), ('attempt', 'GITHUB_RUN_ATTEMPT'), ('sha', 'GITHUB_SHA')]}
    _context(value)
    return value


def _context(ctx):
    try:
        validate_context(ctx)
    except ValueError:
        raise SmokeError('invalid_ci_context') from None


def artifact_name(ctx, role, stage):
    _context(ctx)
    require(role in ROLES and stage in ('identity-ready', 'server-ready'), 'invalid_artifact_name')
    return f"installed-interop-{ctx['run']}-{ctx['attempt']}-{role}-{stage}"


def pin_digest(pin):
    return hashlib.sha256(canonical_bytes(pin)).hexdigest()


def installation_tuple(pin, role):
    row = pin['targets'][role]
    return {key: row[key] for key in ('target', 'archive_sha256', 'manifest_sha256')} | {
        key: pin[key] for key in ('version', 'source_commit', 'installer_sha256')}


def _hex(value):
    return isinstance(value, str) and HEX.fullmatch(value) is not None


def validate_timing(value, *, require_valid=False):
    require(isinstance(value, dict) and set(value) == {'unit', 'clock', 'stages'} and
            value['unit'] == 'ns' and value['clock'] == 'continuous+monotonic' and
            isinstance(value['stages'], dict) and set(value['stages']) <= TIMING_STAGES,
            'invalid_timing')
    for row in value['stages'].values():
        require(isinstance(row, dict) and set(row) == {
            'elapsed_ns', 'active_ns', 'suspended_ns', 'clock_valid'} and
            type(row['clock_valid']) is bool and all(type(row[key]) is int and
            0 <= row[key] <= 4_620_000_000_000 for key in
            ('elapsed_ns', 'active_ns', 'suspended_ns')) and
            row['suspended_ns'] == max(0, row['elapsed_ns'] - row['active_ns']) and
            (not row['clock_valid'] or row['suspended_ns'] <= 5_000_000_000) and
            (not require_valid or row['clock_valid']), 'invalid_timing')
    return value


def validate_identity(record, ctx, pin, role, *, now):
    _context(ctx)
    require(role in ROLES and isinstance(record, dict) and set(record) == IDENTITY_KEYS,
            'invalid_identity_record')
    require(type(record['format']) is int and record['format'] == 1 and
            record['kind'] == 'identity-ready' and record['role'] == role and
            all(record[key] == ctx[key] for key in ('run', 'attempt', 'sha')) and
            record['pin_sha256'] == pin_digest(pin) and record['target'] == ROLE_TARGETS[role] and
            record['relays'] == RELAYS and record['installation'] == installation_tuple(pin, role),
            'identity_provenance_mismatch')
    require(all(_hex(record[key]) for key in ('nonce', 'server', 'client')) and
            record['server'] != record['client'], 'invalid_identity_record')
    interpreter = record['interpreter']
    require(isinstance(interpreter, dict) and set(interpreter) == {
        'implementation', 'version', 'path_class'} and interpreter['implementation'] == 'cpython' and
        interpreter['path_class'] in ('system', 'homebrew', 'ci-bootstrap') and
        isinstance(interpreter['version'], list) and len(interpreter['version']) == 3 and
        all(type(part) is int and 0 <= part <= 999 for part in interpreter['version']) and
        interpreter['version'] >= [3, 11, 0], 'invalid_interpreter_record')
    require(type(now) is int and type(record['published_at']) is int and
            type(record['expires_at']) is int and 0 < record['published_at'] <= now + 60 and
            now < record['expires_at'] and
            0 < record['expires_at'] - record['published_at'] <= 4200, 'expired_identity_record')
    validate_timing(record['timing'], require_valid=True)
    require(REQUIRED_STAGES <= set(record['timing']['stages']), 'incomplete_identity_timing')
    require(len(canonical_bytes(record)) <= 8192, 'artifact_oversized')
    return record


def identity_barrier(records, ctx, pin, *, now):
    require(isinstance(records, dict) and set(records) == set(ROLES), 'incomplete_identity_barrier')
    for role in ROLES:
        validate_identity(records[role], ctx, pin, role, now=now)
    require(len({records[role][key] for role in ROLES for key in ('server', 'client')}) == 6 and
            len({records[role]['nonce'] for role in ROLES}) == 3, 'duplicate_barrier_identity')
    return records


validate_identities = identity_barrier


def validate_server_ready(record, ctx, pin, role, identities, *, now):
    identity_barrier(identities, ctx, pin, now=now)
    require(isinstance(record, dict) and set(record) == IDENTITY_KEYS | READY_KEYS,
            'invalid_server_record')
    base = {key: record[key] for key in IDENTITY_KEYS}
    base['kind'] = 'identity-ready'
    validate_identity(base, ctx, pin, role, now=now)
    require(record['kind'] == 'server-ready' and record['ready'] is True and
            all(base[key] == identities[role][key] for key in IDENTITY_KEYS - {
                'kind', 'published_at', 'expires_at', 'timing'}) and
            record['identity_ready_sha256'] == {
                item: hashlib.sha256(canonical_bytes(identities[item])).hexdigest() for item in ROLES},
            'server_identity_mismatch')
    require(type(record['server_started_at']) is int and
            0 < record['server_started_at'] <= record['published_at'] + 60 and
            isinstance(record['fixtures'], dict) and set(record['fixtures']) == set(ROLES),
            'invalid_server_record')
    roots = set()
    for client_role, fixture in record['fixtures'].items():
        require(isinstance(fixture, dict) and set(fixture) == {'path', 'sha256'} and
                isinstance(fixture['path'], str) and _hex(fixture['sha256']), 'invalid_server_fixture')
        suffix = '/f/' + client_role + '/main'
        path = fixture['path']
        root = path[:-len(suffix)] if path.endswith(suffix) else ''
        require(ROOT.fullmatch(root) and root.startswith(
            '/private/tmp/' if role == 'macos-arm64' else '/tmp/'), 'invalid_server_fixture')
        roots.add(root)
    require(len(roots) == 1 and len(canonical_bytes(record)) <= 8192, 'invalid_server_fixture')
    return record


def _valid_url(url, token):
    require(isinstance(url, str) and 0 < len(url) <= 16384 and url.isascii() and
            not any(ord(char) <= 32 for char in url), 'invalid_artifact_url')
    try:
        parts = urllib.parse.urlsplit(url)
        host = parts.hostname or ''
        api = host == 'api.github.com' and parts.path.startswith('/repos/Epiphytic/marmish/')
        signed = host.endswith(('.blob.core.windows.net', '.actions.githubusercontent.com'))
        require(parts.scheme == 'https' and parts.port in (None, 443) and
                parts.username is None and parts.password is None and not parts.fragment and
                (api or signed) and (token is None or api), 'invalid_artifact_url')
    except ValueError:
        raise SmokeError('invalid_artifact_url') from None


def _exchange(config):
    """Network-only worker; its parent provides the total time/pipe bound."""
    headers = {'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'}
    if config['token'] is not None:
        headers['Authorization'] = 'Bearer ' + config['token']
    remaining = config['deadline'] - continuous_time()
    require(remaining > 0, 'artifact_request_timeout')
    opener = urllib.request.build_opener(NoRedirect)
    try:
        with opener.open(urllib.request.Request(config['url'], headers=headers),
                         timeout=min(15, remaining)) as response:
            chunks, total = [], 0
            while True:
                require(continuous_time() < config['deadline'], 'artifact_request_timeout')
                chunk = response.read1(min(65536, config['limit'] + 1 - total))
                if not chunk:
                    break
                total += len(chunk)
                require(total <= config['limit'], 'artifact_oversized')
                chunks.append(chunk)
            return {'status': response.status, 'body': base64.b64encode(b''.join(chunks)).decode(),
                    'location': None}
    except urllib.error.HTTPError as error:
        try:
            location = error.headers.get('Location')
            require(location is None or len(location) <= 16384, 'invalid_artifact_redirect')
            return {'status': error.code, 'body': '', 'location': location}
        finally:
            error.close()
    except (urllib.error.URLError, TimeoutError, OSError):
        raise SmokeError('artifact_network_failed') from None


def _request_process(config):
    """Run a fixed adapter child; credentials travel only over its stdin pipe."""
    directory = str(Path(__file__).resolve().parent)
    code = ('import runpy,sys;sys.path.insert(0,sys.argv[1]);'
            'sys.argv=sys.argv[2:];runpy.run_path(sys.argv[0],run_name="__main__")')
    child = subprocess.Popen([sys.executable, '-I', '-B', '-c', code, directory,
                              str(Path(__file__).resolve()), '--http-worker'],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                             env={'PATH': '/usr/bin:/bin', 'LANG': 'C'}, close_fds=True,
                             start_new_session=True)
    selector = selectors.DefaultSelector()
    payload, offset, output = canonical_bytes(config), 0, bytearray()
    for pipe in (child.stdin, child.stdout):
        os.set_blocking(pipe.fileno(), False)
    selector.register(child.stdin, selectors.EVENT_WRITE)
    selector.register(child.stdout, selectors.EVENT_READ)
    try:
        while True:
            require(continuous_time() < config['deadline'], 'artifact_request_timeout')
            for key, _ in selector.select(min(.05, max(0, config['deadline'] - continuous_time()))):
                if key.fileobj is child.stdin:
                    offset += os.write(key.fd, payload[offset:offset + 4096])
                    if offset == len(payload):
                        selector.unregister(child.stdin)
                        child.stdin.close()
                else:
                    data = os.read(key.fd, 65536)
                    if not data:
                        selector.unregister(child.stdout)
                        child.stdout.close()
                    output.extend(data)
                    require(len(output) <= config['limit'] * 2 + 32768, 'artifact_oversized')
            if child.stdout.closed and os.waitid(
                    os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None:
                break
    except (OSError, ValueError):
        raise SmokeError('artifact_network_failed') from None
    finally:
        # The unreaped leader reserves its process group through the last signal.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            raise SmokeError('artifact_worker_cleanup_failed') from None
        finally:
            selector.close()
            for pipe in (child.stdin, child.stdout):
                if not pipe.closed:
                    pipe.close()
        try:
            os.killpg(child.pid, 0)
        except ProcessLookupError:
            pass
        else:
            raise SmokeError('artifact_worker_cleanup_failed')
    try:
        value = strict_json(bytes(output))
        if isinstance(value, dict) and set(value) == {'error'}:
            require(value['error'] in {'artifact_network_failed', 'artifact_request_timeout',
                                      'artifact_oversized', 'invalid_artifact_redirect'},
                    'artifact_worker_failed')
            raise SmokeError(value['error'])
        require(child.returncode == 0 and isinstance(value, dict) and
                set(value) == {'status', 'body', 'location'} and type(value['status']) is int and
                100 <= value['status'] <= 599 and isinstance(value['body'], str) and
                (value['location'] is None or isinstance(value['location'], str) and
                 len(value['location']) <= 16384), 'artifact_worker_failed')
        data = base64.b64decode(value['body'], validate=True)
        require(len(data) <= config['limit'], 'artifact_oversized')
        return value['status'], data, value['location']
    except (RpcError, ValueError, TypeError):
        raise SmokeError('artifact_worker_failed') from None


def request_bounded(url, token, limit, deadline):
    _valid_url(url, token)
    require(token is None or isinstance(token, str) and 0 < len(token) <= 4096 and
            '\n' not in token and '\r' not in token, 'invalid_artifact_token')
    require(type(limit) is int and 1 <= limit <= 262144 and
            type(deadline) in (int, float) and math.isfinite(deadline), 'invalid_artifact_request')
    return _request_process({'url': url, 'token': token, 'limit': limit,
                             'deadline': min(deadline, continuous_time() + 15)})


def _listing(ctx, token, deadline):
    for _restart in range(3):
        artifacts, seen, expected = [], set(), None
        for page in range(1, 11):
            status, raw, _location = request_bounded(
                API + f"/actions/runs/{ctx['run']}/artifacts?per_page=100&page={page}",
                token, 262144, deadline)
            require(status != 429 and not 500 <= status <= 599, 'artifact_download_transient')
            require(status == 200, 'artifact_api_failed')
            body = strict_json(raw)
            require(isinstance(body, dict) and type(body.get('total_count')) is int and
                    0 <= body['total_count'] <= 1000 and isinstance(body.get('artifacts'), list) and
                    len(body['artifacts']) <= 100, 'invalid_artifact_list')
            expected = body['total_count'] if expected is None else expected
            items = body['artifacts']
            require(all(isinstance(item, dict) and type(item.get('id')) is int and item['id'] > 0
                        and isinstance(item.get('name'), str) and item.get('expired') is False
                        and type(item.get('size_in_bytes')) is int and
                        0 <= item['size_in_bytes'] <= 2**63 - 1 and isinstance(item.get('digest'), str)
                        and re.fullmatch(r'sha256:[0-9a-f]{64}', item['digest']) for item in items),
                    'invalid_artifact_list')
            if body['total_count'] != expected or any(item['id'] in seen for item in items):
                break
            seen.update(item['id'] for item in items)
            if len(seen) != len(artifacts) + len(items):
                break
            artifacts.extend(items)
            if len(artifacts) == expected:
                return artifacts
            if len(items) < 100 or len(artifacts) > expected:
                break
    raise SmokeError('artifact_listing_unstable')


def _archive(item, wanted, token, deadline):
    # The resolve artifact can exceed this bound; only peer records use this ZIP reader.
    require(item.get('name') == wanted and type(item.get('id')) is int and item['id'] > 0 and
            item.get('expired') is False and type(item.get('size_in_bytes')) is int and
            0 <= item['size_in_bytes'] <= 65536 and isinstance(item.get('digest'), str) and
            re.fullmatch(r'sha256:[0-9a-f]{64}', item['digest']), 'invalid_peer_artifact')
    status, raw, location = request_bounded(API + f"/actions/artifacts/{item['id']}/zip", token, 65536, deadline)
    if status == 302:
        _valid_url(location, None)
        require(urllib.parse.urlsplit(location).hostname != 'api.github.com', 'invalid_artifact_redirect')
        status, raw, _location = request_bounded(location, None, 65536, deadline)
    require(status != 429 and not 500 <= status <= 599, 'artifact_download_transient')
    require(status == 200, 'artifact_download_failed')
    return read_zip(raw, item['digest'])


def wait_records(ctx, pin, stage, roles, *, deadline, output, token=None, identities=None):
    _context(ctx)
    require(tuple(roles) == tuple(ROLES) and stage in ('identity-ready', 'server-ready'), 'invalid_barrier')
    require(isinstance(token, str) and token, 'missing_artifact_token')
    while continuous_time() < deadline:
        try:
            artifacts = _listing(ctx, token, deadline)
            records = {}
            for role in ROLES:
                wanted = artifact_name(ctx, role, stage)
                matches = [item for item in artifacts if item['name'] == wanted]
                require(len(matches) <= 1, 'duplicate_peer_artifact')
                if matches:
                    value = _archive(matches[0], wanted, token, deadline)
                    if stage == 'identity-ready':
                        validate_identity(value, ctx, pin, role, now=int(time.time()))
                    else:
                        validate_server_ready(value, ctx, pin, role, identities, now=int(time.time()))
                    records[role] = value
            if len(records) == len(ROLES):
                if stage == 'identity-ready':
                    identity_barrier(records, ctx, pin, now=int(time.time()))
                require(continuous_time() < deadline, 'barrier_timeout')
                from marmish_installed_state import write_private
                write_private(Path(output), records)
                return records
        except SmokeError as error:
            if str(error) not in TRANSIENT:
                raise
        time.sleep(min(3, max(0, deadline - continuous_time())))
    raise SmokeError('barrier_timeout')


def main():
    if sys.argv[1:] == ['--http-worker']:
        try:
            raw = sys.stdin.buffer.read(32769)
            require(len(raw) <= 32768, 'artifact_worker_failed')
            config = strict_json(raw)
            require(isinstance(config, dict) and set(config) == {'url', 'token', 'limit', 'deadline'},
                    'artifact_worker_failed')
            result = _exchange(config)
        except SmokeError as error:
            category = str(error)
            result = {'error': category if category in TRANSIENT | {
                'artifact_oversized', 'invalid_artifact_redirect'} else 'artifact_network_failed'}
        except Exception:
            result = {'error': 'artifact_network_failed'}
        sys.stdout.buffer.write(canonical_bytes(result))
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('wait',))
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--pin-dir', type=Path, required=True)
    parser.add_argument('--stage', choices=('identity-ready', 'server-ready'), required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--identities', type=Path)
    parser.add_argument('--timeout', type=int, default=600)
    args = parser.parse_args()
    status = None
    status_path = None
    try:
        require(1 <= args.timeout <= 600, 'invalid_timeout')
        from marmish_installed_release import load_pin
        from marmish_installed_state import load_state
        state = load_state(args.state)
        ctx = context()
        require(state['ctx'] == ctx, 'invalid_ci_context')
        pin = load_pin(args.pin_dir, ctx)
        require(state['pin_sha256'] == pin_digest(pin), 'pin_identity_changed')
        from marmish_installed_state import read_private
        identities = strict_json(read_private(args.identities, 32768)) if args.identities else None
        require(args.output.parent == Path(state['output']) or
                args.output.parent == Path(state['root']), 'unsafe_state_path')
        status_path = args.output.with_name(args.output.name + '.status.json')
        status = {'format': 1, 'kind': 'adapter-result', **ctx,
                  'pin_sha256': pin_digest(pin), 'role': state['role'],
                  'stage': args.stage, 'outcome': 'passed', 'error': None}
        wait_records(ctx, pin, args.stage, ROLES, deadline=min(
            state['barrier_deadline'], continuous_time() + args.timeout), output=args.output,
            token=os.environ.get('GITHUB_TOKEN'), identities=identities)
        from marmish_installed_state import write_private
        write_private(status_path, status)
        return 0
    except (SmokeError, RpcError, OSError, ValueError, TypeError, KeyError) as error:
        if status is not None:
            category = str(error) if isinstance(error, SmokeError) else 'artifact_invalid'
            if category not in {'barrier_timeout', 'artifact_network_failed',
                                'artifact_listing_unstable'}:
                category = 'artifact_invalid'
            status.update(outcome='failed', error=category)
            try:
                from marmish_installed_state import write_private
                write_private(status_path, status)
            except (OSError, SmokeError):
                pass  # No trusted writable destination; the barrier fails missing evidence.
        print('{"outcome":"failed","error":"rendezvous_failed"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
