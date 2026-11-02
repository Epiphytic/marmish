"""Resolve immutable public release inputs and verify actual installed bytes."""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import tempfile
import time
import types
import urllib.error

from marmish_installed import file_digest as digest_file, strict_json, validate_record
from marmish_platform import continuous_time

REPO = Path(__file__).resolve().parent.parent
REPOSITORY = 'Epiphytic/marmish'
VERSION = '0.1.0-experimental.2'
ROLE_TARGETS = {'linux-x64': 'x86_64-unknown-linux-gnu',
                'linux-arm64': 'aarch64-unknown-linux-gnu',
                'macos-arm64': 'aarch64-apple-darwin'}
ROLES = tuple(ROLE_TARGETS)
RELAYS = ['wss://relay.eu.whitenoise.chat/', 'wss://relay.us.whitenoise.chat/']
MAX_PIN = 32768
MAX_MANIFEST = 2 * 1024 * 1024
PIN_FIELDS = {'format', 'kind', 'repository', 'run', 'attempt', 'sha',
              'installer_sha256', 'version', 'tag', 'source_commit',
              'checksums_sha256', 'relays', 'targets'}
TARGET_FIELDS = {'target', 'archive_name', 'archive_sha256', 'manifest_name', 'manifest_sha256'}


def canonical_bytes(record):
    return (json.dumps(record, sort_keys=True, separators=(',', ':'),
                       ensure_ascii=False, allow_nan=False) + '\n').encode('utf-8')


def installer_from_bytes(raw):
    """Load validation helpers from the exact bytes the caller verified."""
    if not isinstance(raw, bytes) or len(raw) > MAX_MANIFEST:
        raise ValueError('pin_invalid')
    begin = b'# BEGIN MARMISH INSTALLER PYTHON\n'
    end = b'# END MARMISH INSTALLER PYTHON'
    if raw.count(begin) != 1 or raw.count(end) != 1:
        raise ValueError('pin_invalid')
    code = raw.split(begin, 1)[1].split(end, 1)[0]
    module = types.ModuleType('marmish_pinned_installer')
    exec(compile(code, 'verified-install.sh', 'exec'), module.__dict__)
    return module


def _require(condition, error='pin_invalid'):
    if not condition:
        raise ValueError(error)


def _hex(value, size=64):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{' + str(size) + '}', value) is not None


def _json(raw):
    _require(not raw.startswith(b'\xef\xbb\xbf'))
    value = strict_json(raw)
    canonical_bytes(value)  # Reject nonfinite numbers even in otherwise unused fields.
    return value


def validate_context(ctx):
    _require(isinstance(ctx, dict) and set(ctx) == {'run', 'attempt', 'sha'})
    _require(all(isinstance(ctx[key], str) and re.fullmatch('[1-9][0-9]{0,19}', ctx[key])
                 for key in ('run', 'attempt')) and _hex(ctx['sha'], 40))


def _manifest(raw):
    _require(isinstance(raw, bytes) and len(raw) <= MAX_MANIFEST)
    record = validate_record(_json(raw))
    _require(bool(record['limitations']) and all(
        all(ord(char) >= 32 and ord(char) != 127 for char in item)
        for item in record['limitations']))
    return record


def validate_pin(record, manifests, ctx):
    """Validate a complete pin without probing the resolve host's libraries."""
    validate_context(ctx)
    _require(isinstance(record, dict) and set(record) == PIN_FIELDS)
    _require(type(record['format']) is int and record['format'] == 1 and
             record['kind'] == 'release-pin' and record['repository'] == REPOSITORY and
             all(record[key] == ctx[key] for key in ctx) and record['relays'] == RELAYS)
    _require(record['version'] == VERSION and record['tag'] == 'v' + VERSION)
    for key in ('installer_sha256', 'checksums_sha256'):
        _require(_hex(record[key]))
    _require(_hex(record['source_commit'], 40) and isinstance(record['targets'], dict) and
             set(record['targets']) == set(ROLES))
    _require(isinstance(manifests, dict) and set(manifests) == set(ROLES))
    for role, target in ROLE_TARGETS.items():
        item = record['targets'][role]
        _require(isinstance(item, dict) and set(item) == TARGET_FIELDS)
        _require(item['target'] == target and
                 item['archive_name'] == f'marmish-{VERSION}-{target}.tar.gz' and
                 item['manifest_name'] == role + '.release.json' and
                 _hex(item['archive_sha256']) and _hex(item['manifest_sha256']))
        raw = manifests[role]
        manifest = _manifest(raw)
        _require(hashlib.sha256(raw).hexdigest() == item['manifest_sha256'] and
                 (manifest['version'], manifest['target'], manifest['source_commit']) ==
                 (VERSION, target, record['source_commit']))
    _require(len(canonical_bytes(record)) <= MAX_PIN)
    return record


