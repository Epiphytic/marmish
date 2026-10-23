"""Pinned MDK build provenance shared by native launch and CI."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat

from marmish_transport_sandbox import SandboxError

SOURCE = 'https://github.com/liamhelmer-bel/mdk.git'
REVISION = '37c248a11239e108afc9bd4ee62b8f60542012e3'
LOCK_SHA256 = 'd8a5367e49427c79f354b0611902808de9b92abbe173eb82ed6864d01e488a71'
ROOT = Path(__file__).resolve().parent.parent
PATCH = ROOT / 'patches' / 'mdk' / 'marmish-restricted.patch'
FIXTURE = ROOT / 'tests/fixtures/relay-canonicalization.json'


def digest(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise SandboxError('unsafe_digest_file')
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def strict_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate_field')
        result[key] = value
    return result


def verify_manifest(wnd, sha256, manifest=None, expected_platform=None):
    """Verify a nonsecret manifest and artifact, without trusting its own hashes."""
    path = Path(manifest) if manifest else Path(str(wnd) + '.manifest.json')
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK)
        with os.fdopen(fd, 'rb') as stream:
            meta = os.fstat(stream.fileno())
            if (not stat.S_ISREG(meta.st_mode) or meta.st_mode & 0o022
                    or meta.st_uid not in (0, os.geteuid()) or meta.st_nlink != 1):
                raise SandboxError('unsafe_manifest')
            raw = stream.read(16385)
        if len(raw) > 16384:
            raise SandboxError('invalid_manifest')
        value = json.loads(raw, object_pairs_hook=strict_object)
        host = expected_platform or platform.system().lower()
        fields = {'version', 'source', 'revision', 'clean_checkout', 'platform', 'architecture',
                  'rust', 'cargo_lock_sha256', 'patch_sha256', 'deployment_target',
                  'restricted_protocol', 'helpers', 'sha256', 'rustc_version', 'target',
                  'codesign', 'relay_fixture_sha256', 'transport_tests'}
        if (not isinstance(value, dict) or set(value) != fields
                or type(value.get('version')) is not int or value.get('version') != 1 or value.get('source') != SOURCE
                or value.get('revision') != REVISION or value.get('clean_checkout') is not True
                or value.get('platform') != host or value.get('sha256') != sha256
                or value.get('rust') != '1.97.1'
                or value.get('architecture') != platform.machine()
                or value.get('cargo_lock_sha256') != LOCK_SHA256
                or not isinstance(value.get('rustc_version'), str)
                or not value['rustc_version'].startswith('rustc 1.97.1 ')
                or value.get('target') != _target(host)
                or not re.fullmatch('[0-9a-f]{64}', sha256)
                or digest(wnd) != sha256):
            raise SandboxError('manifest_mismatch')
        if host == 'darwin':
            if (value.get('patch_sha256') != digest(PATCH)
                    or value.get('codesign') != 'adhoc'
                    or value.get('transport_tests') != 'marmish_transport::tests:passed'
                    or value.get('relay_fixture_sha256') != digest(FIXTURE)
                    or type(value.get('restricted_protocol')) is not int or value.get('restricted_protocol') != 1
                    or value.get('deployment_target') != '15.0'):
                raise SandboxError('manifest_patch_mismatch')
            if set(value.get('helpers', {})) != {'marmish-lifetime', 'marmish-probe'}:
                raise SandboxError('manifest_helper_mismatch')
            for name in ('marmish-lifetime', 'marmish-probe'):
                if value.get('helpers', {}).get(name) != digest(Path(wnd).parent / name):
                    raise SandboxError('manifest_helper_mismatch')
        elif (value.get('patch_sha256') is not None or value.get('helpers') != {}
              or value.get('codesign') is not None or value.get('relay_fixture_sha256') is not None
              or value.get('transport_tests') is not None
              or value.get('deployment_target') is not None or value.get('restricted_protocol') is not None):
            raise SandboxError('manifest_patch_mismatch')
        return value
    except SandboxError:
        raise
    except (OSError, ValueError, TypeError, AttributeError):
        raise SandboxError('invalid_manifest') from None


def _target(host):
    cpu = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'x86_64': 'x86_64'}.get(platform.machine())
    if not cpu:
        raise SandboxError('unsupported_architecture')
    return cpu + ('-apple-darwin' if host == 'darwin' else '-unknown-linux-gnu')
