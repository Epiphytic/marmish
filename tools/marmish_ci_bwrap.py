"""Rootless CI-only authenticated Ubuntu Bubblewrap acquisition; no installation."""
import argparse
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import struct
import subprocess
import tarfile
import tempfile
import time

from marmish_ci import refuse_api_tokens, build_environment
from marmish_platform import continuous_time

# Raising these anti-rollback floors is a reviewed source change.
FLOORS = {'noble': '0.9.0-1ubuntu0.3', 'resolute': '0.11.1-1ubuntu0.3'}
KEYRING = Path('/usr/share/keyrings/ubuntu-archive-keyring.gpg')
DEADLINE = float('inf')
TRUSTED_PARENT = Path('/tmp')


def require(condition, category):
    if not condition:
        raise RuntimeError(category)


def trusted_parent():
    fd = os.open(TRUSTED_PARENT, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        info = os.fstat(fd)
        require(stat.S_ISDIR(info.st_mode) and info.st_uid == 0 and
                (not info.st_mode & 0o022 or info.st_mode & stat.S_ISVTX), 'bwrap_untrusted_parent')
        return fd
    except BaseException:
        os.close(fd)
        raise


def private_directory_fd(path):
    require(path.is_absolute() and '..' not in path.parts, 'bwrap_cleanup_ownership')
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for component in path.parts[1:]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            os.close(fd)
            fd = child
        return fd
    except BaseException:
        os.close(fd)
        raise


def prefix_fd(parent, name):
    require(re.fullmatch(r'marmish-bwrap-[0-9a-f]{32}', name) is not None, 'bwrap_cleanup_ownership')
    fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
    info = os.fstat(fd)
    if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
        os.close(fd)
        raise RuntimeError('bwrap_cleanup_ownership')
    return fd


def read_private(parent, name):
    fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=parent)
    with os.fdopen(fd, 'rb') as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid() and
                stat.S_IMODE(info.st_mode) == 0o600 and info.st_nlink == 1 and info.st_size <= 4096,
                'bwrap_cleanup_ownership')
        value = source.read(4097)
    require(len(value) <= 4096, 'bwrap_cleanup_ownership')
    return value.decode('utf-8')


def write_private(parent, name, value, *, on_create=None):
    fd = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                 0o600, dir_fd=parent)
    with os.fdopen(fd, 'w') as output:
        if on_create is not None:
            on_create()
        output.write(value)
        output.flush()
        os.fsync(output.fileno())


def remove_prefix(parent, name, identity):
    fd = prefix_fd(parent, name)
    try:
        info = os.fstat(fd)
        require((info.st_dev, info.st_ino) == identity, 'bwrap_cleanup_ownership')
        current = os.stat(name, dir_fd=parent, follow_symlinks=False)
        require((current.st_dev, current.st_ino) == identity, 'bwrap_cleanup_ownership')
        shutil.rmtree(name, dir_fd=parent)
    finally:
        os.close(fd)


def strip_work(fd):
    for name in os.listdir(fd):
        if name in ('bin', 'provenance.json', 'owned-state'):
            continue
        info = os.stat(name, dir_fd=fd, follow_symlinks=False)
        if stat.S_ISDIR(info.st_mode):
            shutil.rmtree(name, dir_fd=fd)
        else:
            os.unlink(name, dir_fd=fd)


@contextlib.contextmanager
def selected_environment(executable, sha):
    selected = {'MARMISH_BWRAP': str(executable), 'MARMISH_BWRAP_SHA256': sha}
    previous = {key: os.environ.get(key) for key in selected}
    try:
        os.environ.update(selected)
        yield
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value