def _read_regular(path, limit, *, mode=None):
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), 'rb') as source:
        info = os.fstat(source.fileno())
        _require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                 info.st_uid == os.geteuid() and info.st_size <= limit)
        if mode is not None:
            _require(stat.S_IMODE(info.st_mode) == mode)
        raw = source.read(limit + 1)
        _require(len(raw) <= limit)
        return raw


def load_pin(directory, ctx):
    directory = Path(directory)
    _require(not directory.is_symlink() and directory.is_dir())
    names = {'record.json'} | {role + '.release.json' for role in ROLES}
    _require({item.name for item in directory.iterdir()} == names)
    raw = _read_regular(directory / 'record.json', MAX_PIN)
    record = _json(raw)
    _require(raw == canonical_bytes(record))
    manifests = {role: _read_regular(directory / (role + '.release.json'), MAX_MANIFEST)
                 for role in ROLES}
    return validate_pin(record, manifests, ctx)


def _transient(error):
    cause = error.__cause__
    if isinstance(cause, urllib.error.HTTPError):
        return cause.code == 429 or 500 <= cause.code <= 599
    return isinstance(cause, (urllib.error.URLError, TimeoutError))


def _download_retry(module, url, destination, limit, deadline):
    for attempt in range(3):
        _require(continuous_time() < deadline, 'release_resolve_failed')
        with tempfile.TemporaryDirectory(prefix='download-', dir=destination.parent) as temporary:
            path = Path(temporary) / 'asset'
            try:
                module.download(url, path, limit)
            except module.InstallError as error:
                if attempt == 2 or not _transient(error):
                    raise ValueError('release_resolve_failed') from None
            else:
                _require(continuous_time() < deadline, 'release_resolve_failed')
                os.replace(path, destination)
                return
        time.sleep(min(3, max(0, deadline - continuous_time())))
    raise ValueError('release_resolve_failed')


def _archive_manifest(module, archive, version, target):
    with tempfile.TemporaryDirectory(prefix='extract-', dir=archive.parent) as temporary:
        stage = Path(temporary)
        module.inflate(archive, stage / 'archive.tar')
        root, payload = module.extract_ustar(stage / 'archive.tar', stage / 'payload', version, target)
        raw = _read_regular(root / 'release.json', MAX_MANIFEST)
        manifest = _manifest(raw)
        _require((manifest['version'], manifest['target']) == (version, target) and
                 set(manifest['files']) == payload, 'release_resolve_failed')
        for name, info in manifest['files'].items():
            _require(stat.S_IMODE((root / name).stat().st_mode) == info['mode'] and
                     digest_file(root / name) == info['sha256'], 'release_resolve_failed')
        return raw, manifest


def resolve_release(repo, output, ctx, *, version):
    """Download/inspect all native archives; never install or run foreign code."""
    validate_context(ctx)
    _require(version == VERSION, 'release_resolve_failed')
    # No network/build child of this operation may inherit API credentials.
    _require(not any(os.environ.get(key) for key in (
        'GITHUB_TOKEN', 'GH_TOKEN', 'GITHUB_ENTERPRISE_TOKEN', 'GH_ENTERPRISE_TOKEN',
        'ACTIONS_ID_TOKEN_REQUEST_TOKEN')), 'release_resolve_failed')
    installer_raw = _read_regular(Path(repo) / 'scripts/install.sh', MAX_MANIFEST)
    module = installer_from_bytes(installer_raw)
    output = Path(output)
    _require(not os.path.lexists(output), 'release_resolve_failed')
    deadline = continuous_time() + 900
    with tempfile.TemporaryDirectory(prefix='marmish-resolve-', dir=output.parent) as temporary:
        work = Path(temporary)
        api = work / 'api.json'
        _download_retry(module, module.API + '/tags/v' + VERSION, api, MAX_MANIFEST, deadline)
        release = _json(api.read_bytes())
        _require(isinstance(release, dict) and release.get('tag_name') == 'v' + VERSION and
                 release.get('draft') is False and release.get('prerelease') is True,
                 'release_resolve_failed')
        targets, manifests, sources = {}, {}, set()
        for role, target in ROLE_TARGETS.items():
            name = f'marmish-{VERSION}-{target}.tar.gz'
            assets = module.release_assets(release, name)
            if not (work / 'SHA256SUMS').exists():
                _download_retry(module, assets['SHA256SUMS'], work / 'SHA256SUMS',
                                MAX_MANIFEST, deadline)
            archive = work / name
            _download_retry(module, assets[name], archive, module.MAX_DOWNLOAD, deadline)
            module.verify_checksum(archive, work / 'SHA256SUMS', name)
            raw, manifest = _archive_manifest(module, archive, version, target)
            manifests[role] = raw
            sources.add(manifest['source_commit'])
            targets[role] = {'target': target, 'archive_name': name,
                'archive_sha256': digest_file(archive), 'manifest_name': role + '.release.json',
                'manifest_sha256': hashlib.sha256(raw).hexdigest()}
            archive.unlink()
            _require(continuous_time() < deadline, 'release_resolve_failed')
        _require(len(sources) == 1, 'release_resolve_failed')
        record = {'format': 1, 'kind': 'release-pin', 'repository': REPOSITORY, **ctx,
            'installer_sha256': hashlib.sha256(installer_raw).hexdigest(),
            'version': version, 'tag': 'v' + version, 'source_commit': sources.pop(),
            'checksums_sha256': digest_file(work / 'SHA256SUMS'), 'relays': list(RELAYS),
            'targets': targets}
        validate_pin(record, manifests, ctx)
        ready = work / 'ready'
        ready.mkdir(mode=0o700)
        (ready / 'record.json').write_bytes(canonical_bytes(record))
        for role, raw in manifests.items():
            (ready / (role + '.release.json')).write_bytes(raw)
        os.rename(ready, output)
        return record


