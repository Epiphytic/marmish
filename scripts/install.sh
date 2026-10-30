#!/bin/sh
# Marmish's complete installer is embedded below; no secondary script is fetched.
set -eu
command -v python3 >/dev/null 2>&1 || { echo 'marmish: Python 3.11 or newer is required' >&2; exit 1; }
exec python3 -I -B - "$@" <<'MARMISH_INSTALLER_PYTHON'
# BEGIN MARMISH INSTALLER PYTHON
import argparse
import ctypes
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shlex
import shutil
import stat
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zlib

API = "https://api.github.com/repos/Epiphytic/marmish/releases"
HOSTS = frozenset({"api.github.com", "github.com", "release-assets.githubusercontent.com", "objects.githubusercontent.com"})
VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-experimental\.(?:0|[1-9][0-9]*)\Z")
TARGETS = {("Linux", "x86_64"): "x86_64-unknown-linux-gnu", ("Linux", "aarch64"): "aarch64-unknown-linux-gnu", ("Darwin", "arm64"): "aarch64-apple-darwin"}
MAX_DOWNLOAD = 512 * 1024 * 1024
MAX_EXPANDED = 1536 * 1024 * 1024
MAX_FILES = 4096
MAX_MANIFEST = 2 * 1024 * 1024
MARKER = ".marmish-install.json"
TOOLS = ("marmish.py", "marmishd.py", "marmish_app.py", "marmish_bootstrap.py", "marmish_keys.py", "marmish_mdk_build.py", "marmish_platform.py", "marmish_policy.py", "marmish_protocol.py", "marmish_rpc.py", "marmish_transport.py", "marmish_transport_macos.py", "marmish_transport_sandbox.py", "mdk_smoke.py", "marmish_installed.py")
EXECUTABLES = {"bin/marmish", "bin/marmishd", "libexec/marmish/wnd", "libexec/marmish/marmish-worker", "libexec/marmish/marmish-engine-bridge", "libexec/marmish/marmish-keychain"}
DARWIN_EXECUTABLES = {"libexec/marmish/marmish-lifetime", "libexec/marmish/marmish-probe"}
FIXED_FILES = EXECUTABLES | {"LICENSE", "libexec/marmish/wnd.manifest.json", "patches/mdk/marmish-bootstrap.patch", "patches/mdk/marmish-restricted.patch", "patches/mdk/source-guard.json", "tests/fixtures/relay-canonicalization.json"} | {"tools/" + name for name in TOOLS}


class InstallError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise InstallError(message)


def validate_version(version):
    require(isinstance(version, str) and len(version) <= 64 and VERSION.fullmatch(version), "expected version MAJOR.MINOR.PATCH-experimental.N")
    return version


def native_target():
    require(sys.version_info >= (3, 11), "Python 3.11 or newer is required")
    target = TARGETS.get((platform.system(), platform.machine()))
    require(target is not None, "supported targets: Linux amd64/arm64 and macOS arm64")
    if platform.system() == "Darwin":
        require(int(platform.mac_ver()[0].split('.')[0] or 0) >= 15, "macOS 15 or newer is required")
    return target


def checked_url(url):
    error = "download URL outside approved HTTPS release hosts"
    try:
        parsed = urllib.parse.urlsplit(url)
        approved = parsed.scheme == "https" and parsed.hostname in HOSTS and parsed.port in (None, 443) and not parsed.username and not parsed.password and not parsed.fragment
    except ValueError as exc:
        raise InstallError(error) from exc
    require(approved, error)
    return url


class ReleaseRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, new_url):
        checked_url(new_url)
        return super().redirect_request(request, fp, code, message, headers, new_url)


def download(url, destination, limit):
    checked_url(url)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), ReleaseRedirect())
    request = urllib.request.Request(url, headers={"User-Agent": "marmish-installer/1", "Accept": "application/vnd.github+json" if url.startswith(API) else "application/octet-stream"})
    try:
        deadline = time.monotonic() + 120
        with opener.open(request, timeout=30) as response, destination.open('xb') as output:
            total = 0
            while chunk := response.read(64 * 1024):
                require(time.monotonic() <= deadline, "download time limit exceeded")
                total += len(chunk)
                require(total <= limit, "download exceeds size limit")
                output.write(chunk)
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        raise InstallError("release download failed; check HTTPS connectivity and GitHub API rate limits, or retry with --version") from exc