def validation_diagnostic():
    """Read-only first failure, deliberately excluding all component names."""
    row = {'category': 'selection', 'index': 0, 'uid': None, 'mode': None, 'errno': None}
    raw = os.environ.get('MARMISH_BWRAP', '/usr/bin/bwrap')
    path = Path(raw)
    if not path.is_absolute() or '..' in path.parts:
        return row
    fd = None
    try:
        fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
        for index, component in enumerate((*path.parts[1:-1], None)):
            row.update(index=index, category='ancestor_io', uid=None, mode=None, errno=None)
            info = os.fstat(fd)
            row.update(uid=info.st_uid, mode=stat.S_IMODE(info.st_mode))
            if info.st_uid not in (0, os.geteuid()):
                row['category'] = 'ancestor_owner'
                return row
            if info.st_mode & 0o022 and not (info.st_uid == 0 and info.st_mode & stat.S_ISVTX):
                row['category'] = 'ancestor_mode'
                return row
            if component is not None:
                row.update(index=index + 1, uid=None, mode=None)
                child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
                os.close(fd)
                fd = child
        row.update(index=len(path.parts) - 1, category='executable_io', uid=None, mode=None)
        binary = os.open(path.name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=fd)
        try:
            info = os.fstat(binary)
            row.update(uid=info.st_uid, mode=stat.S_IMODE(info.st_mode), category='executable_other')
            if not stat.S_ISREG(info.st_mode):
                row['category'] = 'executable_type'
            elif info.st_uid not in (0, os.geteuid()):
                row['category'] = 'executable_owner'
            elif info.st_mode & (0o022 | stat.S_ISUID | stat.S_ISGID) or not info.st_mode & 0o111:
                row['category'] = 'executable_mode'
        finally:
            os.close(binary)
    except OSError as error:
        row['errno'] = error.errno
    finally:
        if fd is not None:
            os.close(fd)
    return row


def checked_selection():
    from marmish_transport_sandbox import validated_bwrap, SandboxError
    try:
        return validated_bwrap()
    except SandboxError:
        print(json.dumps({'bwrap_validation': validation_diagnostic()}, sort_keys=True))
        raise


def tmp_filesystem():
    """Bounded, public mount facts for literal /tmp; unknown is not a failure."""
    unknown = {'fstype': 'unknown', 'flags': []}
    if platform.system() != 'Linux':
        return unknown
    try:
        with open('/proc/self/mountinfo', 'rb') as source:
            body = source.read(256 * 1024 + 1)
        if len(body) > 256 * 1024:
            return unknown
        selected = None
        for line in body.decode('ascii').splitlines():
            before, after = line.split(' - ')
            fields, tail = before.split(), after.split()
            if len(fields) < 6 or len(tail) < 3:
                return unknown
            mount = re.sub(r'\\(040|011|012|134)', lambda match: chr(int(match[1], 8)), fields[4])
            if mount != '/' and mount != '/tmp':
                continue
            if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]{0,31}', tail[0]):
                return unknown
            flags = set(fields[5].split(','))
            allowed = {'ro', 'rw', 'noexec', 'nosuid', 'nodev', 'relatime', 'noatime', 'strictatime'}
            if selected is None or len(mount) >= selected[0]:
                selected = (len(mount), {'fstype': tail[0], 'flags': sorted(flags & allowed)})
        return selected[1] if selected is not None else unknown
    except (OSError, UnicodeError, ValueError):
        return unknown


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv, *, root, timeout=30, limit=1024 * 1024, env=None, cwd=None):
    """Bound the complete process group, output and owned metadata storage."""
    with tempfile.TemporaryFile() as output:
        child = subprocess.Popen(argv, stdout=output, stderr=subprocess.STDOUT,
            start_new_session=True, env=env or build_environment(), cwd=cwd)
        deadline = min(DEADLINE, continuous_time() + timeout)
        try:
            while child.poll() is None:
                require(continuous_time() < deadline, 'bwrap_tool_timeout')
                require(output.tell() <= limit, 'bwrap_tool_output_oversized')
                check_storage(root)
                time.sleep(.05)
            check_storage(root)
            output.seek(0)
            body = output.read(limit + 1)
            require(len(body) <= limit, 'bwrap_tool_output_oversized')
            require(child.returncode == 0, 'bwrap_tool_failed')
            return body
        finally:
            # A producer may leave descendants even after its leader exits.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)


def check_storage(root):
    size = 0
    for path in root.rglob('*'):
        try:
            if path.is_file():
                size += path.stat().st_size
        except FileNotFoundError:
            pass  # APT atomically publishes/removes its private temporary files.
    require(size <= 128 * 1024 * 1024, 'bwrap_metadata_oversized')


