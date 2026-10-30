#!/usr/bin/env python3
"""Disposable managed-Python packaging proof; never a production installer."""
import argparse
import copy
import contextlib
import gzip
import json
import os
from pathlib import Path
import platform
import re
import resource
import shlex
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import traceback
import urllib.parse

from marmish_installed import file_digest as digest

REPO = Path(__file__).resolve().parent.parent
FIXTURE = REPO / 'tests/fixtures/managed-runtime-assets.json'
HOSTS = frozenset({'github.com', 'release-assets.githubusercontent.com', 'objects.githubusercontent.com'})
CAPS = {'uv': 64 << 20, 'python': 128 << 20, 'marmish': 512 << 20}
TARGETS = {('Linux', 'x86_64'): 'x86_64-unknown-linux-gnu',
           ('Linux', 'aarch64'): 'aarch64-unknown-linux-gnu',
           ('Darwin', 'arm64'): 'aarch64-apple-darwin'}


class Refused(Exception):
    pass


def require(condition, category):
    if not condition:
        raise Refused(category)


def checked_url(url):
    try:
        parsed = urllib.parse.urlsplit(url)
        valid = (parsed.scheme == 'https' and parsed.hostname in HOSTS
                 and parsed.port in (None, 443) and not parsed.username
                 and not parsed.password and not parsed.fragment
                 and not any(ord(c) < 32 for c in url))
    except ValueError:
        valid = False
    require(valid, 'download_url_refused')
    return url


BWRAP_CATEGORIES = frozenset({'invalid_bwrap_selection', 'unsafe_bwrap_path',
                              'unsafe_bwrap_executable', 'bwrap_hash_mismatch'})
CI_PLATFORMS = {'linux-x64': ('x86_64-unknown-linux-gnu', ['self-hosted', 'Linux', 'X64', 'bel-ci']),
                'linux-arm64': ('aarch64-unknown-linux-gnu', ['self-hosted', 'Linux', 'ARM64', 'laptop-ci']),
                'macos-arm64': ('aarch64-apple-darwin', ['self-hosted', 'macOS', 'ARM64', 'laptop-ci'])}


def bwrap_validation(operation):
    from marmish_transport_sandbox import SandboxError
    try:
        return operation()
    except SandboxError as error:
        category = str(error) if str(error) in BWRAP_CATEGORIES else 'unknown'
        raise Refused('bwrap_selection:' + category) from None


def bwrap_executable():
    from marmish_transport_sandbox import validated_bwrap
    return bwrap_validation(validated_bwrap)


def verified_helper_pair():
    from marmish_ci_bwrap import environment as helper_environment
    return bwrap_validation(helper_environment)


def helper_provenance(pair):
    require(str(bwrap_executable()) == pair['MARMISH_BWRAP'], 'bwrap_provenance_invalid')
    path = Path(pair['MARMISH_BWRAP']).parent.parent / 'provenance.json'
    try:
        descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
        with os.fdopen(descriptor, 'rb') as source:
            info = os.fstat(source.fileno())
            require(stat.S_ISREG(info.st_mode) and info.st_uid in (0, os.geteuid())
                    and info.st_size <= 65536, 'bwrap_provenance_invalid')
            raw = source.read(65537)
            require(len(raw) <= 65536, 'bwrap_provenance_invalid')
            value = json.loads(raw)
        require(isinstance(value, dict), 'bwrap_provenance_invalid')
        names = ('suite', 'architecture', 'package_version', 'keyring_sha256', 'package_sha256',
                 'executable_sha256', 'version')
        result = {name: value[name] for name in names}
        architecture = {'x86_64': 'amd64', 'aarch64': 'arm64'}.get(platform.machine())
        require(result['suite'] in ('noble', 'resolute') and result['architecture'] == architecture,
                'bwrap_provenance_invalid')
        patterns = {'package_version': r'[A-Za-z0-9.+:~_-]{1,128}',
                    'version': r'bubblewrap [0-9]+(?:\.[0-9]+){1,3}',
                    **{key: '[0-9a-f]{64}' for key in ('keyring_sha256', 'package_sha256', 'executable_sha256')}}
        require(all(isinstance(result[key], str) and re.fullmatch(pattern, result[key])
                    for key, pattern in patterns.items()) and len(result['version']) <= 64
                and result['executable_sha256'] == pair['MARMISH_BWRAP_SHA256'], 'bwrap_provenance_invalid')
        return result
    except (OSError, ValueError, KeyError, TypeError):
        raise Refused('bwrap_provenance_invalid') from None


def initialize_helper(required, report):
    if platform.system() == 'Darwin':
        require(not required and not any(name.startswith('MARMISH_BWRAP') for name in os.environ),
                'bwrap_configuration_on_darwin')
        return
    pair = verified_helper_pair()
    require(not required or bool(pair), 'bwrap_helper_required')
    bwrap_executable()  # Validate system selection too, never fallback after refusal.
    report['bwrap_source'] = 'helper' if pair else 'system'
    if pair:
        report['bwrap_helper'] = helper_provenance(pair)


def test_supervisor(env):
    if platform.system() == 'Darwin':
        return dict(env)
    return dict(env, **verified_helper_pair())


def ci_context(raw, target):
    fields = raw.split(':')
    require(len(fields) == 4, 'ci_context_invalid')
    sha, run_id, attempt, native = fields
    require(re.fullmatch('[0-9a-f]{40}', sha) and re.fullmatch('[1-9][0-9]{0,19}', run_id)
            and re.fullmatch('[1-9][0-9]{0,5}', attempt) and native in CI_PLATFORMS
            and CI_PLATFORMS[native][0] == target, 'ci_context_invalid')
    return {'source_commit': sha, 'run_id': run_id, 'attempt': attempt, 'platform': native,
            'workflow': 'native-validation', 'runner_class': CI_PLATFORMS[native][1]}


def host_facts():
    result = {'system': platform.system(), 'architecture': platform.machine()}
    if platform.system() == 'Linux':
        try:
            distribution = platform.freedesktop_os_release()
        except OSError:
            distribution = {}
        result['distribution'] = {name.lower(): value if re.fullmatch('[A-Za-z0-9._-]{1,64}', value) else None
                                  for name in ('ID', 'VERSION_ID')
                                  for value in [distribution.get(name, '')]}
    else:
        version = platform.mac_ver()[0]
        result['os_version'] = version if re.fullmatch(r'[0-9]+(?:\.[0-9]+){0,3}', version) else None
    return result


def interrupted(signum, frame):
    raise Refused('total_timeout' if signum == signal.SIGALRM else 'interrupted')


def install_interrupt_handlers(timeout):
    require(type(timeout) is int and 1 <= timeout <= 1320, 'total_timeout_invalid')
    old_handlers = {number: signal.getsignal(number) for number in (signal.SIGALRM, signal.SIGTERM)}
    for number in old_handlers:
        signal.signal(number, interrupted)
    old_timer = signal.setitimer(signal.ITIMER_REAL, timeout)
    return old_handlers, old_timer, time.monotonic()