def read_json_bytes(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    try:
        return json.loads(data, object_pairs_hook=unique)
    except (ValueError, UnicodeError) as exc:
        raise InstallError("invalid JSON metadata") from exc


def release_metadata(version, directory):
    path = directory / 'api.json'
    url = API + ("/tags/v" + validate_version(version) if version else "?per_page=100")
    download(url, path, MAX_MANIFEST)
    records = read_json_bytes(path.read_bytes())
    require(isinstance(records, (list, dict)), "invalid release API response")
    records = records if isinstance(records, list) else [records]
    for record in records:
        if not isinstance(record, dict):
            continue
        tag = record.get('tag_name', '')
        if not isinstance(tag, str) or not tag.startswith('v') or not VERSION.fullmatch(tag[1:]):
            continue
        if record.get('draft') is False and record.get('prerelease') is True and (version is None or tag == 'v' + version):
            return tag[1:], record
    raise InstallError("no matching experimental prerelease found; specify --version")


def release_assets(record, archive_name):
    assets = record.get('assets')
    require(isinstance(assets, list), "release assets missing")
    result = {}
    for asset in assets:
        require(isinstance(asset, dict), "invalid release asset")
        name = asset.get('name')
        if name in (archive_name, 'SHA256SUMS'):
            require(name not in result and asset.get('state') == 'uploaded', "ambiguous or incomplete release assets")
            result[name] = checked_url(asset.get('browser_download_url', ''))
    require(set(result) == {archive_name, 'SHA256SUMS'}, "release is missing archive or SHA256SUMS")
    return result


def verify_checksum(archive, sums, archive_name):
    try:
        lines = sums.read_text(encoding='ascii').splitlines()
    except UnicodeError as exc:
        raise InstallError("invalid SHA256SUMS") from exc
    matching = []
    for line in lines:
        match = re.fullmatch(r'([0-9a-f]{64}) [ *]([A-Za-z0-9._-]+)', line)
        require(match is not None, "invalid SHA256SUMS line")
        if match[2] == archive_name:
            matching.append(match[1])
    require(len(matching) == 1, "archive checksum missing or duplicated")
    require(digest_file(archive) == matching[0], "archive checksum mismatch")


def digest_file(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def inflate(archive, output):
    decoder = zlib.decompressobj(16 + zlib.MAX_WBITS)
    total = 0
    compressed = 0
    with archive.open('rb') as source, output.open('xb') as sink:
        while block := source.read(64 * 1024):
            compressed += len(block)
            require(compressed <= MAX_DOWNLOAD, "compressed archive exceeds size limit")
            if decoder.eof:
                require(not block.strip(b'\0'), "trailing gzip data")
                continue
            while block:
                try:
                    chunk = decoder.decompress(block, 64 * 1024)
                except zlib.error as exc:
                    raise InstallError("invalid gzip archive") from exc
                total += len(chunk)
                require(total <= MAX_EXPANDED, "expanded archive exceeds size limit")
                sink.write(chunk)
                block = decoder.unconsumed_tail
                if decoder.eof:
                    require(not decoder.unused_data.strip(b'\0'), "trailing gzip data")
                    break
        require(decoder.eof, "truncated gzip archive")


def tar_text(field):
    raw, separator, tail = field.partition(b'\0')
    require(not separator or not tail.strip(b'\0'), "noncanonical tar text field")
    try:
        return raw.decode('ascii')
    except UnicodeError as exc:
        raise InstallError("non-ASCII archive path") from exc


def tar_number(field):
    require(not (field[0] & 0x80), "tar numeric extensions are forbidden")
    value = field.rstrip(b'\0 ').lstrip(b' ')
    require(not value or re.fullmatch(b'[0-7]+', value), "invalid tar number")
    return int(value or b'0', 8)


def canonical_path(name):
    require(len(name) <= 240 and re.fullmatch(r'[A-Za-z0-9._/-]+', name) and all(part not in ('', '.', '..') for part in name.split('/')), "unsafe archive path")
    return name


def payload_mode(path, target):
    executable = EXECUTABLES | (DARWIN_EXECUTABLES if target.endswith('darwin') else set())
    return 0o755 if path in executable else 0o644


def allowed_payload(path, target):
    fixed = FIXED_FILES | (DARWIN_EXECUTABLES if target.endswith('darwin') else set())
    return path in fixed or (path.startswith('licenses/') and len(path) > len('licenses/'))


def parse_header(header):
    require(header[257:263] == b'ustar\0' and header[263:265] == b'00', "only strict USTAR archives are accepted")
    expected = sum(header[:148]) + 8 * 32 + sum(header[156:])
    require(tar_number(header[148:156]) == expected, "tar header checksum mismatch")
    require(header[156:157] in (b'0', b'\0', b'5'), "archive links, extensions and special files are forbidden")
    require(not header[157:257].strip(b'\0') and not header[500:].strip(b'\0'), "unsupported tar metadata")
    for field in (header[108:116], header[116:124], header[136:148], header[329:337], header[337:345]):
        tar_number(field)
    name = tar_text(header[:100])
    prefix = tar_text(header[345:500])
    name = prefix + '/' + name if prefix else name
    directory = header[156:157] == b'5'
    name = name[:-1] if directory and name.endswith('/') else name
    return canonical_path(name), tar_number(header[100:108]), tar_number(header[124:136]), directory


def extract_ustar(tar_path, stage, version, target):
    root = 'marmish-' + version + '-' + target
    seen = set()
    case_paths = {}
    files = set()
    directories = set()
    with tar_path.open('rb') as source:
        while True:
            header = source.read(512)
            require(len(header) == 512, "truncated tar header")
            if header == bytes(512):
                require(source.read(512) == bytes(512), "tar needs two end blocks")
                while tail := source.read(64 * 1024):
                    require(not tail.strip(b'\0'), "trailing tar data")
                break
            name, mode, size, directory = parse_header(header)
            require(name not in seen and len(seen) < MAX_FILES, "duplicate or excessive archive members")
            seen.add(name)
            for component in (name, *(str(parent) for parent in Path(name).parents if str(parent) != '.')):
                key = component.lower()
                require(case_paths.get(key, component) == component, "case-colliding archive paths")
                case_paths[key] = component
            require(name == root or name.startswith(root + '/'), "archive root does not match version and target")
            relative = name[len(root):].lstrip('/')
            require(directory or relative, "archive root must be a directory")
            require(mode == (0o755 if directory else payload_mode(relative, target)), "unsafe or unexpected archive mode")
            if directory:
                require(size == 0, "archive directory has data")
                directories.add(relative)
                (stage / name).mkdir(parents=True, exist_ok=True, mode=0o755)
                continue
            require(relative == 'release.json' or allowed_payload(relative, target), "unexpected archive payload")
            require(size <= (MAX_MANIFEST if relative == 'release.json' else MAX_DOWNLOAD), "archive member exceeds size limit")
            path = stage / name
            path.parent.mkdir(parents=True, exist_ok=True, mode=0o755)
            with path.open('xb') as output:
                remaining = size
                while remaining:
                    chunk = source.read(min(64 * 1024, remaining))
                    require(chunk, "truncated tar payload")
                    output.write(chunk)
                    remaining -= len(chunk)
            path.chmod(mode)
            padding = (-size) % 512
            require(source.read(padding) == bytes(padding), "invalid tar padding")
            files.add(relative)
    implied = {''}
    for name in files:
        implied.update(str(parent) for parent in Path(name).parents if str(parent) != '.')
    require(directories <= implied, "unexpected archive directory")
    require('release.json' in files, "release manifest missing")
    # Implicit directory creation must not depend on the caller's umask.
    for path in (stage / root).rglob('*'):
        if path.is_dir():
            path.chmod(0o755)
    (stage / root).chmod(0o755)
    return stage / root, files - {'release.json'}


def validate_manifest(root, payload, version, target):
    manifest = read_json_bytes((root / 'release.json').read_bytes())
    fields = {'format', 'version', 'target', 'source_commit', 'files', 'prerequisites', 'experimental', 'limitations'}
    require(isinstance(manifest, dict) and set(manifest) == fields, "invalid release manifest fields")
    require(type(manifest['format']) is int and manifest['format'] == 1 and manifest['experimental'] is True, "unsupported or non-experimental release")
    require(manifest['version'] == version and manifest['target'] == target, "manifest version or target mismatch")
    require(isinstance(manifest['source_commit'], str) and re.fullmatch('[0-9a-f]{40}', manifest['source_commit']), "invalid source commit")
    records = manifest['files']
    require(isinstance(records, dict) and set(records) == payload, "manifest payload set mismatch")
    fixed = FIXED_FILES | (DARWIN_EXECUTABLES if target.endswith('darwin') else set())
    require(fixed <= payload and any(name.startswith('licenses/') for name in payload), "incomplete runtime or license payload")
    for name, record in records.items():
        require(isinstance(record, dict) and set(record) == {'sha256', 'mode'}, "invalid payload record")
        require(type(record['mode']) is int and record['mode'] == payload_mode(name, target), "manifest mode mismatch")
        require(isinstance(record['sha256'], str) and re.fullmatch('[0-9a-f]{64}', record['sha256']), "invalid payload digest")
        require(digest_file(root / name) == record['sha256'], "payload checksum mismatch")
    limits = manifest['limitations']
    require(isinstance(limits, list) and limits and all(isinstance(item, str) and 0 < len(item) < 1024 and all(ord(char) >= 32 and ord(char) != 127 for char in item) for item in limits), "invalid experimental limitations")
    validate_prerequisites(manifest['prerequisites'], target)
    return manifest


def version_tuple(value):
    require(isinstance(value, str) and re.fullmatch(r'[0-9]+\.[0-9]+(?:\.[0-9]+)?', value), "invalid runtime prerequisite version")
    return tuple(map(int, value.split('.')))


def valid_library(value):
    if not isinstance(value, str) or not 0 < len(value) <= 240 or '..' in value:
        return False
    if '/' not in value:
        return value != '.' and re.fullmatch(r'[A-Za-z0-9_.+-]+', value) is not None
    return value.startswith(('/usr/lib/', '/System/Library/')) and re.fullmatch(r'/[A-Za-z0-9_./+-]+', value) is not None and all(part and part != '.' for part in value.split('/')[1:])


def validate_prerequisites(prerequisites, target):
    require(isinstance(prerequisites, dict) and set(prerequisites) == {'python', 'glibc', 'shared_libraries'}, "invalid runtime prerequisites")
    require(prerequisites['python'] == '3.11', "unsupported Python prerequisite")
    libraries = prerequisites['shared_libraries']
    require(isinstance(libraries, list) and len(libraries) <= 64 and all(valid_library(item) for item in libraries), "invalid shared library requirements")
    if target.endswith('linux-gnu'):
        required = version_tuple(prerequisites['glibc'])
        family, current = platform.libc_ver()
        require(family == 'glibc' and version_tuple(current) >= required, "host glibc is older than this binary requires")
    else:
        require(prerequisites['glibc'] is None, "unexpected glibc requirement on macOS")
    for library in libraries:
        try:
            ctypes.CDLL(library)
        except OSError as exc:
            raise InstallError("required shared library unavailable: " + library) from exc


def safe_path(path):
    require(path.is_absolute() and str(path) != '/' and '..' not in path.parts, "installation paths must be absolute and canonical")
    for component in (path, *path.parents):
        require(not component.is_symlink(), "symlink installation path refused")
        if component.exists():
            require(component.is_dir(), "installation path component is not a directory")


def owned_directory(path, private=False):
    safe_path(path)
    if not path.exists():
        path.mkdir(parents=True, mode=0o700)
    info = path.stat()
    require(info.st_uid == os.geteuid(), "installation directory is not owned by this user")
    guidance = "chmod go-rwx" if private else "chmod go-w"
    require(not info.st_mode & 0o022 and (not private or not info.st_mode & 0o077), "installation directory permissions are unsafe: " + str(path) + "; run " + guidance + " on that directory and retry")


def regular_owned(path):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid() and info.st_nlink == 1 and not info.st_mode & 0o022, "unowned, linked or unsafe installer file")


@contextmanager
def installation_lock(prefix):
    path = prefix / '.marmish-install.lock'
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    try:
        regular_owned(path)
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise InstallError("another Marmish installer is using this prefix") from exc
        yield
    finally:
        os.close(descriptor)


def launcher_bytes(version_root, name):
    return ('#!/bin/sh\n# marmish installer format 1\nexec ' + shlex.quote(str(version_root / 'bin' / name)) + ' "$@"\n').encode()


def read_installation(prefix, launcher_dir):
    marker = prefix / MARKER
    if not marker.exists() and not marker.is_symlink():
        require(not (prefix / 'current').exists() and not (prefix / 'current').is_symlink(), "unowned current selector")
        for name in ('marmish', 'marmishd'):
            require(not os.path.lexists(launcher_dir / name), "refusing to replace an unrelated launcher")
        return None
    regular_owned(marker)
    require(marker.stat().st_size <= 16384, "oversized installation marker")
    state = read_json_bytes(marker.read_bytes())
    require(isinstance(state, dict) and set(state) == {'format', 'version', 'target', 'launcher_dir'}, "invalid installation ownership marker")
    require(state['format'] == 1 and state['target'] in TARGETS.values() and state['launcher_dir'] == str(launcher_dir), "installation marker mismatch")
    validate_version(state['version'])
    old_root = prefix / 'versions' / ('marmish-' + state['version'] + '-' + state['target'])
    safe_path(old_root)
    current = prefix / 'current'
    require(current.is_symlink() and current.lstat().st_uid == os.geteuid() and os.readlink(current) == str(old_root.relative_to(prefix)), "unowned current selector")
    for name in ('marmish', 'marmishd'):
        path = launcher_dir / name
        regular_owned(path)
        require(path.read_bytes() == launcher_bytes(old_root, name), "launcher ownership marker mismatch")
    return state


def matching_tree(existing, staged):
    require(not existing.is_symlink(), "version destination is a symlink")
    existing_paths = {str(path.relative_to(existing)) for path in existing.rglob('*')}
    staged_paths = {str(path.relative_to(staged)) for path in staged.rglob('*')}
    require(existing_paths == staged_paths, "installed version collides with different files")
    for relative in staged_paths:
        old, new = existing / relative, staged / relative
        require(not old.is_symlink() and old.stat().st_uid == os.geteuid(), "unsafe existing version member")
        require(stat.S_IFMT(old.stat().st_mode) == stat.S_IFMT(new.stat().st_mode) and stat.S_IMODE(old.stat().st_mode) == stat.S_IMODE(new.stat().st_mode), "installed version mode collision")
        if new.is_file():
            require(old.stat().st_nlink == 1 and digest_file(old) == digest_file(new), "installed version content collision")


def replace_file(path, data, mode):
    descriptor, temporary = tempfile.mkstemp(prefix='.marmish-write-', dir=path.parent)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, mode)
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def replace_selector(prefix, destination):
    temporary = prefix / ('.marmish-current-' + secrets.token_hex(8))
    try:
        temporary.symlink_to(destination)
        os.replace(temporary, prefix / 'current')
    finally:
        temporary.unlink(missing_ok=True)


