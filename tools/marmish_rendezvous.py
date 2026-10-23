"""Bounded public-only Actions artifact rendezvous; token never enters MDK."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile

from marmish_platform import continuous_time
from mdk_smoke import SmokeError, require
from marmish_rpc import strict_json, RpcError

RELAYS = ['wss://relay.eu.whitenoise.chat/', 'wss://relay.us.whitenoise.chat/']
ROLES = ('linux', 'macos')
HEX = re.compile(r'[0-9a-f]{64}')
ROOT = re.compile(r'/(?:private/)?tmp/mci-[A-Za-z0-9_-]{6,16}')
BASE_KEYS = {'version', 'run', 'attempt', 'sha', 'role', 'nonce', 'server', 'client',
             'relays', 'fixture', 'fixture_sha256', 'intruder', 'platform', 'published_at', 'expires_at'}


def context():
    require(os.environ.get('GITHUB_REPOSITORY') == 'Epiphytic/marmish', 'invalid_ci_context')
    value = {name: os.environ.get(env, '') for name, env in
             [('run', 'GITHUB_RUN_ID'), ('attempt', 'GITHUB_RUN_ATTEMPT'), ('sha', 'GITHUB_SHA')]}
    require(value['run'].isdigit() and value['attempt'].isdigit() and
            re.fullmatch(r'[0-9a-f]{40}', value['sha']) is not None, 'invalid_ci_context')
    return value


def name(ctx, role, stage):
    require(role in ROLES and stage in ('ready', 'paired'), 'invalid_artifact_name')
    return f"interop-{ctx['run']}-{ctx['attempt']}-{role}-{stage}"


def validate(value, ctx, role, stage):
    require(isinstance(value, dict) and set(value) == BASE_KEYS | (
        {'peer_nonce', 'peer_server', 'peer_client', 'ready'} if stage == 'paired' else set()),
        'invalid_peer_metadata')
    require(value['version'] == 1 and type(value['version']) is int and
            all(value[key] == ctx[key] for key in ('run', 'attempt', 'sha')) and
            value['role'] == role and value['relays'] == RELAYS,
            'invalid_peer_metadata')
    for key in ('nonce', 'server', 'client', 'intruder', 'fixture_sha256'):
        require(isinstance(value[key], str) and HEX.fullmatch(value[key]), 'invalid_peer_metadata')
    require(len({value['server'], value['client'], value['intruder']}) == 3, 'invalid_peer_metadata')
    require(isinstance(value['fixture'], str) and value['fixture'].endswith('/f/main') and
            ROOT.fullmatch(value['fixture'][:-7]) and value['platform'] == {'linux': 'linux', 'macos': 'darwin'}[role],
            'invalid_peer_metadata')
    require(type(value['published_at']) is int and type(value['expires_at']) is int and
            0 < value['expires_at'] - value['published_at'] <= 4200,
            'expired_peer_metadata')
    if stage == 'paired':
        require(value['ready'] is True, 'peer_not_ready')
        for key in ('peer_nonce', 'peer_server', 'peer_client'):
            require(isinstance(value[key], str) and HEX.fullmatch(value[key]), 'invalid_peer_metadata')
    return value


def read_zip(data, digest=None):
    require(len(data) <= 65536, 'artifact_oversized')
    if digest is not None:
        require(digest == 'sha256:' + hashlib.sha256(data).hexdigest(), 'artifact_digest_mismatch')
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            entries = archive.infolist()
            require(len(entries) == 1 and entries[0].filename == 'record.json' and
                    entries[0].file_size <= 8192 and entries[0].extract_version < 45 and
                    not entries[0].flag_bits & 1, 'invalid_artifact_archive')
            extra = entries[0].extra
            while extra:
                require(len(extra) >= 4, 'invalid_artifact_archive')
                kind, size = int.from_bytes(extra[:2], 'little'), int.from_bytes(extra[2:4], 'little')
                require(kind != 1 and len(extra) >= size + 4, 'invalid_artifact_archive')
                extra = extra[size + 4:]
            with archive.open(entries[0]) as source:
                raw = source.read(8193)
            require(len(raw) <= 8192, 'artifact_oversized')
            require(not raw.startswith(b'\xef\xbb\xbf'), 'invalid_artifact_archive')
            return strict_json(raw)
    except (ValueError, OSError, zipfile.BadZipFile, RuntimeError, RpcError):
        raise SmokeError('invalid_artifact_archive') from None


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        return None


def request(url, token=None, limit=65536):
    headers = {'Accept': 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    opener = urllib.request.build_opener(NoRedirect)
    try:
        with opener.open(urllib.request.Request(url, headers=headers), timeout=15) as response:
            raw = response.read(limit + 1)
            require(len(raw) <= limit, 'artifact_oversized')
            return response.status, raw, None
    except urllib.error.HTTPError as error:
        return error.code, b'', error.headers.get('Location')
    except (urllib.error.URLError, TimeoutError, OSError):
        raise SmokeError('artifact_network_failed') from None


def archive(artifact, token):
    status, raw, location = request(
        f"https://api.github.com/repos/Epiphytic/marmish/actions/artifacts/{artifact['id']}/zip", token)
    if status == 302:
        parts = urllib.parse.urlsplit(location or '')
        host = parts.hostname or ''
        require(parts.scheme == 'https' and parts.port in (None, 443) and
                parts.username is None and parts.password is None and
                (host.endswith('.blob.core.windows.net') or
                 host.endswith('.actions.githubusercontent.com')), 'invalid_artifact_redirect')
        status, raw, _unused = request(location)  # Never forward API credentials.
    require(status != 429 and not 500 <= status <= 599, 'artifact_download_transient')
    require(status == 200, 'artifact_download_failed')
    return read_zip(raw, artifact.get('digest'))


def wait(ctx, role, stage, timeout, deadline=None):
    token = os.environ.get('GITHUB_TOKEN')
    require(isinstance(token, str) and token, 'missing_artifact_token')
    deadline = min(deadline or float("inf"), continuous_time() + timeout)
    wanted = name(ctx, role, stage)
    while continuous_time() < deadline:
        try:
            status, raw, _location = request(
                f"https://api.github.com/repos/Epiphytic/marmish/actions/runs/{ctx['run']}/artifacts?per_page=100",
                token, 262144)
        except SmokeError as error:
            if str(error) != 'artifact_network_failed':
                raise
            status, raw = 503, b''
        require(continuous_time() < deadline, 'pairing_prerequisite_timeout')
        if status == 429 or 500 <= status <= 599:
            time.sleep(min(3, max(0, deadline - continuous_time())))
            continue
        require(status == 200, 'artifact_api_failed')
        try:
            body = strict_json(raw)
            require(isinstance(body, dict) and isinstance(body.get('artifacts'), list),
                    'invalid_artifact_list')
            matches = [x for x in body['artifacts'] if x.get('name') == wanted]
            require(len(matches) <= 1, 'duplicate_peer_artifact')
            if matches:
                item = matches[0]
                require(type(item.get('id')) is int and item['id'] > 0 and
                        item.get('expired') is False and item.get('size_in_bytes', 65537) <= 65536,
                        'invalid_peer_artifact')
                try:
                    value = archive(item, token)
                except SmokeError as error:
                    if str(error) not in ('artifact_network_failed', 'artifact_download_transient'):
                        raise
                    time.sleep(min(3, max(0, deadline - continuous_time())))
                    continue
                require(continuous_time() < deadline, 'pairing_prerequisite_timeout')
                return validate(value, ctx, role, stage)
        except (ValueError, TypeError, KeyError, AttributeError, RpcError):
            raise SmokeError('invalid_artifact_list') from None
        time.sleep(min(3, max(0, deadline - continuous_time())))
    raise SmokeError('pairing_prerequisite_timeout')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--role', choices=ROLES, required=True)
    parser.add_argument('--stage', choices=('ready', 'paired'), required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=int, default=1200)
    parser.add_argument('--state', type=Path, required=True)
    args = parser.parse_args()
    require(1 <= args.timeout <= 1200, 'invalid_timeout')
    try:
        from marmish_protocol import load_json
        root = Path(args.state.read_text())
        deadline = load_json(root / 'config.json')['pair_deadline']
        value = wait(context(), args.role, args.stage, args.timeout, deadline)
        args.output.write_text(json.dumps(value, sort_keys=True))
        return 0
    except SmokeError as error:
        print(json.dumps({'outcome': 'failed', 'error': str(error)}))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