def sources(suite, arch):
    origin = 'https://archive.ubuntu.com/ubuntu' if arch == 'amd64' else 'https://ports.ubuntu.com/ubuntu-ports'
    security = 'https://security.ubuntu.com/ubuntu' if arch == 'amd64' else origin
    return '\n'.join(f'deb [arch={arch} signed-by={KEYRING}] {url} {pocket} main'
        for url, pocket in [(origin, suite), (origin, suite + '-updates'),
                            (security, suite + '-security')]) + '\n'


def apt_config(root, suite, arch):
    for name in ('empty', 'lists/partial', 'cache/archives/partial', 'downloads'):
        (root / name).mkdir(parents=True)
    (root / 'sources.list').write_text(sources(suite, arch))
    for name in ('status', 'trusted.gpg', 'main.conf', 'preferences', 'auth.conf'):
        (root / name).touch()
    settings = {'Dir::Etc::main': root / 'main.conf', 'Dir::Etc::parts': root / 'empty',
        'Dir::Etc::sourcelist': root / 'sources.list', 'Dir::Etc::sourceparts': root / 'empty',
        'Dir::Etc::trusted': root / 'trusted.gpg', 'Dir::Etc::trustedparts': root / 'empty',
        'Dir::Etc::preferences': root / 'preferences', 'Dir::Etc::preferencesparts': root / 'empty',
        'Dir::Etc::netrc': root / 'auth.conf', 'Dir::Etc::netrcparts': root / 'empty',
        'Dir::State::lists': root / 'lists', 'Dir::State::status': root / 'status',
        'Dir::Cache': root / 'cache', 'APT::Architecture': arch,
        'APT::Update::Error-Mode': 'any', 'Acquire::Languages': 'none',
        'Acquire::IndexTargets::deb::DEP-11::DefaultEnabled': 'false',
        'Acquire::IndexTargets::deb::CNF::DefaultEnabled': 'false',
        'Acquire::Check-Date': 'true', 'Acquire::AllowInsecureRepositories': 'false',
        'Acquire::AllowDowngradeToInsecureRepositories': 'false',
        'APT::Get::AllowUnauthenticated': 'false', 'Acquire::https::Timeout': '30',
        'Acquire::Retries': '1'}
    config = root / 'apt.conf'
    config.write_text('\n'.join(f'{key} "{value}";' for key, value in settings.items()))
    return dict(build_environment(), APT_CONFIG=str(config))


def metadata(body, version, arch):
    records = []
    for block in body.decode().split('\n\n'):
        row = dict(line.split(': ', 1) for line in block.splitlines() if ': ' in line and not line.startswith(' '))
        if row.get('Version') == version:
            records.append(tuple(row.get(key) for key in ('Package', 'Version', 'Architecture', 'Filename', 'Size', 'SHA256')))
    require(records and len(set(records)) == 1, 'bwrap_conflicting_package_metadata')
    package, _version, architecture, filename, size, sha = records[0]
    require(package == 'bubblewrap' and architecture == arch and
            re.fullmatch(r'pool/main/b/bubblewrap/[A-Za-z0-9_.+~-]+\.deb', filename or '') and
            size and size.isdigit() and 0 < int(size) <= 2 * 1024 * 1024 and
            re.fullmatch('[0-9a-f]{64}', sha or ''), 'bwrap_invalid_package_metadata')
    return int(size), sha


def extract(body, target, arch):
    require(len(body) <= 16 * 1024 * 1024, 'bwrap_tar_oversized')
    with tarfile.open(fileobj=io.BytesIO(body), mode='r:') as archive:
        members = archive.getmembers()
        require(len(members) <= 256, 'bwrap_tar_oversized')
        matches = [entry for entry in members if entry.name in ('./usr/bin/bwrap', 'usr/bin/bwrap')]
        require(len(matches) == 1, 'bwrap_invalid_payload')
        entry = matches[0]
        require(entry.isfile() and not entry.mode & 0o6000 and 20 <= entry.size <= 1024 * 1024,
                'bwrap_invalid_payload')
        data = archive.extractfile(entry).read(1024 * 1024 + 1)
    require(data[:6] == b'\x7fELF\x02\x01' and
            struct.unpack('<H', data[18:20])[0] == {'amd64': 62, 'arm64': 183}[arch],
            'bwrap_wrong_elf_architecture')
    with target.open('xb') as stream:
        stream.write(data)
    target.chmod(0o755)