def publish_installation(prefix, launcher_dir, root, version, target, previous):
    old_files = {name: (launcher_dir / name).read_bytes() if previous else None for name in ('marmish', 'marmishd')}
    old_marker = (prefix / MARKER).read_bytes() if previous else None
    old_selector = os.readlink(prefix / 'current') if previous else None
    try:
        for name in ('marmish', 'marmishd'):
            replace_file(launcher_dir / name, launcher_bytes(root, name), 0o755)
        replace_selector(prefix, str(root.relative_to(prefix)))
        state = {'format': 1, 'version': version, 'target': target, 'launcher_dir': str(launcher_dir)}
        replace_file(prefix / MARKER, (json.dumps(state, sort_keys=True) + '\n').encode(), 0o600)
    except BaseException:
        for name, data in old_files.items():
            if data is None:
                (launcher_dir / name).unlink(missing_ok=True)
            else:
                replace_file(launcher_dir / name, data, 0o755)
        if old_selector is None:
            (prefix / 'current').unlink(missing_ok=True)
        else:
            replace_selector(prefix, old_selector)
        if old_marker is None:
            (prefix / MARKER).unlink(missing_ok=True)
        else:
            replace_file(prefix / MARKER, old_marker, 0o600)
        raise


def install_archive(archive, prefix, launcher_dir, version, target):
    validate_version(version)
    require(target in TARGETS.values(), "unknown target")
    require(os.geteuid() != 0, "run this installer as your normal user, without sudo")
    owned_directory(prefix, private=True)
    owned_directory(launcher_dir)
    with installation_lock(prefix):
        return install_locked(archive, prefix, launcher_dir, version, target)


