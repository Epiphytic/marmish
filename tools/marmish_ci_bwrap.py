"""Rootless CI-only authenticated Ubuntu Bubblewrap acquisition; no installation."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
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


def require(condition, category):
    if not condition:
        raise RuntimeError(category)


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
    from marmish_transport_sandbox import validated_bwrap
    executable = str(validated_bwrap())
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
    require('\n' not in os.environ['RUNNER_TEMP'] and '\r' not in os.environ['RUNNER_TEMP'], 'bwrap_invalid_temp')
    root = Path(tempfile.mkdtemp(prefix='marmish-bwrap-', dir=os.environ['RUNNER_TEMP'])).resolve()
    root.chmod(0o700)
    state_created = False
    try:
        with state.open('x') as pointer:
            state_created = True
            pointer.write(str(root))
        state.chmod(0o600)
        (root / 'owned-state').write_text(str(state.absolute()))
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
        (root / 'bin').mkdir()
        executable = root / 'bin/bwrap'
        extract(data, executable, arch)
        reported = runtime_version(executable, root)
        provenance = {'suite': suite, 'architecture': arch, 'package_version': version,
            'sources': sources(suite, arch).splitlines(), 'keyring_sha256': digest(KEYRING),
            'package_sha256': sha, 'executable_sha256': digest(executable), 'version': reported,
            'system_bwrap_present': Path('/usr/bin/bwrap').exists()}
        (root / 'provenance.json').write_text(json.dumps(provenance, sort_keys=True))
        with open(os.environ['GITHUB_ENV'], 'a') as output:
            output.write('MARMISH_BWRAP=' + str(executable) + '\nMARMISH_BWRAP_SHA256=' + digest(executable) + '\n')
        print(json.dumps(provenance, sort_keys=True))
    except BaseException:
        shutil.rmtree(root)
        if state_created:
            state.unlink(missing_ok=True)
        raise


def cleanup(state):
    require(not state.is_symlink(), 'bwrap_cleanup_ownership')
    if not state.exists():
        return
    info = state.stat()
    require(state.is_file() and info.st_uid == os.geteuid() and
            not info.st_mode & 0o077 and info.st_size <= 4096, 'bwrap_cleanup_ownership')
    root = Path(state.read_text())
    require(root.parent == Path(os.environ['RUNNER_TEMP']).resolve() and root.name.startswith('marmish-bwrap-')
            and not root.is_symlink() and root.stat().st_uid == os.geteuid()
            and root.stat().st_mode & 0o777 == 0o700 and
            (root / 'owned-state').read_text() == str(state.absolute()), 'bwrap_cleanup_ownership')
    shutil.rmtree(root)
    state.unlink()


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