def environment():
    """Forward an explicitly verified supervisor input; never change PATH."""
    keys = ('MARMISH_BWRAP', 'MARMISH_BWRAP_SHA256')
    if not any(key in os.environ for key in keys):
        return {}
    from marmish_transport_sandbox import validated_bwrap
    validated_bwrap()
    return {key: os.environ[key] for key in keys}


def provenance():
    selected = environment()
    if not selected:
        return None
    return json.loads((Path(selected['MARMISH_BWRAP']).parent.parent / 'provenance.json').read_text())


def probe():
    executable = str(checked_selection())
    args = [executable, '--unshare-user', '--unshare-net', '--unshare-pid',
            '--ro-bind', '/', '/', '--proc', '/proc', '--dev', '/dev']
    with tempfile.TemporaryDirectory(prefix='marmish-bwrap-probe-') as temporary:
        try:
            run(['/usr/bin/unshare', '--user', '--map-root-user', '--net',
                 *args, *args, '/usr/bin/true'], root=Path(temporary), timeout=30)
        except RuntimeError as error:
            if str(error) == 'bwrap_tool_failed':
                raise RuntimeError('userns_denied_private_bwrap') from None
            raise


def runtime_version(executable, root):
    try:
        return run([str(executable), '--version'], root=root).decode().strip()
    except OSError:
        raise RuntimeError('bwrap_execution_denied') from None
    except RuntimeError as error:
        if str(error) == 'bwrap_tool_failed':
            raise RuntimeError('bwrap_runtime_check_failed') from None
        raise  # Keep deadline, output and storage quota failures truthful.


def bootstrap(state):
    global DEADLINE
    DEADLINE = continuous_time() + 300
    refuse_api_tokens()
    require(os.geteuid() != 0, 'bwrap_root_refused')
    require(not any(key in os.environ for key in ('MARMISH_BWRAP', 'MARMISH_BWRAP_SHA256')),
            'bwrap_preexisting_override')
    require(not state.exists() and not state.is_symlink(), 'bwrap_state_exists')
    release = dict(line.split('=', 1) for line in Path('/etc/os-release').read_text().splitlines() if '=' in line)
    suite = release.get('VERSION_CODENAME', '').strip('"')
    arch = {'x86_64': 'amd64', 'aarch64': 'arm64'}.get(platform.machine())
    require(release.get('ID', '').strip('"') == 'ubuntu' and suite in FLOORS and arch,
            'bwrap_unsupported_platform')
    info = KEYRING.lstat()
    require(KEYRING.is_file() and not KEYRING.is_symlink() and info.st_uid == 0 and
            not info.st_mode & 0o022, 'bwrap_unsafe_keyring')
    for tool in ('/usr/bin/apt-get', '/usr/bin/apt-cache', '/usr/bin/dpkg',
                 '/usr/bin/dpkg-deb', '/usr/bin/unshare', '/usr/sbin/ip', '/usr/sbin/sysctl'):
        require(Path(tool).is_file() and os.access(tool, os.X_OK), 'bwrap_missing_system_tool')
    parent = trusted_parent()
    name = 'marmish-bwrap-' + os.urandom(16).hex()
    root = TRUSTED_PARENT / name
    state = state.absolute()
    state_created = False
    created = False
    identity = None
    state_parent = None
    prefix = None
    def mark_state_created():
        nonlocal state_created
        state_created = True
    try:
        os.mkdir(name, mode=0o700, dir_fd=parent)
        created = True
        prefix = prefix_fd(parent, name)
        info = os.fstat(prefix)
        identity = (info.st_dev, info.st_ino)
        state_parent = private_directory_fd(state.parent)
        write_private(state_parent, state.name, json.dumps({'root': str(root), 'device': identity[0], 'inode': identity[1]}),
                      on_create=mark_state_created)
        write_private(prefix, 'owned-state', str(state))
        env = apt_config(root, suite, arch)
        run(['/usr/bin/apt-get', 'update'], root=root, env=env, timeout=180)
        policy = run(['/usr/bin/apt-cache', 'policy', 'bubblewrap:' + arch], root=root, env=env).decode()
        candidate = re.search(r'^\s*Candidate: ([A-Za-z0-9.+:~_-]+)$', policy, re.M)
        require(candidate is not None, 'bwrap_missing_candidate')
        version = candidate[1]
        try:
            run(['/usr/bin/dpkg', '--compare-versions', version, 'ge', FLOORS[suite]], root=root)
        except RuntimeError as error:
            if str(error) == 'bwrap_tool_failed':
                raise RuntimeError('bwrap_candidate_below_floor') from None
            raise
        body = run(['/usr/bin/apt-cache', 'show', 'bubblewrap:' + arch + '=' + version], root=root, env=env)
        size, sha = metadata(body, version, arch)
        run(['/usr/bin/apt-get', 'download', 'bubblewrap:' + arch + '=' + version],
            root=root, env=env, timeout=60, cwd=root / 'downloads')
        packages = list((root / 'downloads').iterdir())
        require(len(packages) == 1 and packages[0].is_file() and packages[0].stat().st_size == size and
                digest(packages[0]) == sha, 'bwrap_package_digest_mismatch')
        data = run(['/usr/bin/dpkg-deb', '--fsys-tarfile', str(packages[0])],
                   root=root, limit=16 * 1024 * 1024)
        (root / 'bin').mkdir(mode=0o700)
        executable = root / 'bin/bwrap'
        extract(data, executable, arch)
        reported = runtime_version(executable, root)
        provenance = {'suite': suite, 'architecture': arch, 'package_version': version,
            'sources': sources(suite, arch).splitlines(), 'keyring_sha256': digest(KEYRING),
            'package_sha256': sha, 'executable_sha256': digest(executable), 'version': reported,
            'system_bwrap_present': Path('/usr/bin/bwrap').exists(), 'tmp_filesystem': tmp_filesystem()}
        (root / 'provenance.json').write_text(json.dumps(provenance, sort_keys=True))
        with selected_environment(executable, digest(executable)):
            checked_selection()
        strip_work(prefix)
        with open(os.environ['GITHUB_ENV'], 'a') as output:
            output.write('MARMISH_BWRAP=' + str(executable) + '\nMARMISH_BWRAP_SHA256=' + digest(executable) + '\n')
        print(json.dumps(provenance, sort_keys=True))
    except BaseException:
        # Preserve the original failure even when cleanup itself cannot prove ownership.
        removed = False
        if created and identity is not None:
            try:
                remove_prefix(parent, name, identity)
                removed = True
            except (OSError, RuntimeError):
                pass
        elif created:
            with contextlib.suppress(OSError):
                os.rmdir(name, dir_fd=parent)
        if state_created and removed:
            with contextlib.suppress(OSError):
                os.unlink(state.name, dir_fd=state_parent)
        raise
    finally:
        for fd in (prefix, state_parent, parent):
            if fd is not None:
                os.close(fd)