def install_locked(archive, prefix, launcher_dir, version, target):
    previous = read_installation(prefix, launcher_dir)
    versions = prefix / 'versions'
    owned_directory(versions, private=True)
    with tempfile.TemporaryDirectory(prefix='.marmish-stage-', dir=prefix) as temporary:
        stage = Path(temporary)
        tar_path = stage / 'package.tar'
        inflate(archive, tar_path)
        extracted, payload = extract_ustar(tar_path, stage, version, target)
        manifest = validate_manifest(extracted, payload, version, target)
        root = versions / extracted.name
        if os.path.lexists(root):
            require(root.is_dir() and root.stat().st_uid == os.geteuid(), "unsafe version destination")
            matching_tree(root, extracted)
        else:
            os.replace(extracted, root)
        publish_installation(prefix, launcher_dir, root, version, target, previous)
    return manifest


def uninstall(prefix, launcher_dir):
    require(os.geteuid() != 0, "run this installer as your normal user, without sudo")
    safe_path(prefix)
    safe_path(launcher_dir)
    require(prefix.exists(), "no installation found")
    owned_directory(prefix, private=True)
    owned_directory(launcher_dir)
    with installation_lock(prefix):
        uninstall_locked(prefix, launcher_dir)


def uninstall_locked(prefix, launcher_dir):
    state = read_installation(prefix, launcher_dir)
    require(state is not None, "no installer-owned installation found")
    for name in ('marmish', 'marmishd'):
        (launcher_dir / name).unlink()
    (prefix / 'current').unlink()
    (prefix / MARKER).unlink()
    print('Removed installer-owned launchers. Version trees and all user state were retained.')