def _entry(path, mode, *, directory=False):
    info = path.lstat()
    _require(info.st_uid == os.geteuid() and stat.S_IMODE(info.st_mode) == mode and
             (stat.S_ISDIR(info.st_mode) if directory else
              stat.S_ISREG(info.st_mode) and info.st_nlink == 1), 'install_verify_failed')


def verify_installed(prefix, pin, manifest_raw, role):
    """Read-only verification after the actual installer subprocess succeeds."""
    _require(role in ROLE_TARGETS, 'install_verify_failed')
    manifest = _manifest(manifest_raw)
    item = pin['targets'][role]
    _require(hashlib.sha256(manifest_raw).hexdigest() == item['manifest_sha256'] and
             (manifest['version'], manifest['target'], manifest['source_commit']) ==
             (pin['version'], ROLE_TARGETS[role], pin['source_commit']), 'install_verify_failed')
    prefix = Path(prefix)
    installer_raw = _read_regular(REPO / 'scripts/install.sh', MAX_MANIFEST)
    _require(hashlib.sha256(installer_raw).hexdigest() == pin['installer_sha256'],
             'install_verify_failed')
    module = installer_from_bytes(installer_raw)
    _require(module.native_target() == item['target'], 'install_verify_failed')
    module.safe_path(prefix)
    _entry(prefix, 0o700, directory=True)
    _require({path.name for path in prefix.iterdir()} == {
        '.marmish-install.json', '.marmish-install.lock', 'current', 'bin', 'versions'},
        'install_verify_failed')
    for name in ('.marmish-install.json', '.marmish-install.lock'):
        _entry(prefix / name, 0o600)
    _require((prefix / '.marmish-install.lock').stat().st_size == 0, 'install_verify_failed')
    state = module.read_installation(prefix, prefix / 'bin')
    _require(type(state['format']) is int and state == {'format': 1, 'version': pin['version'], 'target': item['target'],
                       'launcher_dir': str(prefix / 'bin')}, 'install_verify_failed')
    _entry(prefix / 'bin', 0o700, directory=True)
    _entry(prefix / 'versions', 0o700, directory=True)
    name = f"marmish-{pin['version']}-{item['target']}"
    root = prefix / 'versions' / name
    _require({path.name for path in (prefix / 'bin').iterdir()} == {'marmish', 'marmishd'} and
             {path.name for path in (prefix / 'versions').iterdir()} == {name},
             'install_verify_failed')
    for launcher in ('marmish', 'marmishd'):
        _entry(prefix / 'bin' / launcher, 0o755)
    _entry(root, 0o755, directory=True)
    files = set(manifest['files']) | {'release.json'}
    directories = {str(parent) for name in files for parent in Path(name).parents
                   if str(parent) != '.'}
    _require({str(path.relative_to(root)) for path in root.rglob('*')} == files | directories,
             'install_verify_failed')
    for relative in directories:
        _entry(root / relative, 0o755, directory=True)
    for relative in files:
        _entry(root / relative, 0o644 if relative == 'release.json' else
               manifest['files'][relative]['mode'])
    _require(_read_regular(root / 'release.json', MAX_MANIFEST) == manifest_raw,
             'install_verify_failed')
    module.validate_manifest(root, set(manifest['files']), pin['version'], item['target'])
    installation = {key: pin[key] for key in ('version', 'source_commit', 'installer_sha256')}
    installation.update({key: item[key] for key in ('target', 'archive_sha256', 'manifest_sha256')})
    return {'root': root, 'client': prefix / 'bin/marmish',
            'server': prefix / 'bin/marmishd', 'installation': installation}
