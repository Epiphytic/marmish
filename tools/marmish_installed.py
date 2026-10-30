"""Read-only installed layout resolution and prerequisite diagnostics."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys

TARGETS = {'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin'}
VERSION = r'0|[1-9][0-9]*'
VERSION_PATTERN = re.compile(r'(?:' + VERSION + r')\.(?:' + VERSION + r')\.(?:' + VERSION + r')-experimental\.(?:' + VERSION + r')')
MODULES = ('marmish', 'marmishd', 'marmish_app', 'marmish_bootstrap', 'marmish_keys',
           'marmish_mdk_build', 'marmish_platform', 'marmish_policy', 'marmish_protocol',
           'marmish_rpc', 'marmish_transport', 'marmish_transport_macos',
           'marmish_transport_sandbox', 'mdk_smoke', 'marmish_installed')
NATIVE = ('wnd', 'wnd.manifest.json', 'marmish-worker', 'marmish-engine-bridge', 'marmish-keychain')
DARWIN_NATIVE = ('marmish-lifetime', 'marmish-probe')


def target():
    cpu = {'x86_64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(platform.machine())
    suffix = {'Linux': 'unknown-linux-gnu', 'Darwin': 'apple-darwin'}.get(platform.system())
    value = str(cpu) + '-' + str(suffix)
    if value not in TARGETS:
        raise ValueError('unsupported_release_target')
    return value


def payload_paths(build_target):
    if build_target not in TARGETS:
        raise ValueError('unsupported_release_target')
    files = {'bin/marmish', 'bin/marmishd', 'LICENSE',
             'tests/fixtures/relay-canonicalization.json'}
    files.update('tools/' + name + '.py' for name in MODULES)
    files.update('libexec/marmish/' + name for name in NATIVE)
    if build_target.endswith('apple-darwin'):
        files.update('libexec/marmish/' + name for name in DARWIN_NATIVE)
    files.update('patches/mdk/' + name for name in
                 ('marmish-bootstrap.patch', 'marmish-restricted.patch', 'source-guard.json'))
    return files


def payload_mode(name):
    return 0o755 if name.startswith('bin/') or (name.startswith('libexec/marmish/')
                       and not name.endswith('.json')) else 0o644


def strict_json(raw):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('duplicate_release_field')
            result[key] = value
        return result
    return json.loads(raw, object_pairs_hook=pairs)


def valid_library(value):
    if (not isinstance(value, str) or not 1 <= len(value) <= 240 or '..' in value):
        return False
    if re.fullmatch(r'[A-Za-z0-9_.+-]+', value):
        return value != '.'
    return (value.startswith(('/usr/lib/', '/System/Library/')) and
            re.fullmatch(r'/[A-Za-z0-9_./+-]+', value) is not None and
            all(part not in ('', '.', '..') for part in value.split('/')[1:]))


def validate_record(record):
    required = {'format', 'version', 'target', 'source_commit', 'files', 'prerequisites',
                'experimental', 'limitations'}
    if (not isinstance(record, dict) or set(record) != required or
            type(record['format']) is not int or record['format'] != 1 or
            not isinstance(record['version'], str) or len(record['version']) > 64 or not VERSION_PATTERN.fullmatch(record['version']) or
            not isinstance(record['target'], str) or record['target'] not in TARGETS or
            not isinstance(record['source_commit'], str) or not re.fullmatch('[0-9a-f]{40}', record['source_commit']) or
            record['experimental'] is not True or not isinstance(record['limitations'], list) or
            not all(isinstance(item, str) and 0 < len(item) <= 1024 for item in record['limitations']) or
            not isinstance(record['files'], dict)):
        raise ValueError('invalid_release_record')
    files = record['files']
    if not payload_paths(record['target']) <= set(files) or not any(name.startswith('licenses/') for name in files):
        raise ValueError('incomplete_release_payload')
    for name, info in files.items():
        if (not isinstance(name, str) or len(name) > 200 or not re.fullmatch('[A-Za-z0-9_./+-]+', name) or
                any(part in ('', '.', '..') for part in name.split('/')) or
                (name not in payload_paths(record['target']) and not name.startswith('licenses/')) or
                not isinstance(info, dict) or set(info) != {'sha256', 'mode'} or
                not isinstance(info['sha256'], str) or not re.fullmatch('[0-9a-f]{64}', info['sha256']) or
                type(info['mode']) is not int or info['mode'] != payload_mode(name)):
            raise ValueError('invalid_release_payload')
    pre = record['prerequisites']
    if (not isinstance(pre, dict) or set(pre) != {'python', 'glibc', 'shared_libraries'} or
            pre['python'] != '3.11' or not isinstance(pre['shared_libraries'], list) or
            len(pre['shared_libraries']) > 64 or
            not all(valid_library(item) for item in pre['shared_libraries']) or
            (pre['glibc'] is not None and (not isinstance(pre['glibc'], str) or
                                        not re.fullmatch(r'[0-9]+(?:\.[0-9]+){1,2}', pre['glibc'])))):
        raise ValueError('invalid_release_prerequisites')
    if record['target'].endswith('linux-gnu') != (pre['glibc'] is not None):
        raise ValueError('invalid_release_glibc')
    return record


def _asset(root, name):
    path = root / name
    for component in [*reversed(path.parents), path]:
        info = component.lstat()
        sticky = stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and info.st_mode & stat.S_ISVTX
        if (stat.S_ISLNK(info.st_mode) or info.st_uid not in (0, os.geteuid()) or
                info.st_mode & 0o022 and not sticky):
            raise ValueError('unsafe_installed_asset')
    if not path.is_file():
        raise ValueError('missing_installed_asset')
    return path


def file_digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def installed_defaults():
    root = Path(__file__).resolve().parent.parent
    marker = root / 'release.json'
    if not marker.exists() and not marker.is_symlink():
        return {}
    _asset(root, 'release.json')
    if marker.stat().st_size > 4 * 1024 * 1024:
        raise ValueError('release_record_too_large')
    raw = marker.read_bytes()
    if len(raw) > 4 * 1024 * 1024:
        raise ValueError('release_record_too_large')
    record = validate_record(strict_json(raw))
    if record['target'] != target() or root.name != 'marmish-' + record['version'] + '-' + record['target']:
        raise ValueError('installed_target_mismatch')
    paths = {name: _asset(root, 'libexec/marmish/' + name) for name in NATIVE}
    for name, path in paths.items():
        relative = 'libexec/marmish/' + name
        if (stat.S_IMODE(path.stat().st_mode) != record['files'][relative]['mode'] or
                file_digest(path) != record['files'][relative]['sha256']):
            raise ValueError('installed_asset_mismatch')
    return {'wnd': paths['wnd'], 'wnd_sha256': record['files']['libexec/marmish/wnd']['sha256'],
            'wnd_manifest': paths['wnd.manifest.json'], 'bridge': paths['marmish-engine-bridge'],
            'worker': paths['marmish-worker'], 'keychain_helper': paths['marmish-keychain']}


def _doctor():
    checks = {'python_3_11': sys.version_info >= (3, 11)}
    if sys.platform.startswith('linux'):
        try:
            result = subprocess.run(['/usr/bin/python3', '-I', '-B', '-c',
                                     'import sys;raise SystemExit(sys.version_info < (3,11))'],
                                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                    stderr=subprocess.DEVNULL, timeout=5)
            checks['system_python_3_11'] = result.returncode == 0
        except (OSError, subprocess.TimeoutExpired):
            checks['system_python_3_11'] = False
        checks.update({name: path.is_file() and os.access(path, os.X_OK) for name, path in
                       [('bwrap', Path('/usr/bin/bwrap')), ('ip', Path('/usr/sbin/ip')),
                        ('sysctl', Path('/usr/sbin/sysctl'))]})
        checks['unshare'] = shutil.which('unshare') is not None
        checks['ca_certificates'] = Path('/etc/ssl/certs').is_dir()
        checks['nsswitch'] = Path('/etc/nsswitch.conf').is_file()
    elif sys.platform == 'darwin':
        version = platform.mac_ver()[0]
        checks['macos_15'] = bool(version) and int(version.split('.')[0]) >= 15
        checks.update({name: Path(path).is_file() and os.access(path, os.X_OK) for name, path in
                       [('sandbox_exec', '/usr/bin/sandbox-exec'), ('codesign', '/usr/bin/codesign'),
                        ('ifconfig', '/sbin/ifconfig')]})
    else:
        checks['supported_platform'] = False
    root = Path(__file__).resolve().parent.parent
    if (root / 'release.json').is_file():
        marker = _asset(root, 'release.json')
        with marker.open('rb') as source:
            raw = source.read(4 * 1024 * 1024 + 1)
        if len(raw) > 4 * 1024 * 1024:
            raise ValueError('release_record_too_large')
        record = validate_record(strict_json(raw))
        minimum = record['prerequisites']['glibc']
        if minimum is not None and sys.platform.startswith('linux'):
            try:
                libc = ctypes.CDLL(None)
                libc.gnu_get_libc_version.restype = ctypes.c_char_p
                actual = libc.gnu_get_libc_version().decode('ascii')
                checks['glibc_minimum'] = tuple(map(int, actual.split('.'))) >= tuple(map(int, minimum.split('.')))
            except (OSError, AttributeError, ValueError):
                checks['glibc_minimum'] = False
        for library in record['prerequisites']['shared_libraries']:
            try:
                ctypes.CDLL(library)
                checks['library:' + library] = True
            except OSError:
                checks['library:' + library] = False
    return {'ok': all(checks.values()), 'checks': checks,
            'note': 'Static prerequisites only; confinement and keychain providers were not tested.'}


def doctor():
    try:
        return _doctor()
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        return {'ok': False, 'checks': {}, 'error': 'invalid_local_release',
                'note': 'Static prerequisite inspection failed; no keychain provider was tested.'}


class DoctorAction(argparse.Action):
    def __call__(self, parser, namespace, values, option_string=None):
        result = doctor()
        print(json.dumps(result, sort_keys=True))
        parser.exit(0 if result['ok'] else 1)