def main(argv=None):
    parser = argparse.ArgumentParser(description='Install experimental Marmish binaries for your user. Downloads from GitHub Releases; no shell profile or identity changes.')
    parser.add_argument('--version', help='MAJOR.MINOR.PATCH-experimental.N; defaults to newest matching prerelease')
    parser.add_argument('--prefix', help='absolute private install prefix; its bin/ receives launchers (default: ~/.local/share/marmish, launchers in ~/.local/bin)')
    parser.add_argument('--uninstall', action='store_true', help='remove owned launchers/current selector; retain versions and all user state')
    options = parser.parse_args(argv)
    require(sys.version_info >= (3, 11), "Python 3.11 or newer is required")
    require(os.geteuid() != 0, "run this installer as your normal user, without sudo")
    prefix = Path(options.prefix) if options.prefix else Path.home() / '.local/share/marmish'
    launcher_dir = prefix / 'bin' if options.prefix else Path.home() / '.local/bin'
    safe_path(prefix)
    safe_path(launcher_dir)
    if options.uninstall:
        require(options.version is None, "--version cannot be combined with --uninstall")
        uninstall(prefix, launcher_dir)
        return
    target = native_target()
    with tempfile.TemporaryDirectory(prefix='marmish-download-') as temporary:
        directory = Path(temporary)
        version, release = release_metadata(options.version, directory)
        archive_name = 'marmish-' + version + '-' + target + '.tar.gz'
        assets = release_assets(release, archive_name)
        archive, sums = directory / archive_name, directory / 'SHA256SUMS'
        download(assets['SHA256SUMS'], sums, MAX_MANIFEST)
        download(assets[archive_name], archive, MAX_DOWNLOAD)
        verify_checksum(archive, sums, archive_name)
        manifest = install_archive(archive, prefix, launcher_dir, version, target)
    print('Installed Marmish ' + version + ' (' + target + '). EXPERIMENTAL:')
    for limitation in manifest['limitations']:
        print('  ' + limitation)
    print('Run marmish --doctor for static prerequisites; this does not validate runtime confinement.')
    print('Add this directory to PATH if needed: ' + shlex.quote(str(launcher_dir)))


if __name__ == '__main__':
    try:
        main()
    except (InstallError, OSError) as error:
        print('marmish installer: ' + str(error), file=sys.stderr)
        sys.exit(1)
# END MARMISH INSTALLER PYTHON
MARMISH_INSTALLER_PYTHON