def restore_interrupt_handlers(state):
    old_handlers, (delay, interval), started = state
    signal.setitimer(signal.ITIMER_REAL, 0)
    for number, handler in old_handlers.items():
        signal.signal(number, handler)
    if delay:
        signal.setitimer(signal.ITIMER_REAL, max(0.001, delay - (time.monotonic() - started)), interval)


def environment(root):
    """Deliberately does not inherit UV/PIP/PYTHON/loader or runner variables."""
    result = {'PATH': '/usr/bin:/bin', 'LANG': 'C', 'LC_ALL': 'C'}
    for name, relative in [('HOME', 'home'), ('XDG_CACHE_HOME', 'cache'),
                           ('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'),
                           ('TMPDIR', 'tmp')]:
        directory = root / relative
        directory.mkdir(mode=0o700, exist_ok=True)
        result[name] = str(directory)
    return result


def regular_entry(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        # uv atomically renames/removes owned staging files during monitoring.
        return None
    return info if stat.S_ISREG(info.st_mode) else None


def observe_entry(observer, path, info):
    if observer is None:
        return False
    try:
        observer(path, info)
        return observer.observed
    except Exception:
        raise Refused('interruption_observer_failed') from None


def total_bytes(root, observer=None):
    total = 0
    for count, path in enumerate(root.rglob('*'), 1):
        require(count <= 50000, 'runtime_entry_count_limit')
        info = regular_entry(path)
        if info is not None:
            total += info.st_size
            if observe_entry(observer, path, info):
                break
    return total


def checked_cwd(env, cwd):
    root = Path(env['HOME']).parent.resolve(strict=True)
    selected = Path(env['HOME']) if cwd is None else Path(cwd)
    require(selected.is_absolute() and not selected.is_symlink(), 'child_cwd_refused')
    try:
        info = selected.stat()
    except OSError:
        raise Refused('child_cwd_refused') from None
    require(stat.S_ISDIR(info.st_mode) and info.st_uid == os.geteuid()
            and selected.resolve(strict=True).is_relative_to(root), 'child_cwd_refused')
    return selected


def require_group_gone(pid):
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        try:
            os.killpg(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.01)
    raise Refused('interruption_group_survived')


def run(command, env, *, timeout=120, monitor=None, limit=1 << 30, capture=False,
        cwd=None, observer=None):
    selected = checked_cwd(env, cwd)
    return _run_process(command, env, timeout=timeout, monitor=monitor, limit=limit,
                        capture=capture, cwd=selected, observer=observer)


def interrupt_observed(child, observer, deadline):
    if not observer.observed:
        require(time.monotonic() < deadline, 'interruption_observation_timeout')
        return False
    require(child.poll() is None, 'finished_before_interruption')
    try:
        os.killpg(child.pid, signal.SIGKILL)
    except ProcessLookupError:
        raise Refused('finished_before_interruption') from None
    child.wait(timeout=2)
    require(child.returncode == -signal.SIGKILL, 'interruption_not_sigkill')
    require_group_gone(child.pid)
    return True


def _run_process(command, env, *, timeout=120, monitor=None, limit=1 << 30, capture=False,
                 cwd, observer=None):
    """Bounded private output; terminate/reap owned process group on every failure."""
    require(observer is None or monitor is not None, 'interruption_monitor_required')
    with tempfile.TemporaryFile() as output:
        child = subprocess.Popen(list(map(str, command)), env=env, stdin=subprocess.DEVNULL,
                                 stdout=output, stderr=output, close_fds=True,
                                 start_new_session=True, cwd=cwd)
        deadline = time.monotonic() + timeout
        observation_deadline = min(deadline, time.monotonic() + 10)
        try:
            while child.poll() is None:
                require(time.monotonic() < deadline, 'child_timeout')
                require(os.fstat(output.fileno()).st_size <= 2 << 20, 'child_output_limit')
                if monitor is not None:
                    require(total_bytes(monitor, observer) <= limit, 'runtime_expansion_limit')
                if observer is not None and interrupt_observed(child, observer, observation_deadline):
                    break
                time.sleep(0.01 if observer is not None else 0.05)
            require(observer is None or observer.observed, 'finished_before_interruption')
            if monitor is not None:
                require(total_bytes(monitor) <= limit, 'runtime_expansion_limit')
            require(os.fstat(output.fileno()).st_size <= 2 << 20, 'child_output_limit')
            output.seek(0)
            data = output.read(2 << 20) if capture else b''
            return child.returncode, data
        finally:
            # The leader may have exited while an owned descendant remains.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
            if observer is not None:
                require_group_gone(child.pid)


def download(url, destination, limit, env):
    """Follow at most five explicitly validated redirects, never curl -L."""
    require(not destination.exists(), 'download_destination_exists')
    with tempfile.TemporaryDirectory(prefix='fetch-', dir=destination.parent) as temporary:
        stage = Path(temporary)
        body, response = stage / 'body', stage / 'response'
        for attempt in range(6):
            checked_url(url)
            def file_limit():
                resource.setrlimit(resource.RLIMIT_FSIZE, (limit, limit))
            with response.open('wb') as metadata:
                result = subprocess.run(
                    ['/usr/bin/curl', '--proto', '=https', '--tlsv1.2', '--silent', '--show-error',
                     '--connect-timeout', '15', '--max-time', '180', '--max-filesize', str(limit),
                     '--output', str(body), '--write-out', '%{http_code}\n%{redirect_url}\n', url],
                    env=env, stdin=subprocess.DEVNULL, stdout=metadata, stderr=subprocess.DEVNULL,
                    cwd=env['HOME'],
                    timeout=190, close_fds=True, preexec_fn=file_limit)
            require(result.returncode == 0, 'download_failed')
            require(body.stat().st_size <= limit and response.stat().st_size <= 4096,
                    'download_size_limit')
            fields = response.read_text().splitlines()
            require(fields and re.fullmatch('[0-9]{3}', fields[0]), 'download_response_invalid')
            if fields[0] == '200':
                os.replace(body, destination)
                return
            require(fields[0] in ('301', '302', '303', '307', '308') and len(fields) == 2
                    and attempt < 5, 'download_redirect_refused')
            url = checked_url(fields[1])
            body.unlink()
    raise Refused('download_failed')


def verify_asset(path, record, kind, env):
    require(type(record.get('size')) is int and 0 < record['size'] <= CAPS[kind], 'asset_size_invalid')
    require(isinstance(record.get('sha256'), str) and re.fullmatch('[0-9a-f]{64}', record['sha256']),
            'asset_hash_required')
    require(path.is_file() and not path.is_symlink() and path.stat().st_size == record['size'],
            'asset_size_mismatch')
    command = ['/usr/bin/shasum', '-a', '256'] if platform.system() == 'Darwin' else ['/usr/bin/sha256sum']
    status, output = run([*command, path], env, capture=True)
    require(status == 0 and output.split(maxsplit=1)[0].decode() == record['sha256'], 'asset_hash_mismatch')



def verify_architecture(path, target):
    with path.open('rb') as source:
        header = source.read(32)
    if target.endswith('darwin'):
        valid = (len(header) >= 8 and header[:4] == b'\xcf\xfa\xed\xfe'
                 and int.from_bytes(header[4:8], 'little') == 0x100000c)
    else:
        machine = 62 if target.startswith('x86_64-') else 183
        valid = (len(header) >= 20 and header[:6] == b'\x7fELF\x02\x01'
                 and int.from_bytes(header[18:20], 'little') == machine)
    require(valid, 'native_architecture_mismatch')


def extract_uv(archive, expected, destination):
    """Read exactly one regular member; no tar extraction API or path writes."""
    found = False
    with tarfile.open(archive, 'r:gz') as source:
        for member in source:
            require(member.name in (expected, expected.rsplit('/', 1)[0],
                                    expected.rsplit('/', 1)[0] + '/uvx'), 'uv_unexpected_member')
            require(not member.issym() and not member.islnk() and
                    (member.isfile() or member.isdir()), 'uv_unsafe_member')
            if member.name == expected:
                require(not found and member.isfile() and 0 < member.size <= 64 << 20,
                        'uv_member_invalid')
                with source.extractfile(member) as reader, destination.open('xb') as writer:
                    shutil.copyfileobj(reader, writer)
                found = True
    require(found, 'uv_member_missing')
    destination.chmod(0o700)


def local_metadata(row, archive, output):
    entry = copy.deepcopy(row['python_entry'])
    require(isinstance(entry.get('sha256'), str) and re.fullmatch('[0-9a-f]{64}', entry['sha256']),
            'metadata_hash_required')
    require(entry['sha256'] == row['python']['sha256'], 'metadata_hash_mismatch')
    entry['url'] = archive.as_uri()
    output.write_text(json.dumps({row['metadata_key']: entry}, sort_keys=True))
    output.chmod(0o600)


class UvInstallFailure(Refused):
    def __init__(self, output):
        super().__init__('uv_install_failed')
        self.output = output


def install_python(uv, row, archive, slot, metadata, env, *, after_rehash=None, cwd=None):
    require(not (slot / 'python').exists() and not (slot / 'python').is_symlink(), 'runtime_slot_exists')
    require(archive.is_file() and not archive.is_symlink(), 'python_archive_missing')
    archive.chmod(0o400)
    verify_asset(archive, row['python'], 'python', env)
    if after_rehash is not None:
        after_rehash()
    local_metadata(row, archive, metadata)
    status, raw = run(uv_command(uv, row, slot, metadata), env, monitor=Path(env['HOME']).parent,
                      capture=True, cwd=cwd)
    if status != 0:
        raise UvInstallFailure(raw)
    interpreter = discover_interpreter(slot)
    base = (slot / 'python').resolve()
    target = TARGETS.get((platform.system(), platform.machine()))
    verify_architecture(interpreter, target)
    code = ('import json,platform,sys;print(json.dumps({"implementation":sys.implementation.name,'
            '"version":list(sys.version_info[:3]),"machine":platform.machine(),'
            '"executable":sys.executable,"prefix":sys.base_prefix,"prefix_equal":sys.prefix==sys.base_prefix}))')
    status, raw = run([interpreter, '-I', '-B', '-c', code], env, capture=True)
    require(status == 0, 'managed_python_failed')
    identity = json.loads(raw)
    require(identity['implementation'] == 'cpython' and identity['version'] == [3, 13, 13]
            and identity['prefix_equal'] and Path(identity['prefix']).resolve().is_relative_to(base)
            and Path(identity['executable']).resolve() == interpreter
            and identity['machine'] == platform.machine(), 'managed_python_identity_mismatch')
    return interpreter



def discover_interpreter(slot):
    base = (slot / 'python').resolve()
    for path in base.rglob('*'):
        if path.is_symlink():
            require(path.resolve().is_relative_to(base), 'runtime_link_escape')
    # uv can expose both a minor-version directory link and its versioned target.
    interpreters = sorted({path.resolve() for path in base.glob('*/bin/python3.13')})
    require(len(interpreters) == 1, 'interpreter_selection_invalid')
    interpreter = interpreters[0]
    require(interpreter.is_relative_to(base), 'interpreter_escape')
    return interpreter


def uv_command(uv, row, slot, metadata):
    return [uv, '--no-config', '--offline', '--no-cache', 'python', 'install',
            '--managed-python', '--no-bin', '--no-registry', '--install-dir', slot / 'python',
            '--python-downloads-json-url', metadata, row['metadata_key']]


def embedded_verifier(destination):
    source = (REPO / 'scripts/install.sh').read_bytes()
    begin, end = b'# BEGIN MARMISH INSTALLER PYTHON\n', b'# END MARMISH INSTALLER PYTHON'
    require(source.count(begin) == source.count(end) == 1, 'installer_marker_ambiguous')
    body = source.split(begin)[1].split(end)[0]
    destination.write_bytes(body)
    require(destination.read_bytes() == body, 'installer_bytes_changed')
    return digest(destination)


def install_payload(interpreter, verifier, archive, slot, fixture, target, env):
    # The exact verifier remains unchanged; this separate stdlib driver calls it.
    code = ('import pathlib,runpy,sys,shutil,json;'
            'm=runpy.run_path(sys.argv[1],run_name="verified_installer");'
            'slot=pathlib.Path(sys.argv[3]);stage=slot/"extract";stage.mkdir();'
            'm["inflate"](pathlib.Path(sys.argv[2]),stage/"archive.tar");'
            'root,payload=m["extract_ustar"](stage/"archive.tar",stage,sys.argv[4],sys.argv[5]);'
            'r=m["validate_manifest"](root,payload,sys.argv[4],sys.argv[5]);'
            'm["require"](r["source_commit"]==sys.argv[6],"source_commit_mismatch");'
            'destination=slot/root.name;shutil.move(root,destination);shutil.rmtree(stage);'
            'print(json.dumps({"files":len(payload),"root_name":destination.name,"prerequisites":r["prerequisites"]}))')
    status, raw = run([interpreter, '-I', '-B', '-c', code, verifier, archive, slot,
                       fixture['marmish_version'], target, fixture['marmish_source_commit']],
                      env, timeout=180, capture=True)
    require(status == 0, 'embedded_archive_verifier_failed')
    return json.loads(raw)



def audit_runtime(slot, interpreter, env):
    root = slot / 'python'
    files = [p for p in root.rglob('*') if p.is_file() and not p.is_symlink()]
    require(len(files) <= 20000, 'runtime_file_count_limit')
    binaries = {interpreter}
    binaries.update(p for p in files if '.so' in p.name or p.name.endswith('.dylib'))
    require(len(binaries) <= 512, 'runtime_native_count_limit')
    libraries, versions = set(), set()
    for binary in sorted(binaries):
        if platform.system() == 'Linux':
            status, raw = run(['/usr/bin/readelf', '-d', binary], env, capture=True)
            require(status == 0, 'runtime_dependency_audit_failed')
            libraries.update(name.decode() for name in re.findall(rb'\(NEEDED\).*\[([A-Za-z0-9_.+-]+)\]', raw))
            status, raw = run(['/usr/bin/readelf', '--version-info', binary], env, capture=True)
            require(status == 0, 'runtime_dependency_audit_failed')
            versions.update(name.decode() for name in re.findall(rb'\bGLIBC_([0-9]+(?:\.[0-9]+)+)\b', raw))
        else:
            status, raw = run(['/usr/bin/otool', '-L', binary], env, capture=True)
            require(status == 0, 'runtime_dependency_audit_failed')
            for line in raw.decode().splitlines()[1:]:
                name = line.strip().split(' (', 1)[0].rsplit('/', 1)[-1]
                require(re.fullmatch('[A-Za-z0-9_.+-]+', name), 'runtime_dependency_name_invalid')
                libraries.add(name)
    smoke = ('import ssl,sqlite3,ctypes,socket,subprocess;'
             'ctypes.CDLL(None);socket.socket().close();'
             'subprocess.check_call(["/usr/bin/true"])')
    status, _ = run([interpreter, '-I', '-B', '-c', smoke], env)
    require(status == 0, 'runtime_stdlib_import_failed')
    return {'files': len(files), 'bytes': total_bytes(root),
            'symlinks': sum(p.is_symlink() for p in root.rglob('*')),
            'native_objects_audited': len(binaries), 'needed_library_basenames': sorted(libraries),
            'max_glibc': max(versions, key=lambda v: tuple(map(int, v.split('.')))) if versions else None,
            'base_prefix_inside_slot': True, 'stdlib_import_smoke': True,
            'layout': sorted(p.name for p in root.iterdir())}


def make_launchers(slot, interpreter):
    from marmish_release import launcher
    directory = slot / 'scratch-bin'
    directory.mkdir(mode=0o700)
    for name, function in [('marmish', 'client_main'), ('marmishd', 'server_main')]:
        original = launcher(function)
        require(original.count('exec python3 ') == 1, 'launcher_template_changed')
        managed = original.replace('exec python3 ', 'exec ' + shlex.quote(str(interpreter)) + ' ', 1)
        require(managed.replace('exec ' + shlex.quote(str(interpreter)) + ' ', 'exec python3 ', 1)
                == original, 'launcher_semantics_changed')
        path = directory / name
        path.write_text(managed)
        path.chmod(0o700)
    return directory


def home_snapshot(home):
    """Record lstat metadata and symlink targets privately; never read user file contents."""
    result = {}
    for relative in ('.cache/uv', '.local/share/uv', '.config/uv', '.local/bin'):
        root = home / relative
        paths = [root]
        if root.is_dir() and not root.is_symlink():
            paths.extend(sorted(root.rglob('*')))
        for path in paths:
            try:
                info = path.lstat()
            except FileNotFoundError:
                result[str(path)] = None
                continue
            value = (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_size,
                     info.st_mtime_ns, info.st_ctime_ns)
            if stat.S_ISLNK(info.st_mode):
                value += (os.readlink(path),)
            result[str(path)] = value
    return result


def argv_acceptance(root, interpreter, env, cwd=None):
    fixture = root / 'argv-fixture'
    (fixture / 'tools').mkdir(parents=True)
    (fixture / 'tools/marmish_app.py').write_text(
        'import json,sys\ndef guarded_main(fn): return fn()\n'
        'def client_main(): print(json.dumps(sys.argv[1:]));return 0\nserver_main=client_main\n')
    launchers = make_launchers(fixture, interpreter)
    arguments = ['space here', "quote'\"", '$HOME;$(false)', 'line\nbreak', '**/*', '', 'snowman☃']
    status, raw = run([launchers / 'marmish', *arguments], env, capture=True, cwd=cwd)
    require(status == 0 and json.loads(raw) == arguments, 'launcher_argv_mismatch')


def expect_refusal(category, operation):
    try:
        operation()
    except Refused as error:
        require(str(error) == category, 'negative_wrong_refusal')
        return
    raise Refused('negative_accepted')


def require_uv_negative_signature(name, raw, row, root):
    if name == 'bad-digest':
        valid = (b'Caused by: Hash mismatch for `' in raw and b'Expected:' in raw
                 and b'0' * 64 in raw and b'Computed:' in raw
                 and row['python']['sha256'].encode('ascii') in raw)
    else:
        valid = (b'failed to query metadata of file `' in raw
                 and str(root / 'absent.tar.gz').encode('utf-8') in raw
                 and b'No such file or directory (os error 2)' in raw)
    require(valid, 'uv_negative_wrong_failure')


def uv_negative_acceptance(uv, row, archive, root, env):
    for name, replacement in [('bad-digest', '0' * 64), ('missing-local', None)]:
        slot = root / name
        slot.mkdir(mode=0o700)
        metadata = root / (name + '.json')
        local_metadata(row, archive, metadata)
        data = json.loads(metadata.read_text())
        entry = data[row['metadata_key']]
        if replacement is None:
            entry['url'] = (root / 'absent.tar.gz').as_uri()
        else:
            entry['sha256'] = replacement
        metadata.write_text(json.dumps(data))
        status, raw = run(uv_command(uv, row, slot, metadata), env, monitor=Path(env['HOME']).parent, capture=True)
        require_uv_negative_signature(name, raw, row, root)
        require(status != 0 and not list(slot.glob('python/*/bin/python3.13')),
                'uv_negative_accepted')
        shutil.rmtree(slot)
    slot = root / 'preexisting'
    (slot / 'python').mkdir(parents=True)
    expect_refusal('runtime_slot_exists', lambda: install_python(uv, row, archive, slot,
                                                               root / 'never.json', env))
    shutil.rmtree(slot)
    missing = copy.deepcopy(row)
    missing['python_entry']['sha256'] = None
    expect_refusal('metadata_hash_required', lambda: local_metadata(missing, archive, root / 'missing.json'))


def network_prefix(interpreter, env):
    if platform.system() != 'Linux':
        return [], 'uv_offline_only_not_independently_enforced'
    root = Path(env['HOME']).parent
    uid, gid = os.geteuid(), os.getegid()
    prefix = [str(bwrap_executable()), '--die-with-parent', '--unshare-user', '--uid', str(uid),
              '--gid', str(gid), '--unshare-net', '--tmpfs', '/']
    for directory in ('/usr', '/bin', '/lib', '/lib64'):
        if Path(directory).exists():
            prefix.extend(['--ro-bind', directory, directory])
    prefix.extend(['--dir', '/tmp', '--ro-bind', str(root), str(root),
                   '--proc', '/proc', '--dev', '/dev', '--'])
    outside = os.readlink('/proc/self/ns/net')
    code = ('import os,socket,sys;assert os.readlink("/proc/self/ns/net")!=sys.argv[1];'
            'assert os.geteuid()==int(sys.argv[2]) and os.getegid()==int(sys.argv[3]);'
            'assert os.geteuid()!=0 and os.stat("/").st_uid in (0,os.geteuid());'
            's=socket.socket();s.settimeout(1);'
            '\ntry:s.connect(("1.1.1.1",443))\nexcept OSError:sys.exit(0)\nsys.exit(1)')
    try:
        status, _ = run([*prefix, interpreter, '-I', '-B', '-c', code, outside, str(uid), str(gid)], env)
    except FileNotFoundError:
        return [], 'network_namespace_unavailable'
    return (prefix, 'network_namespace_enforced') if status == 0 else ([], 'network_namespace_unavailable')


def prepare_slot(slot, fixture, target, report, env, cwd=None):
    root = slot.parent
    row = fixture['targets'][target]
    stage = root / 'downloads'
    slot.mkdir(mode=0o700)
    report['stage'] = 'python_install'
    interpreter = install_python(stage / 'uv', row, stage / 'python.tar.gz', slot,
                                 stage / (slot.name.replace(' ', '-') + '.json'), env, cwd=cwd)
    report['stage'] = 'archive_install'
    dependencies = install_payload(interpreter, root / 'verified-installer.py', stage / 'marmish.tar.gz',
                                   slot, fixture, target, env)
    app_root = slot / dependencies['root_name']
    require(app_root.name == 'marmish-' + fixture['marmish_version'] + '-' + target,
            'verified_archive_basename_changed')
    for helper in (app_root / 'libexec/marmish').iterdir():
        if helper.is_file() and not helper.name.endswith('.json'):
            verify_architecture(helper, target)
    make_launchers(app_root, interpreter)
    return (slot, interpreter, app_root), dependencies


@contextlib.contextmanager
def prepare_slots(root, fixture, target, report, env):
    first, _ = prepare_slot(root / 'version one', fixture, target, report, env)
    with hostile_ambient(root, env) as cwd:
        second, dependencies = prepare_slot(root / 'version-two', fixture, target, report, env, cwd)
        # Both ran the same exact identity checks; compare their native executable too.
        require(digest(first[1]) == digest(second[1]), 'clean_hostile_interpreter_identity_mismatch')
        yield [first, second], dependencies, cwd
    report['assertions'].append('actual_hostile_ambient_and_cwd_isolation_and_restoration')
    report['unmet_acceptance'].remove('complete_hostile_ambient_and_cwd_combinations')


def native_probe(app_root, env, index, cwd=None):
    result = {'slot': 'slot_' + str(index), 'exit_code': None, 'signal': None,
              'marker_exact': False, 'output_bytes': 0}
    try:
        status, raw = run([app_root / 'libexec/marmish/wnd', '--help'], env, capture=True, cwd=cwd)
        result.update(exit_code=status if status >= 0 else None,
                      signal=-status if status < 0 else None,
                      marker_exact=raw == b'marmish_restricted_start_failed\n', output_bytes=len(raw))
    except FileNotFoundError:
        result['category'] = 'missing_tool'
    except OSError:
        result['category'] = 'spawn_failed'
    except Refused as error:
        require(str(error) in ('child_timeout', 'child_output_limit'), 'native_probe_internal_failure')
        result['category'] = str(error)
    return result


def verify_native_signatures(app_root, env):
    if platform.system() != 'Darwin':
        return
    for helper in (app_root / 'libexec/marmish').iterdir():
        if helper.is_file() and not helper.name.endswith('.json'):
            status, _ = run(['/usr/bin/codesign', '--verify', '--strict', helper], env)
            require(status == 0, 'native_signature_changed')


def native_manifest_contract(app_root):
    if platform.system() != 'Darwin':
        return True
    try:
        manifest = json.loads((app_root / 'libexec/marmish/wnd.manifest.json').read_bytes())
        return (isinstance(manifest, dict)
                and manifest.get('patch_sha256') == digest(app_root / 'patches/mdk/marmish-restricted.patch')
                and type(manifest.get('restricted_protocol')) is int and manifest['restricted_protocol'] == 1)
    except (OSError, ValueError, TypeError):
        return False


def accept_native_results(native_results, manifest_contracts, report):
    require(all(manifest_contracts), 'native_manifest_contract_mismatch')
    darwin = platform.system() == 'Darwin'
    require(all(row['exit_code'] == (1 if darwin else 0) and row['signal'] is None
                and (row['marker_exact'] if darwin else True) and row.get('category') is None
                for row in native_results), 'native_probe_failed')
    mode = 'restricted_startup_guard_refusal' if darwin else 'native_help_and_manifest_provenance'
    report['native_probe_mode'] = mode
    report['native_execution_probe'] = mode


def installed_acceptance(slots, root, launch_env, env, report, cwd=None):
    native_results = []
    manifest_contracts = []
    for index, (slot, _, app_root) in enumerate(slots, 1):
        for command in ('marmish', 'marmishd'):
            status, raw = run([app_root / 'scratch-bin' / command, '--help'], launch_env, capture=True, cwd=cwd)
            if status != 0:
                report['launcher_failure'] = {'command': command, 'exit_code': status,
                                               'category': 'installed_help_failed'}
            require(status == 0, 'installed_help_failed')
        status, raw = run([app_root / 'scratch-bin/marmish', '--doctor'], launch_env, capture=True, cwd=cwd)
        require(status == 0 and json.loads(raw)['ok'], 'installed_doctor_failed')
        verify_native_signatures(app_root, env)
        manifest_contracts.append(native_manifest_contract(app_root))
        native_results.append(native_probe(app_root, launch_env, index, cwd))
    report['native_probe_results'] = native_results
    accept_native_results(native_results, manifest_contracts, report)


def compiler_lookup(rustup, env):
    # Dedicated host lookup: fixed argv and cwd, never an installed launcher.
    return _run_process([rustup, 'which', 'rustc'], env, cwd=Path('/'), timeout=10, capture=True)


def test_compiler_directory():
    """Resolve existing compiler once; rustup lookup alone may read its host config."""
    compiler = shutil.which('rustc')
    require(compiler is not None, 'test_compiler_missing')
    selected = Path(compiler).resolve(strict=True)
    rustup = shutil.which('rustup')
    if rustup is not None and selected.name == 'rustup':
        # Never run a rustup shim with the disposable HOME: it has no toolchain.
        lookup_env = {'HOME': str(Path.home()), 'PATH': '/usr/bin:/bin', 'LANG': 'C', 'LC_ALL': 'C'}
        for name in ('RUSTUP_HOME', 'CARGO_HOME'):
            if name in os.environ:
                lookup_env[name] = os.environ[name]
        status, raw = compiler_lookup(Path(rustup).resolve(strict=True), lookup_env)
        require(status == 0, 'test_compiler_resolution_failed')
        try:
            selected = Path(raw.decode('utf-8').strip())
            require(selected.is_absolute(), 'test_compiler_resolution_failed')
            selected = selected.resolve(strict=True)
        except (UnicodeError, ValueError, OSError):
            raise Refused('test_compiler_resolution_failed') from None
    require(selected.is_file() and selected.name == 'rustc' and os.access(selected, os.X_OK),
            'test_compiler_invalid')
    return selected.parent


def namespace_probe(command, env):
    result = {'exit_code': None, 'signal': None, 'denied_operation_hint': None}
    try:
        status, raw = run(command, env, timeout=10, capture=True)
        result.update(exit_code=status if status >= 0 else None, signal=-status if status < 0 else None)
        if b'Operation not permitted' in raw:
            result['denied_operation_hint'] = 'operation_not_permitted'
        elif b'Permission denied' in raw:
            result['denied_operation_hint'] = 'permission_denied'
    except FileNotFoundError:
        result['category'] = 'missing_tool'
    except OSError:
        result['category'] = 'spawn_failed'
    except Refused as error:
        require(str(error) in ('child_timeout', 'child_output_limit'), 'namespace_probe_internal_failure')
        result['category'] = str(error)
    return result


def linux_namespace_preflight(env, report):
    if platform.system() != 'Linux':
        return
    policies = {}
    for name, path in (('apparmor_restrict_unprivileged_userns', '/proc/sys/kernel/apparmor_restrict_unprivileged_userns'),
                       ('unprivileged_userns_clone', '/proc/sys/kernel/unprivileged_userns_clone'),
                       ('max_user_namespaces', '/proc/sys/user/max_user_namespaces')):
        try:
            with open(path, 'rb') as source:
                raw = source.read(33)
            valid = re.fullmatch(rb'[0-9]{1,20}\s?', raw)
            policies[name] = {'value': int(raw) if valid else None, 'category': None if valid else 'invalid_value'}
        except OSError:
            policies[name] = {'value': None, 'category': 'unavailable'}
    try:
        executable = str(bwrap_executable())
    except Refused as error:
        report['linux_namespace_preflight'] = {'selection_failure': str(error)}
        raise
    bwrap = [executable, '--unshare-user', '--unshare-pid', '--uid', '0', '--gid', '0',
             '--cap-drop', 'ALL', '--die-with-parent', '--new-session', '--clearenv',
             '--setenv', 'PATH', '/usr/bin:/usr/sbin:/bin', '--setenv', 'HOME', '/tmp',
             '--setenv', 'LANG', 'C.UTF-8', '--ro-bind', '/', '/', '--proc', '/proc',
             '--dev', '/dev', '--', '/usr/bin/true']
    outer = ['/usr/bin/unshare', '--user', '--map-root-user', '--net']
    preparation = ('/usr/sbin/ip link set lo up && /usr/sbin/sysctl -q -w '
                   'net.ipv4.ip_unprivileged_port_start=0 && exec "$@"')
    report['linux_namespace_preflight'] = {
        'policy': policies,
        'tools_present': {name: shutil.which(name, path=env['PATH']) is not None for name in ('ip', 'sysctl')},
        'bare_unshare': namespace_probe([*outer, '/usr/bin/true'], env),
        'bwrap': namespace_probe(bwrap, env),
        'unshare_preparation_bwrap': namespace_probe([*outer, '/bin/sh', '-c', preparation, 'marmish-probe', *bwrap], env)}


def full_tools_acceptance(interpreter, env, root, report, failure_log=None):
    report['stage'] = 'full_tools'
    compiler = test_compiler_directory()
    test_env = dict(env, PATH=str(interpreter.parent) + ':' + str(compiler) + ':/usr/bin:/bin:/usr/sbin:/sbin')
    linux_namespace_preflight(test_env, report)
    test_env = test_supervisor(test_env)
    status, raw = run([interpreter, '-I', '-B', '-m', 'unittest', 'discover', '-s', REPO / 'tools', '-p', 'test_*.py', '-q'],
                      test_env, timeout=900, capture=True)
    count = re.search(rb'Ran ([0-9]+) tests?', raw)
    skipped = re.search(rb'skipped=([0-9]+)', raw)
    report['full_tools_tests'] = {'top_level_and_path_children': 'managed_interpreter', 'compiler': 'existing_host_toolchain_test_only', 'exit_code': status, 'count': int(count[1]) if count else None, 'skipped': int(skipped[1]) if skipped else 0}
    if status != 0:
        # Only source-defined unittest identifiers may enter the uploaded report.
        failures = re.findall(rb'^(?:FAIL|ERROR): (test_[A-Za-z0-9_]+|setUpClass|tearDownClass) \((test_[A-Za-z0-9_.]+)\)(?: \([^\r\n]*\))?$', raw, re.MULTILINE)
        report['full_tools_tests']['failed_tests'] = [
            {'method': method.decode('ascii'), 'class': owner.decode('ascii')}
            for method, owner in failures[:64]]
        if failure_log is not None:
            fd = os.open(failure_log, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            with os.fdopen(fd, 'wb') as output:
                output.write(raw)
    require(status == 0, 'full_tools_suite_failed')
    report['assertions'].append('full_tools_suite_under_managed_python')


def offline_acceptance(slots, stage, env, launch_env, touched, report):
    first, interpreter, _ = slots[0]
    report['stage'] = 'offline_retention'
    prefix, denial = network_prefix(interpreter, env)
    report['offline_network'] = denial
    shutil.rmtree(stage)
    for key in ('XDG_CACHE_HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME'):
        shutil.rmtree(env[key])
    for slot, _, app_root in slots:
        status, _ = run([*prefix, app_root / 'scratch-bin/marmish', '--help'], launch_env)
        require(status == 0, 'offline_retained_launcher_failed')
    shutil.rmtree(first)
    status, _ = run([*prefix, slots[1][2] / 'scratch-bin/marmish', '--help'], launch_env)
    require(status == 0, 'remaining_retained_launcher_failed')
    require(not touched.exists(), 'path_python_was_used')
    require(denial != 'network_namespace_unavailable', 'linux_independent_offline_denial_unavailable')
    report['assertions'].append('offline_retention_after_uv_download_cache_removal_and_version_deletion')


def require_uv_hash_mismatch(raw, expected, computed):
    expected_match = re.search(rb'Expected:\s+([0-9a-f]{64})', raw)
    computed_match = re.search(rb'Computed:\s+([0-9a-f]{64})', raw)
    require(b'Caused by: Hash mismatch for `' in raw
            and expected_match is not None and expected_match[1].decode('ascii') == expected
            and computed_match is not None and computed_match[1].decode('ascii') == computed,
            'uv_negative_wrong_failure')


def substituted_gzip(original, destination):
    shutil.copyfile(original, destination)
    require(destination.stat().st_ino != original.stat().st_ino, 'substitute_inode_unchanged')
    with destination.open('r+b') as output:
        header = output.read(10)
        require(len(header) == 10 and header[:3] == b'\x1f\x8b\x08'
                and not header[3] & 2, 'substitute_invalid_gzip')
        output.seek(4)
        output.write(bytes([header[4] ^ 1]) + header[5:8])
    with gzip.open(original, 'rb') as first, gzip.open(destination, 'rb') as second:
        total = 0
        while True:
            left, right = first.read(1 << 20), second.read(1 << 20)
            require(left == right, 'substitute_decompressed_mismatch')
            total += len(left)
            require(total <= 512 << 20, 'substitute_decompressed_limit')
            if not left:
                break
    destination.chmod(0o400)
    require(destination.stat().st_uid == os.geteuid() and not destination.is_symlink(),
            'substitute_owner_invalid')
    return digest(destination)


def require_no_exposure(slot, staging=None, scope=None):
    base = slot / 'python'
    require(not any(base.glob('cpython-*')), 'negative_runtime_exposed')
    for path in (scope or slot).rglob('*'):
        if (path.is_file() or path.is_symlink()) and re.fullmatch(r'python(?:[0-9]+(?:\.[0-9]+)*)?', path.name):
            require(staging is not None and path.is_relative_to(staging), 'negative_interpreter_exposed')
        require(path.name not in ('scratch-bin', 'bin-launchers'), 'negative_launcher_exposed')


def substitution_acceptance(uv, row, archive, root, report):
    case = root / 'substitution-case'
    case.mkdir(mode=0o700)
    env = environment(case)
    slot = case / 'slot'
    slot.mkdir()
    original = case / 'python.tar.gz'
    shutil.copyfile(archive, original)
    replacement = case / 'replacement.tar.gz'
    computed = substituted_gzip(original, replacement)
    calls = []
    def replace_after_rehash():
        calls.append(True)
        os.replace(replacement, original)
    try:
        install_python(uv, row, original, slot, case / 'metadata.json', env,
                       after_rehash=replace_after_rehash)
    except UvInstallFailure as error:
        require_uv_hash_mismatch(error.output, row['python']['sha256'], computed)
    else:
        raise Refused('negative_accepted')
    require(len(calls) == 1, 'substitution_callback_not_called_once')
    require_no_exposure(slot, scope=case)
    shutil.rmtree(case)
    report['assertions'].append('actual_uv_rejects_archive_substituted_after_final_rehash')
    report['unmet_acceptance'].remove('substituted_archive_after_rehash')


class ExtractionObserver:
    def __init__(self, slot, temporary):
        self.slot = slot
        self.temporary = temporary
        self.observed = False
        self.staging = None

    def __call__(self, path, info):
        if path.suffix not in ('.py', '.so', '.dylib'):
            return
        base = self.slot / 'python'
        for parent in (self.temporary, base):
            if path.is_relative_to(parent):
                relative = path.relative_to(parent)
                if len(relative.parts) > 1 and not relative.parts[0].startswith('cpython-'):
                    self.staging = parent / relative.parts[0]
                    self.observed = True
                    return


def require_fresh_isolation(fresh, env, previous):
    require(not previous.exists() and not previous.is_symlink(), 'interruption_debris_visible')
    root = fresh.parent.resolve(strict=True)
    require(not fresh.is_symlink(), 'retry_slot_escape')
    for name in ('HOME', 'TMPDIR', 'XDG_CACHE_HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME'):
        require(Path(env[name]).resolve(strict=True).is_relative_to(root), 'retry_environment_escape')
    for path in fresh.rglob('*'):
        try:
            resolved = path.resolve(strict=True)
        except OSError:
            raise Refused('retry_slot_escape') from None
        require(resolved.is_relative_to(fresh), 'retry_slot_escape')


def interruption_acceptance(uv, row, archive, root, report):
    case = root / 'interruption-case'
    case.mkdir(mode=0o700)
    env = environment(case)
    slot = case / 'slot'
    slot.mkdir()
    metadata = case / 'metadata.json'
    local_metadata(row, archive, metadata)
    observer = ExtractionObserver(slot, Path(env['TMPDIR']))
    status, _ = run(uv_command(uv, row, slot, metadata), env, monitor=case, observer=observer)
    require(status == -signal.SIGKILL and observer.observed and observer.staging is not None,
            'interruption_not_observed')
    require_no_exposure(slot, observer.staging, case)
    shutil.rmtree(case)
    retry = root / 'interruption-retry'
    retry.mkdir(mode=0o700)
    retry_env = environment(retry)
    fresh = retry / 'fresh-slot'
    fresh.mkdir()
    install_python(uv, row, archive, fresh, retry / 'metadata.json', retry_env)
    require_fresh_isolation(fresh, retry_env, case)
    shutil.rmtree(retry)
    report['assertions'].append('actual_interrupted_uv_extraction_no_exposure_and_fresh_retry')
    report['unmet_acceptance'].remove('interrupted_extraction')


def hostile_configuration(root, env):
    case = root / 'hostile-case'
    cwd = case / 'unexpected cwd' / 'nested'
    cwd.mkdir(parents=True)
    for directory in (case, cwd.parent, cwd):
        (directory / 'uv.toml').write_text('invalid TOML [\n')
        (directory / 'pyproject.toml').write_text('[tool.uv]\nunsupported-value = true\n')
        (directory / '.python-version').write_text('0.0.0-do-not-install\n')
    configuration = Path(env['XDG_CONFIG_HOME']) / 'uv'
    configuration.mkdir(exist_ok=True)
    (configuration / 'uv.toml').write_text('invalid TOML [\n')
    cache, install = case / 'unused-cache', case / 'unused-install'
    cache.mkdir()
    install.mkdir()
    bad = case / 'bad-metadata.json'
    bad.write_text('{invalid')
    sentinels = {'UV_CACHE_DIR': str(cache), 'UV_PYTHON_INSTALL_DIR': str(install),
                 'UV_PYTHON_DOWNLOADS_JSON_URL': bad.as_uri(), 'PIP_CONFIG_FILE': str(bad),
                 'PYTHONHOME': str(case / 'missing-python'), 'PYTHONPATH': str(case / 'missing-modules'),
                 'LD_PRELOAD': str(case / 'missing.so'), 'DYLD_INSERT_LIBRARIES': str(case / 'missing.dylib'),
                 'MARMISH_RESTRICTED_CONFIG': str(case / 'missing-config')}
    return cwd, sentinels, (cache, install)


@contextlib.contextmanager
def hostile_ambient(root, env):
    cwd, sentinels, untouched = hostile_configuration(root, env)
    allowed = {'HOME', 'PATH', 'LANG', 'LC_ALL', 'TMPDIR', 'XDG_CACHE_HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME'}
    require(set(env) == allowed, 'child_environment_not_allowlisted')
    before = dict(os.environ)
    descriptor = os.open('.', os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.environ.update(sentinels)
        os.chdir(cwd)
        yield cwd
        require(all(not list(path.iterdir()) for path in untouched), 'hostile_sentinel_used')
    finally:
        restored = True
        try:
            for name in sentinels:
                if name in before:
                    os.environ[name] = before[name]
                else:
                    os.environ.pop(name, None)
        except Exception:
            restored = False
        try:
            os.fchdir(descriptor)
            restored = restored and os.stat('.').st_ino == os.fstat(descriptor).st_ino
        except Exception:
            restored = False
        finally:
            os.close(descriptor)
        require(restored and dict(os.environ) == before, 'ambient_restore_failed')


def proof(root, fixture, target, report, failure_log=None):
    row = fixture['targets'][target]
    env = environment(root)
    stage = root / 'downloads'
    stage.mkdir(mode=0o700)
    (Path(env['HOME']) / 'uv.toml').write_text('this is deliberately invalid TOML [\n')
    (root / 'uv.toml').write_text('this is deliberately invalid TOML [\n')
    require(not any(key.startswith(('UV_', 'PIP_', 'PYTHON', 'LD_', 'DYLD_')) for key in env),
            'environment_not_isolated')
    assets = {}
    for kind in ('uv', 'python', 'marmish'):
        record = row[kind]
        path = stage / (kind + '.tar.gz')
        download(record['url'], path, CAPS[kind], env)
        verify_asset(path, record, kind, env)
        assets[kind] = path
    report['assertions'].append('all_downloads_size_and_system_sha256_verified')
    report['stage'] = 'uv_extract'
    uv = stage / 'uv'
    extract_uv(assets['uv'], row['uv_member'], uv)
    verify_architecture(uv, target)
    report['stage'] = 'uv_version'
    status, raw = run([uv, '--no-config', '--version'], env, capture=True)
    require(status == 0 and raw.decode().split()[:2] == ['uv', fixture['uv_version']], 'uv_version_mismatch')
    report['stage'] = 'uv_negative'
    uv_negative_acceptance(uv, row, assets['python'], root, env)
    report['assertions'].append('uv_rejects_bad_digest_missing_archive_and_existing_slot')
    report['stage'] = 'interrupted_extraction'
    interruption_acceptance(uv, row, assets['python'], root, report)
    report['stage'] = 'substituted_archive'
    substitution_acceptance(uv, row, assets['python'], root, report)
    verifier = root / 'verified-installer.py'
    report['installer_python_sha256'] = embedded_verifier(verifier)
    with prepare_slots(root, fixture, target, report, env) as (slots, dependencies, cwd):
        report['native_prerequisites'] = dependencies['prerequisites']
        report['assertions'].extend(['two_absolute_base_interpreters_and_exact_archive_verifier',
                                     'invalid_cwd_uv_configuration_ignored_and_environment_allowlisted'])
        report['base_prefixes'] = ['<scratch>/' + slot.name + '/python/<discovered-managed-install>'
                                   for slot, _, _ in slots]
        report['stage'] = 'launcher_acceptance'
        first, interpreter, _ = slots[0]
        report['runtime_audit'] = audit_runtime(first, interpreter, env)
        argv_acceptance(root, interpreter, env, cwd)
        # Hostile ambient state cannot affect the allowlisted env or --no-config uv.
        sentinel = root / 'sentinel'
        sentinel.mkdir()
        touched = root / 'python-sentinel-used'
        fake = sentinel / 'python3'
        fake.write_text('#!/bin/sh\ntouch ' + shlex.quote(str(touched)) + '\nexit 93\n')
        fake.chmod(0o700)
        launch_env = dict(env, PATH=str(sentinel) + ':/usr/bin:/bin')
        installed_acceptance(slots, root, launch_env, env, report, cwd)
        require(not touched.exists(), 'path_python_was_used')
        report['assertions'].append('installed_help_doctor_restricted_guard_and_literal_argv'
                                    if platform.system() == 'Darwin' else
                                    'installed_help_doctor_native_help_and_literal_argv')
        report['native_version_flag_supported'] = False
        report['system_python_dependency'] = ('linux_doctor_and_sandbox_still_require_usr_bin_python3'
                                              if platform.system() == 'Linux' else 'no_linux_dependency_claim')
    full_tools_acceptance(slots[1][1], env, root, report, failure_log)
    offline_acceptance(slots, stage, env, launch_env, touched, report)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--total-timeout', type=int, default=1320)
    parser.add_argument('--ci-context')
    parser.add_argument('--require-bwrap-helper', action='store_true')
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--failure-log', type=Path, help='Private local diagnostic only; never upload this file')
    options = parser.parse_args(argv)
    options.report = options.report.absolute()
    if options.failure_log is not None:
        options.failure_log = options.failure_log.absolute()
    report = {'schema': 1, 'ok': False, 'assertions': [], 'failure': None,
              'cleanup': 'not_started', 'home_unchanged': False,
              'unmet_acceptance': ['interrupted_extraction', 'substituted_archive_after_rehash',
                                   'complete_hostile_ambient_and_cwd_combinations']}
    before = None
    owned_root = None
    handler_state = None
    try:
        handler_state = install_interrupt_handlers(options.total_timeout)
        report['total_timeout_seconds'] = options.total_timeout
        require(os.geteuid() != 0, 'root_refused')
        target = TARGETS.get((platform.system(), platform.machine()))
        require(target is not None, 'unsupported_target')
        if options.ci_context is not None:
            report['ci'] = ci_context(options.ci_context, target)
        initialize_helper(options.require_bwrap_helper, report)
        report['host'] = host_facts()
        fixture = json.loads(FIXTURE.read_bytes())
        report.update(target=target, uv_version=fixture['uv_version'], python_version=fixture['python_version'],
                      marmish_version=fixture['marmish_version'], assets=fixture['targets'][target],
                      metadata_source=fixture['metadata_source'], checksum_sources=fixture['checksum_sources'])
        before = home_snapshot(Path.home())
        temporary_root = Path('/private/tmp' if platform.system() == 'Darwin' else '/tmp').resolve(strict=True)
        with tempfile.TemporaryDirectory(prefix='marmish-managed-', dir=temporary_root) as temporary:
            root = Path(temporary)
            owned_root = root
            require(root.stat().st_uid == os.geteuid() and stat.S_IMODE(root.stat().st_mode) == 0o700
                    and root.name.isascii() and not root.is_symlink(), 'unsafe_scratch')
            report['cleanup'] = 'pending'
            proof(root, fixture, target, report, options.failure_log)
            report['ok'] = True
    except Refused as error:
        report['failure'] = str(error)
    except KeyboardInterrupt:
        report['failure'] = 'interrupted'
    except Exception as error:
        report['failure'] = 'unexpected_local_failure'
        report['error_type'] = type(error).__name__
        report['errno'] = error.errno if isinstance(error, OSError) else None
        report['error_frames'] = [{'file': Path(frame.filename).name, 'line': frame.lineno,
                                   'function': frame.name} for frame in traceback.extract_tb(error.__traceback__)[-8:]]
    finally:
        if handler_state is not None:
            restore_interrupt_handlers(handler_state)
        if owned_root is not None:
            report['cleanup'] = 'incomplete' if owned_root.exists() else 'complete'
            if report['cleanup'] != 'complete':
                report['ok'] = False
                report['failure'] = 'cleanup_incomplete'
        if before is not None:
            try:
                report['home_unchanged'] = before == home_snapshot(Path.home())
            except OSError:
                report['home_unchanged'] = False
            if not report['home_unchanged']:
                report['ok'] = False
                report['prior_failure'] = report['failure']
                report['failure'] = 'real_uv_home_changed'
        fd = os.open(options.report, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'w') as output:
            json.dump(report, output, indent=2, sort_keys=True)
            output.write('\n')
    return 0 if report['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