def cleanup(state):
    state = state.absolute()
    state_parent = private_directory_fd(state.parent)
    parent = prefix = None
    try:
        try:
            value = json.loads(read_private(state_parent, state.name))
        except FileNotFoundError:
            return
        require(isinstance(value, dict) and set(value) == {'root', 'device', 'inode'} and
                type(value['device']) is int and type(value['inode']) is int and
                isinstance(value['root'], str), 'bwrap_cleanup_ownership')
        root = Path(value['root'])
        require(root.is_absolute() and '..' not in root.parts and root.parent == TRUSTED_PARENT,
                'bwrap_cleanup_ownership')
        parent = trusted_parent()
        prefix = prefix_fd(parent, root.name)
        require(read_private(prefix, 'owned-state') == str(state), 'bwrap_cleanup_ownership')
        remove_prefix(parent, root.name, (value['device'], value['inode']))
        os.unlink(state.name, dir_fd=state_parent)
    finally:
        for fd in (prefix, parent, state_parent):
            if fd is not None:
                os.close(fd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('bootstrap', 'probe', 'cleanup'))
    parser.add_argument('--state', type=Path)
    args = parser.parse_args()
    try:
        {'bootstrap': lambda: bootstrap(args.state), 'probe': probe,
         'cleanup': lambda: cleanup(args.state)}[args.operation]()
    except (RuntimeError, OSError, ValueError, KeyError, tarfile.TarError) as error:
        category = str(error) if isinstance(error, RuntimeError) else 'bwrap_local_io_failed'
        print(json.dumps({'outcome': 'failed', 'error': category}))
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
