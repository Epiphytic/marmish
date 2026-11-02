"""Private, bounded process/state boundary for installed interoperability CI.

This module never implements application operations: identities use installed
launchers, and cleanup refuses uncertain ownership. Public reports are emitted
by the orchestrator after this module has finished deleting private state.
"""
import contextlib
import errno
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

from marmish_platform import canonical_state_path, continuous_time, process_identity
from marmish_protocol import load_json, save_json
from marmish_rpc import RpcError, strict_json
from mdk_smoke import Processes, SmokeError, require
from marmish_transport_sandbox import SandboxError

ROLES = ('linux-x64', 'linux-arm64', 'macos-arm64')
RELAYS = ('wss://relay.eu.whitenoise.chat/', 'wss://relay.us.whitenoise.chat/')
CHECKS = ('frontend_absent', 'mdk_absent', 'workers_absent', 'caffeinate_absent',
          'engine_cleanup_ready', 'uninstall_complete', 'root_removed')
_HEX = re.compile('[0-9a-f]{64}')
_MAX_PROCESSES = 4096
_PRIVATE_LIMIT = 1024 * 1024
_DIGEST_CACHE = {}
DIAGNOSTIC_REASONS = frozenset({
    'unsafe_state_path', 'unsafe_state_file', 'state_exists', 'invalid_state',
    'state_identity_changed', 'process_identity_changed', 'invalid_process',
    'invalid_executable', 'invalid_identity', 'identity_exists', 'identity_duplicate',
    'identity_init_failed', 'interpreter_failed', 'operation_timeout',
    'process_scan_preflight_failed', 'caffeinate_failed', 'clock_invalid',
    'cleanup_incomplete', 'uninstall_failed', 'install_verify_failed',
    'installer_refused', 'installer_timeout', 'installer_output_limit', 'artifact_invalid',
})
DIAGNOSTIC_STAGES = frozenset({
    'pin', 'install', 'verify', 'identity_server', 'identity_client', 'barrier',
    'cleanup_load', 'cleanup_validate', 'cleanup_caffeinate', 'cleanup_native',
    'cleanup_frontends', 'cleanup_runtime', 'cleanup_uninstall', 'cleanup_final', 'cleanup_remove',
})


def failure_diagnostic(error, *, stage, category):
    """Emit fixed diagnostic vocabulary, never exception messages or paths."""
    types = {'AttributeError', 'ChildProcessError', 'PermissionError', 'FileNotFoundError',
             'OSError', 'RuntimeError', 'ValueError', 'TypeError', 'KeyError', 'SmokeError',
             'SandboxError', 'KeyboardInterrupt'}
    reason = error.args[0] if isinstance(error, SmokeError) and len(error.args) == 1 else None
    name = type(error).__name__
    code = getattr(error, 'errno', None)
    value = {'kind': 'installed-failure',
          'stage': stage if type(stage) is str and stage in DIAGNOSTIC_STAGES else 'other',
          'category': category if type(category) is str and category in DIAGNOSTIC_REASONS | {
              'internal_failure', 'barrier_timeout', 'artifact_network_failed',
              'artifact_listing_unstable'} else 'internal_failure',
          'reason': reason if type(reason) is str and reason in DIAGNOSTIC_REASONS else 'other',
          'error_type': name if name in types | {'IdentityError'} else 'other',
          'errno': code if type(code) is int and 0 <= code <= 65535 else None}
    if type(error) is IdentityError:
        from marmish_release_acceptance import DIAGNOSTIC_ERRORS
        facts = {}
        for key, allowed in (
                ('reason', IDENTITY_FAILURE_REASONS),
                ('cleanup', frozenset({'complete', 'incomplete'})),
                ('frontend_category', DIAGNOSTIC_ERRORS | {'none'})):
            item = getattr(error, 'identity_' + key, None)
            facts[key] = item if type(item) is str and item in allowed else 'other'
        returned = getattr(error, 'returncode', None)
        facts['returncode'] = returned if type(returned) is int and -255 <= returned <= 255 else None
        facts['signal'] = -returned if type(returned) is int and -64 <= returned < 0 else None
        for key in ('timed_out', 'output_limited'):
            item = getattr(error, 'identity_' + key, None)
            facts[key] = item if type(item) is bool else None
        value['identity'] = facts
    print(json.dumps(value, sort_keys=True), file=sys.stderr)


def _canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)
            + '\n').encode()


def _directory_fd(path, *, private=False):
    path = canonical_state_path(path)
    fd = os.open('/', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        for component in path.parts[1:]:
            child = os.open(component, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
                            | os.O_CLOEXEC, dir_fd=fd)
            os.close(fd)
            fd = child
            info = os.fstat(fd)
            require(info.st_uid in (0, os.geteuid()) and
                    (not info.st_mode & 0o022 or
                     info.st_uid == 0 and info.st_mode & stat.S_ISVTX), 'unsafe_state_path')
        if private:
            info = os.fstat(fd)
            require(info.st_uid == os.geteuid() and stat.S_IMODE(info.st_mode) == 0o700,
                    'unsafe_state_path')
        return fd
    except BaseException:
        os.close(fd)
        raise


def _file_bytes(path, limit=_PRIVATE_LIMIT, *, private=True):
    parent = _directory_fd(Path(path).parent)
    try:
        fd = os.open(Path(path).name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK
                     | os.O_CLOEXEC, dir_fd=parent)
    finally:
        os.close(parent)
    with os.fdopen(fd, 'rb') as source:
        info = os.fstat(source.fileno())
        require(stat.S_ISREG(info.st_mode) and info.st_nlink == 1 and
                info.st_uid in ((os.geteuid(),) if private else (0, os.geteuid()))
                and (stat.S_IMODE(info.st_mode) == 0o600 if private else
                     not info.st_mode & 0o022) and info.st_size <= limit, 'unsafe_state_file')
        data = source.read(limit + 1)
        require(len(data) <= limit, 'unsafe_state_file')
        return data


def _new_json(path, value):
    path = Path(path)
    parent = _directory_fd(path.parent)
    temporary = '.marmish-state-' + secrets.token_hex(8)
    try:
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
                     | os.O_CLOEXEC, 0o600, dir_fd=parent)
        with os.fdopen(fd, 'wb') as output:
            output.write(_canonical(value))
            output.flush()
            os.fsync(output.fileno())
        os.link(temporary, path.name, src_dir_fd=parent, dst_dir_fd=parent,
                follow_symlinks=False)
        os.unlink(temporary, dir_fd=parent)
        os.fsync(parent)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary, dir_fd=parent)
        os.close(parent)


def _root_identity(info):
    return {'dev': info.st_dev, 'ino': info.st_ino, 'uid': info.st_uid}


def write_private(path, value):
    """Create one immutable, bounded private JSON record without replacing it."""
    require(len(_canonical(value)) <= _PRIVATE_LIMIT, 'unsafe_state_file')
    _new_json(path, value)


def read_private(path, limit=_PRIVATE_LIMIT):
    require(type(limit) is int and 0 < limit <= _PRIVATE_LIMIT, 'unsafe_state_file')
    return _file_bytes(path, limit)


def _validate_state(state):
    root = Path(state['root'])
    require(root == canonical_state_path(root) and
            re.fullmatch(r'mci-[A-Za-z0-9_-]{6,16}', root.name) is not None and
            root.parent == canonical_state_path('/tmp'), 'unsafe_state_path')
    fd = _directory_fd(root, private=True)
    try:
        require(_root_identity(os.fstat(fd)) == state['root_identity'], 'state_identity_changed')
    finally:
        os.close(fd)
    expected = {
        'prefix': root / 'install', 'home': root / 'home', 'tmp': root / 'tmp',
        'registry': root / 'registry', 'installer': root / 'install.sh',
    }
    require(all(state[key] == str(path) for key, path in expected.items()), 'invalid_state')
    if 'output_identity' in state:
        output = Path(state['state_file']).with_name(Path(state['state_file']).name + '.public')
        require(state['output'] == str(output), 'invalid_state')
        descriptor = _directory_fd(output, private=True)
        try:
            require(_root_identity(os.fstat(descriptor)) == state['output_identity'],
                    'state_identity_changed')
        finally:
            os.close(descriptor)
    return state


def clock_sample():
    return {'continuous_ns': round(continuous_time() * 1_000_000_000),
            'monotonic_ns': time.monotonic_ns(), 'wall_ns': time.time_ns()}


def elapsed(start, end):
    require(set(start) == set(end) == {'continuous_ns', 'monotonic_ns', 'wall_ns'}
            and all(type(value) is int and value >= 0 for value in (*start.values(), *end.values())),
            'clock_invalid')
    total = end['continuous_ns'] - start['continuous_ns']
    active = end['monotonic_ns'] - start['monotonic_ns']
    return {'elapsed_ns': max(0, total), 'active_ns': max(0, active),
            'suspended_ns': max(0, total - active),
            'clock_valid': total >= 0 and active >= 0 and abs(total - active) <= 5_000_000_000}


def public_directory(state_file):
    """Owned report destination survives private state deletion and setup errors."""
    state_file = canonical_state_path(state_file)
    parent = _directory_fd(state_file.parent)
    os.close(parent)
    output = state_file.with_name(state_file.name + '.public')
    with contextlib.suppress(FileExistsError):
        output.mkdir(mode=0o700)
    descriptor = _directory_fd(output, private=True)
    os.close(descriptor)
    return output


def create_state(state_file, ctx, pin_sha256, role, *, installer_bytes=None, installer_sha256=None):
    require((installer_bytes is None) == (installer_sha256 is None), 'invalid_state')
    if installer_bytes is not None:
        require(type(installer_bytes) is bytes and 0 < len(installer_bytes) <= _PRIVATE_LIMIT and
                isinstance(installer_sha256, str) and _HEX.fullmatch(installer_sha256), 'invalid_state')
        require(hashlib.sha256(installer_bytes).hexdigest() == installer_sha256, 'install_verify_failed')
    state_file = canonical_state_path(state_file)
    require(set(ctx) == {'run', 'attempt', 'sha'} and
            all(isinstance(ctx[key], str) and re.fullmatch('[1-9][0-9]*', ctx[key])
                for key in ('run', 'attempt')) and
            isinstance(ctx['sha'], str) and re.fullmatch('[0-9a-f]{40}', ctx['sha']) and
            isinstance(pin_sha256, str) and _HEX.fullmatch(pin_sha256) and role in ROLES,
            'invalid_state')
    parent = _directory_fd(state_file.parent)
    os.close(parent)
    require(not os.path.lexists(state_file), 'state_exists')
    snapshot = clock_sample()
    unreadable = _snapshot_unreadable(snapshot['continuous_ns'] // 1000) if sys.platform.startswith('linux') else []
    output = state_file.with_name(state_file.name + '.public')
    root = canonical_state_path(tempfile.mkdtemp(prefix='mci-', dir='/tmp'))
    root.chmod(0o700)
    made_output = False
    try:
        made_output = not os.path.lexists(output)
        output = public_directory(state_file)
        for name in ('home', 'tmp', 'registry', 'home/config', 'home/data', 'home/cache', 'home/runtime'):
            (root / name).mkdir(mode=0o700)
        installer = Path(__file__).resolve().parents[1] / 'scripts/install.sh'
        raw = _file_bytes(installer, private=False) if installer_bytes is None else installer_bytes
        fd = os.open(root / 'install.sh', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        with os.fdopen(fd, 'wb') as copy:
            copy.write(raw)
            copy.flush()
            os.fsync(copy.fileno())
        copied_digest = hashlib.sha256(_file_bytes(root / 'install.sh')).hexdigest()
        require(copied_digest == hashlib.sha256(raw).hexdigest(), 'install_verify_failed')
        started = clock_sample()
        now = started['continuous_ns'] / 1_000_000_000
        state = {'format': 1, 'ctx': dict(ctx), 'pin_sha256': pin_sha256, 'role': role,
                 'root': str(root), 'root_identity': _root_identity(root.stat()),
                 'state_file': str(state_file), 'prefix': str(root / 'install'),
                 'home': str(root / 'home'), 'tmp': str(root / 'tmp'),
                 'registry': str(root / 'registry'), 'output': str(output),
                 'output_identity': _root_identity(output.stat()),
                 'installer': str(root / 'install.sh'),
                 'installer_sha256': copied_digest,
                 'repo': str(Path(__file__).resolve().parents[1]),
                 'python': str(Path(sys.executable).absolute()),
                 'execution_id': 'marmish-installed:' + secrets.token_hex(24),
                 'preexisting_unreadable': unreadable,
                 'start': started, 'preparation_deadline': now + 900,
                 'barrier_deadline': now + 1500, 'overall_deadline': now + 4500}
        _new_json(root / 'state.json', state)
        _new_json(state_file, {'format': 1, 'root': str(root),
                              'root_identity': state['root_identity'],
                              'state_sha256': hashlib.sha256(_canonical(state)).hexdigest()})
        return state
    except BaseException:
        shutil.rmtree(root)
        if made_output:
            output.rmdir()
        raise


def load_state(state_file):
    state_file = canonical_state_path(state_file)
    pointer = strict_json(_file_bytes(state_file, 8192))
    require(isinstance(pointer, dict) and set(pointer) ==
            {'format', 'root', 'root_identity', 'state_sha256'} and pointer['format'] == 1,
            'invalid_state')
    root = Path(pointer['root'])
    _validate_state({'root': str(root), 'root_identity': pointer['root_identity'],
                     **{key: str(root / value) for key, value in
                        [('prefix', 'install'), ('home', 'home'), ('tmp', 'tmp'),
                         ('registry', 'registry'), ('installer', 'install.sh')]}})
    raw = _file_bytes(root / 'state.json')
    require(hashlib.sha256(raw).hexdigest() == pointer['state_sha256'], 'state_identity_changed')
    state = strict_json(raw)
    require(isinstance(state, dict) and state['state_file'] == str(state_file)
            and state['root_identity'] == pointer['root_identity'], 'invalid_state')
    return _validate_state(state)


def child_environment(state, *, runtime):
    _validate_state(state)
    python_dir = str(Path(state['python']).parent)
    env = {'PATH': ':'.join(dict.fromkeys([python_dir, '/usr/bin', '/bin', '/usr/sbin', '/sbin'])),
           'LANG': 'C', 'HOME': state['home'], 'TMPDIR': state['tmp'],
           'XDG_CONFIG_HOME': str(Path(state['home']) / 'config'),
           'XDG_DATA_HOME': str(Path(state['home']) / 'data'),
           'XDG_CACHE_HOME': str(Path(state['home']) / 'cache'),
           'XDG_RUNTIME_DIR': str(Path(state['home']) / 'runtime'),
           'EPIPHYTIC_EXECUTION_ID': state['execution_id']}
    if 'RUNNER_TRACKING_ID' in os.environ:
        env['RUNNER_TRACKING_ID'] = os.environ['RUNNER_TRACKING_ID']
    if runtime and sys.platform.startswith('linux'):
        from marmish_ci_bwrap import environment
        env.update(environment())
    return env


def register_executable(state, path, sha256):
    """Trusted harness registration, only after independent artifact verification."""
    _validate_state(state)
    path = canonical_state_path(path)
    require(Path(state['root']) in path.parents and _HEX.fullmatch(sha256), 'invalid_executable')
    require(hashlib.sha256(_file_bytes(path, 512 * 1024 * 1024, private=False)).hexdigest()
            == sha256, 'invalid_executable')
    record = {'path': str(path), 'sha256': sha256}
    name = 'artifact-' + hashlib.sha256(os.fsencode(path)).hexdigest() + '.json'
    destination = Path(state['registry']) / name
    if destination.exists():
        require(load_json(destination) == record, 'invalid_executable')
    else:
        _new_json(destination, record)


def register_installed(state, installed_root):
    installed_root = canonical_state_path(installed_root)
    require(Path(state['prefix']) in installed_root.parents, 'invalid_executable')
    manifest = strict_json(_file_bytes(installed_root / 'release.json', 2 * 1024 * 1024,
                                       private=False))
    for name, metadata in manifest['files'].items():
        require(isinstance(name, str) and not Path(name).is_absolute() and '..' not in Path(name).parts,
                'invalid_executable')
        if metadata['mode'] & 0o111 or name.endswith('.py'):
            register_executable(state, installed_root / name, metadata['sha256'])


_exited = Processes.exited


def _confirmed_exit(child):
    """Prove the parent still owns the same unreaped exited leader."""
    require(child.returncode is None, 'process_identity_changed')
    try:
        first = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        if first is None:
            return None
        second = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
    except OSError:
        raise SmokeError('process_identity_changed') from None
    key = lambda status: (status.si_pid, status.si_code, status.si_status)
    require(second is not None and key(first) == key(second) and first.si_pid == child.pid
            and child.returncode is None and type(first.si_status) is int,
            'process_identity_changed')
    if first.si_code == os.CLD_EXITED and 0 <= first.si_status <= 255:
        return first.si_status
    require(first.si_code in (os.CLD_KILLED, os.CLD_DUMPED)
            and 1 <= first.si_status < signal.NSIG, 'process_identity_changed')
    return -first.si_status


def _signal_group(child, identity, number):
    expected = _confirmed_exit(child) if sys.platform == 'darwin' else None
    if expected is None:
        require(child.returncode is None and process_identity(child.pid) == identity and
                os.getpgid(child.pid) == child.pid, 'process_identity_changed')
    try:
        os.killpg(child.pid, number)
    except ProcessLookupError:
        pass
    except OSError as error:
        if sys.platform != 'darwin' or error.errno != errno.EPERM:
            raise
        # A live leader can exit between its identity check and killpg.
        expected = _confirmed_exit(child)
        require(expected is not None, 'cleanup_incomplete')
    return expected


def _wait_exit(child, seconds):
    deadline = continuous_time() + seconds
    while not _exited(child) and continuous_time() < deadline:
        time.sleep(.02)
    return _exited(child)


def _registry_records(state, prefix):
    entries = list(Path(state['registry']).iterdir())
    require(len(entries) <= 1024, 'cleanup_incomplete')
    return [strict_json(_file_bytes(path, 65536)) for path in entries
            if path.name.startswith(prefix) and path.name.endswith('.json')]


def _process_absent(identity):
    if sys.platform.startswith('linux'):
        path = Path('/proc') / str(identity['pid'])
        if _linux_dead(path):
            return True
        return process_identity(identity['pid']) != identity
    from marmish_platform import inspect_process
    actual = inspect_process(identity['pid'])
    return actual is None or any(actual[key] != identity[key] for key in ('pid', 'boot_id', 'start'))


def _read_proc(path):
    with path.open('rb') as source:
        data = source.read(65537)
    require(len(data) <= 65536, 'cleanup_incomplete')
    return data


def _linux_dead(path):
    try:
        raw = _read_proc(path / 'stat')
        return raw[raw.rfind(b')') + 2:].split()[0] == b'Z'
    except FileNotFoundError:
        return not path.exists()


def _snapshot_unreadable(before_start):
    entries = [path for path in Path('/proc').iterdir() if path.name.isdecimal()]
    require(len(entries) <= _MAX_PROCESSES, 'process_scan_preflight_failed')
    result = []
    for path in entries:
        try:
            if path.stat().st_uid != os.geteuid() or _linux_dead(path):
                continue
            identity = process_identity(int(path.name))
            try:
                _read_proc(path / 'environ')
            except PermissionError:
                require(process_identity(identity['pid']) == identity and
                        identity['start'] < before_start, 'process_scan_preflight_failed')
                result.append(identity)
        except (FileNotFoundError, SmokeError):
            require(_linux_dead(path), 'process_scan_preflight_failed')
    return result


def _linux_scan(state, marker=None):
    """Exact unique marker, then immutable executable/argument authentication."""
    entries = [path for path in Path('/proc').iterdir() if path.name.isdecimal()]
    require(len(entries) <= _MAX_PROCESSES, 'cleanup_incomplete')
    markers = {marker} if marker else {state['execution_id']} | {
        record['execution_id'] for record in _registry_records(state, 'child-')}
    needles = {b'EPIPHYTIC_EXECUTION_ID=' + value.encode() for value in markers}
    found = []
    for path in entries:
        try:
            if path.stat().st_uid != os.geteuid() or _linux_dead(path):
                continue
            try:
                env = _read_proc(path / 'environ')
            except PermissionError:
                identity = process_identity(int(path.name))
                require(identity in state.get('preexisting_unreadable', []), 'cleanup_incomplete')
                continue
            if not needles.intersection(env.split(b'\0')):
                continue
            identity = process_identity(int(path.name))
            argv = _read_proc(path / 'cmdline').rstrip(b'\0').split(b'\0')
            executable = Path(os.readlink(path / 'exe'))
            require(process_identity(identity['pid']) == identity, 'cleanup_incomplete')
            _verify_process(state, identity, executable, argv)
            found.append(identity)
        except (FileNotFoundError, SmokeError):
            require(_linux_dead(path), 'cleanup_incomplete')
    return found


def _verify_process(state, identity, executable, argv):
    tracked = _registry_records(state, 'child-')
    if any(record['identity'] == identity for record in tracked):
        return
    artifacts = _registry_records(state, 'artifact-')
    if _verify_transport_process(state, identity, executable, argv, artifacts):
        return
    digest = _digest_path(executable)
    if Path(state['root']) in executable.parents and any(
            record['path'] == str(executable) and record['sha256'] == digest for record in artifacts):
        return
    python = Path(state['python']).resolve()
    system_python = Path('/usr/bin/python3').resolve()
    if executable in (python, system_python):
        for record in artifacts:
            if record['path'].endswith('.py') and os.fsencode(record['path']) in argv:
                require(hashlib.sha256(_file_bytes(Path(record['path']), private=False)).hexdigest()
                        == record['sha256'], 'cleanup_incomplete')
                return
    raise SmokeError('cleanup_incomplete')


def _digest_path(path):
    info = Path(path).stat()
    key = (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)
    if key not in _DIGEST_CACHE:
        require(len(_DIGEST_CACHE) < 4096, 'cleanup_incomplete')
        _DIGEST_CACHE[key] = hashlib.sha256(_file_bytes(path, 512 * 1024 * 1024,
                                                       private=False)).hexdigest()
    return _DIGEST_CACHE[key]


def _transport_configs(state, artifacts, identity, argv):
    """Authenticate original host-side config before trusting namespace binds."""
    entries = list(Path(state['tmp']).iterdir())
    require(len(entries) <= 128, 'cleanup_incomplete')
    markers = {state['execution_id']} | {row['execution_id'] for row in
                                       _registry_records(state, 'child-')}
    scripts = [row for row in artifacts if row['path'].endswith('/marmish_transport_sandbox.py')]
    result = []
    for directory in entries:
        if not re.fullmatch(r'marmish-confinement-[A-Za-z0-9_-]+', directory.name):
            continue
        fd = _directory_fd(directory, private=True)
        os.close(fd)
        path = directory / 'config.json'
        if not path.exists():
            continue
        named = [arg for arg in argv if arg.startswith(os.fsencode(state['tmp']) + b'/')
                 and arg.endswith(b'/config.json')]
        if named and os.fsencode(path) not in named:
            continue
        if argv == [b'/usr/bin/python3', b'/sandbox.py', b'--inner', b'/config.json'] or argv[:1] == [b'/wnd']:
            if not _bound_matches(identity, 'config.json', path):
                continue
        config = strict_json(_file_bytes(path, 16384, private=False))
        require(set(config) == {'home', 'wnd', 'origins', 'relays', 'directory',
                                'execution_marker', 'bootstrap'} and
                config['directory'] == str(directory) and
                config['home'] in [str(Path(state['root']) / role / 'mdk')
                                   for role in ('server', 'client')] and
                config['relays'] == list(RELAYS) and type(config['bootstrap']) is bool and
                config['origins'] == [['127.0.0.2', 443, '0.sock'], ['127.0.0.3', 443, '1.sock']] and
                set(config['execution_marker']) == {'EPIPHYTIC_EXECUTION_ID'} and
                config['execution_marker']['EPIPHYTIC_EXECUTION_ID'] in markers,
                'cleanup_incomplete')
        matches = [row for row in artifacts if row['path'] == config['wnd']]
        require(len(matches) == 1 and _digest_path(config['wnd']) == matches[0]['sha256'],
                'cleanup_incomplete')
        for script in scripts:
            require(_digest_path(script['path']) == script['sha256'], 'cleanup_incomplete')
            result.append((path, config, Path(script['path'])))
    return result


def _same_bound_file(identity, bound, original):
    """Deliberate proc magic-link access, fenced by native identity and inode."""
    require(_bound_matches(identity, bound, original), 'cleanup_incomplete')


def _bound_matches(identity, bound, original):
    require(process_identity(identity['pid']) == identity, 'cleanup_incomplete')
    descriptor = os.open(Path('/proc') / str(identity['pid']) / 'root' / bound,
                         os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        actual, expected = os.fstat(descriptor), Path(original).stat()
        require(process_identity(identity['pid']) == identity, 'cleanup_incomplete')
        return (stat.S_ISREG(actual.st_mode) and actual.st_dev == expected.st_dev and
                actual.st_ino == expected.st_ino and actual.st_size == expected.st_size)
    finally:
        os.close(descriptor)


def _verify_transport_process(state, identity, executable, argv, artifacts, depth=0):
    require(depth <= 8, 'cleanup_incomplete')
    for config_path, config, script in _transport_configs(state, artifacts, identity, argv):
        from marmish_transport_sandbox import validated_bwrap
        bwrap = validated_bwrap()
        outer = [b'/usr/bin/python3', os.fsencode(script), b'--outer',
                 os.fsencode(config_path), os.fsencode(bwrap)]
        if executable == Path('/usr/bin/unshare').resolve() and argv == [
                b'unshare', b'--user', b'--map-root-user', b'--net', *outer]:
            return True
        if executable == Path('/usr/bin/python3').resolve() and argv == outer:
            return True
        if executable == bwrap.resolve():
            if os.fsencode(config_path) not in argv:
                continue
            # Bind paths must be exact argument pairs, never substrings.
            for source, target in ((str(config_path), '/config.json'), (str(script), '/sandbox.py'),
                                   (config['wnd'], '/wnd'), (config['home'], '/state')):
                encoded = [os.fsencode(source), os.fsencode(target)]
                require(any(argv[index:index+2] == encoded for index in range(len(argv)-1)),
                        'cleanup_incomplete')
            require(argv[-5:] == [b'--', b'/usr/bin/python3', b'/sandbox.py', b'--inner', b'/config.json'],
                    'cleanup_incomplete')
            return True
        inner = argv == [b'/usr/bin/python3', b'/sandbox.py', b'--inner', b'/config.json']
        wnd = argv[:1] == [b'/wnd']
        if inner or wnd:
            if not _bound_matches(identity, 'config.json', config_path):
                continue
            _same_bound_file(identity, 'config.json', config_path)
            _same_bound_file(identity, 'sandbox.py', script)
            if inner:
                require(executable == Path('/usr/bin/python3').resolve(), 'cleanup_incomplete')
            else:
                _same_bound_file(identity, 'wnd', config['wnd'])
                fd = os.open(Path('/proc') / str(identity['pid']) / 'exe',
                             os.O_RDONLY | os.O_CLOEXEC)
                try:
                    actual, expected = os.fstat(fd), Path(config['wnd']).stat()
                    require((actual.st_dev, actual.st_ino) == (expected.st_dev, expected.st_ino)
                            and process_identity(identity['pid']) == identity, 'cleanup_incomplete')
                finally:
                    os.close(fd)
                relays = ','.join(RELAYS).encode()
                require(argv == [b'/wnd', b'--home', b'/state', b'--socket', b'/state/s',
                    b'--logs-dir', b'/state/logs', b'--secret-store',
                    b'marmish-stdin' if config['bootstrap'] else b'file',
                    b'--discovery-relays', relays, b'--default-account-relays', relays], 'cleanup_incomplete')
            return True
        allowed = {
            '/usr/sbin/ip': [[b'/usr/sbin/ip', b'link', b'set', b'lo', b'up'],
                             [b'/usr/sbin/ip', b'link', b'set', b'lo', b'down'],
                             [b'/usr/sbin/ip', b'-j', b'route', b'show', b'default'],
                             [b'/usr/sbin/ip', b'-j', b'-6', b'route', b'show', b'default']],
            '/usr/sbin/sysctl': [[b'/usr/sbin/sysctl', b'-q', b'-w',
                                 b'net.ipv4.ip_unprivileged_port_start=0']],
        }
        for tool, commands in allowed.items():
            if executable != Path(tool).resolve() or argv not in commands:
                continue
            proc = Path('/proc') / str(identity['pid'])
            raw = _read_proc(proc / 'stat')
            parent_pid = int(raw[raw.rfind(b')') + 2:].split()[1])
            parent = process_identity(parent_pid)
            parent_path = Path('/proc') / str(parent_pid)
            parent_argv = _read_proc(parent_path / 'cmdline').rstrip(b'\0').split(b'\0')
            parent_executable = Path(os.readlink(parent_path / 'exe'))
            if parent_argv != outer and parent_argv != [b'/usr/bin/python3', b'/sandbox.py',
                    b'--inner', b'/config.json']:
                continue
            require(_verify_transport_process(state, parent, parent_executable, parent_argv,
                    artifacts, depth + 1) and process_identity(identity['pid']) == identity,
                    'cleanup_incomplete')
            return True
    return False


def _pidfd_signal(identity, number):
    if _process_absent(identity):
        return
    require(process_identity(identity['pid']) == identity, 'process_identity_changed')
    descriptor = os.pidfd_open(identity['pid'])
    try:
        require(process_identity(identity['pid']) == identity, 'process_identity_changed')
        signal.pidfd_send_signal(descriptor, number)
    except ProcessLookupError:
        require(_process_absent(identity), 'cleanup_incomplete')
    finally:
        os.close(descriptor)


def _observe_linux(state, marker=None):
    identities = _linux_scan(state, marker)
    for identity in identities:
        path = Path(state['registry']) / ('observed-' +
            hashlib.sha256(_canonical(identity)).hexdigest() + '.json')
        if not path.exists():
            _new_json(path, identity)
    return identities


def _clean_linux(state, marker=None):
    for number, grace in ((signal.SIGINT, 1), (signal.SIGTERM, 1), (signal.SIGKILL, 2)):
        identities = _observe_linux(state, marker)
        for identity in identities:
            _pidfd_signal(identity, number)
        deadline = continuous_time() + grace
        while identities and continuous_time() < deadline:
            time.sleep(.05)
            identities = _linux_scan(state, marker)
        if not identities:
            break
    require(not _linux_scan(state, marker), 'cleanup_incomplete')
    time.sleep(.05)
    require(not _linux_scan(state, marker), 'cleanup_incomplete')


def _finish_child(child, identity, *, interrupted, runtime):
    """Keep the leader reserved until all group signals have completed."""
    if interrupted:
        _signal_group(child, identity, signal.SIGINT if runtime else signal.SIGTERM)
        _wait_exit(child, 5 if runtime else 1)
    if not _exited(child):
        _signal_group(child, identity, signal.SIGTERM)
        _wait_exit(child, 1)
    expected = _signal_group(child, identity, signal.SIGKILL)
    require(_wait_exit(child, 2), 'cleanup_incomplete')
    code = child.wait(timeout=1)
    require(expected is None or code == expected, 'cleanup_incomplete')
    # A group must disappear after the reserved leader is reaped. Never signal
    # that numeric group again: it may now belong to another process.
    deadline = continuous_time() + 2
    while continuous_time() < deadline:
        try:
            os.killpg(child.pid, 0)
        except OSError as error:
            if error.errno == errno.ESRCH:
                return code
            if error.errno != errno.EPERM:
                raise
        time.sleep(.02)
    raise SmokeError('cleanup_incomplete')


def run_bounded(argv, state, *, timeout, output_limit, runtime, stdin=b''):
    _validate_state(state)
    require(isinstance(argv, list) and argv and all(isinstance(arg, (str, bytes)) for arg in argv)
            and type(output_limit) is int and 0 < output_limit <= 4 * 1024 * 1024
            and isinstance(stdin, bytes) and math.isfinite(timeout) and timeout > 0,
            'invalid_process')
    started = clock_sample()
    deadline = min(continuous_time() + timeout, state['overall_deadline'])
    require(continuous_time() < deadline, 'operation_timeout')
    token = secrets.token_hex(16)
    marker = state['execution_id'] + ':' + token
    env = child_environment(state, runtime=runtime)
    env['EPIPHYTIC_EXECUTION_ID'] = marker
    child = subprocess.Popen(argv, cwd=state['home'], env=env,
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             close_fds=True, start_new_session=True)
    record_path = Path(state['registry']) / ('child-' + token + '.json')
    try:
        identity = process_identity(child.pid)
        record = {'identity': identity, 'runtime': runtime, 'finished': False,
                  'cleanup': None, 'execution_id': marker}
        _new_json(record_path, record)
    except BaseException:
        # Popen's new session leader is still unreaped, reserving this PGID.
        with contextlib.suppress(ProcessLookupError):
            os.killpg(child.pid, signal.SIGKILL)
        child.wait(timeout=2)
        for pipe in (child.stdin, child.stdout, child.stderr):
            pipe.close()
        raise
    buffers = {'stdout': bytearray(), 'stderr': bytearray()}
    first_output_ns = last_output_ns = None
    timed_out = output_limited = False
    cleanup = 'incomplete'
    control_verified = False
    code = None
    try:
        with selectors.DefaultSelector() as selector:
            for name in ('stdout', 'stderr'):
                pipe = getattr(child, name)
                os.set_blocking(pipe.fileno(), False)
                selector.register(pipe, selectors.EVENT_READ, name)
            os.set_blocking(child.stdin.fileno(), False)
            if stdin:
                selector.register(child.stdin, selectors.EVENT_WRITE, 'stdin')
            else:
                child.stdin.close()
            offset = 0
            next_scan = continuous_time() + .5
            while selector.get_map() or not _exited(child):
                if continuous_time() >= deadline:
                    timed_out = True
                    break
                if runtime and sys.platform == 'darwin':
                    control_verified |= _mac_control_evidence(state, argv)
                if sys.platform.startswith('linux') and continuous_time() >= next_scan:
                    _observe_linux(state, marker)
                    next_scan = continuous_time() + .5
                for key, _events in selector.select(min(.1, max(0, deadline - continuous_time()))):
                    if key.data == 'stdin':
                        try:
                            offset += os.write(key.fd, stdin[offset:offset + 4096])
                        except BrokenPipeError:
                            offset = len(stdin)
                        if offset == len(stdin):
                            selector.unregister(key.fileobj)
                            key.fileobj.close()
                    else:
                        data = os.read(key.fd, 16384)
                        if not data:
                            selector.unregister(key.fileobj)
                            key.fileobj.close()
                            continue
                        observed_ns = clock_sample()['continuous_ns'] - started['continuous_ns']
                        if first_output_ns is None:
                            first_output_ns = observed_ns
                        last_output_ns = observed_ns
                        available = output_limit - sum(map(len, buffers.values()))
                        buffers[key.data].extend(data[:available])
                        if len(data) > available:
                            output_limited = True
                            break
                if output_limited:
                    break
        code = _finish_child(child, identity, interrupted=timed_out or output_limited, runtime=runtime)
        if sys.platform.startswith('linux'):
            _clean_linux(state, marker)
        cleanup = 'complete'
    finally:
        if child.returncode is None:
            with contextlib.suppress(OSError, SmokeError, ChildProcessError):
                code = _finish_child(child, identity, interrupted=True, runtime=runtime)
        for pipe in (child.stdin, child.stdout, child.stderr):
            pipe.close()
        record.update(finished=child.returncode is not None, cleanup=cleanup,
                      returncode=child.returncode, timed_out=timed_out, output_limited=output_limited)
        if runtime and sys.platform == 'darwin':
            record['control_verified'] = control_verified
        save_json(record_path, record)
    return {'returncode': code, 'timed_out': timed_out, 'output_limited': output_limited,
            'stdout': bytes(buffers['stdout']), 'stderr': bytes(buffers['stderr']),
            'timing': elapsed(started, clock_sample()), 'cleanup': cleanup,
            'first_output_ns': first_output_ns, 'last_output_ns': last_output_ns}


def interpreter_info(state):
    result = run_bounded([state['python'], '-I', '-B', '-c',
                          'import json,sys;print(json.dumps(list(sys.version_info[:3])))'],
                         state, timeout=10, output_limit=4096, runtime=False)
    require(result['returncode'] == 0 and not result['stderr'] and result['cleanup'] == 'complete',
            'interpreter_failed')
    version = strict_json(result['stdout'])
    require(isinstance(version, list) and len(version) == 3
            and all(type(item) is int and item >= 0 for item in version), 'interpreter_failed')
    path = Path(state['python'])
    classification = ('system' if str(path).startswith(('/usr/', '/System/')) else
                      'homebrew' if str(path).startswith(('/opt/homebrew/', '/usr/local/')) else 'ci-bootstrap')
    return {'implementation': 'cpython', 'version': version, 'path_class': classification}


IDENTITY_FAILURE_REASONS = frozenset({
    'nonzero_exit', 'timed_out', 'output_limited', 'cleanup_incomplete',
    'invalid_identity_output',
})



class IdentityError(SmokeError):
    def __init__(self, result=None, *, reason='invalid_identity_output'):
        super().__init__('identity_init_failed')
        from marmish_release_acceptance import DIAGNOSTIC_ERRORS
        result = result if type(result) is dict else {}
        code = result.get('returncode')
        self.returncode = code if type(code) is int and -255 <= code <= 255 else None
        self.signal = -code if type(code) is int and -64 <= code < 0 else None
        self.identity_reason = reason if type(reason) is str and reason in IDENTITY_FAILURE_REASONS else 'other'
        for key in ('timed_out', 'output_limited'):
            item = result.get(key)
            setattr(self, 'identity_' + key, item if type(item) is bool else None)
        cleanup = result.get('cleanup')
        self.identity_cleanup = cleanup if type(cleanup) is str and cleanup in ('complete', 'incomplete') else 'other'
        stderr = result.get('stderr')
        self.identity_frontend_category = 'none' if type(stderr) is bytes and stderr == b'' else 'other'
        if type(stderr) is bytes:
            for category in DIAGNOSTIC_ERRORS:
                if stderr == b'marmish: ' + category.encode('ascii') + b'\n':
                    self.identity_frontend_category = category
                    break


def initialize_identity(state, name, *, timeout):
    if sys.platform == 'darwin':
        start_caffeinate(state)
    require(name in ('server', 'client'), 'invalid_identity')
    path = Path(state['root']) / name
    require(not os.path.lexists(path), 'identity_exists')
    args = [str(Path(state['prefix']) / 'bin/marmish'), '--init', '-i', str(path),
            '--key-storage', 'files']
    for relay in RELAYS:
        args.extend(['--relay', relay])
    result = run_bounded(args, state, timeout=timeout, output_limit=65536, runtime=True)
    if (result['returncode'] != 0 or result['timed_out'] or result['output_limited']
            or result['cleanup'] != 'complete'):
        reason = ('timed_out' if result['timed_out'] else
                  'output_limited' if result['output_limited'] else
                  'cleanup_incomplete' if result['cleanup'] != 'complete' else 'nonzero_exit')
        raise IdentityError(result, reason=reason)
    try:
        value = strict_json(result['stdout'])
        require(isinstance(value, dict) and set(value) == {'account'}
                and isinstance(value['account'], str) and _HEX.fullmatch(value['account']),
                'identity_init_failed')
    except (RpcError, ValueError, SmokeError, TypeError):
        raise IdentityError(result, reason='invalid_identity_output') from None
    other_name = 'client' if name == 'server' else 'server'
    other = load_json(Path(state['registry']) / ('identity-' + other_name + '.json'))
    require(other is None or other['account'] != value['account'], 'identity_duplicate')
    _new_json(Path(state['registry']) / ('identity-' + name + '.json'), value)
    return value['account']


def _mac_control_evidence(state, argv):
    from marmish_transport_macos import control_dir, _valid_identity
    for name in ('server', 'client'):
        profile = str(Path(state['root']) / name)
        if profile not in argv:
            continue
        home = Path(profile) / 'mdk'
        path = control_dir(home) / '.marmish-run.json'
        if path.exists():
            try:
                record = strict_json(read_private(path, 16384))
            except FileNotFoundError:
                continue
            identities = [_valid_identity(record[key]) for key in ('daemon', 'shim')]
            for identity in identities:
                require(_process_absent(identity) or process_identity(identity['pid']) == identity,
                        'cleanup_incomplete')
                destination = Path(state['registry']) / ('native-' +
                    hashlib.sha256(_canonical(identity)).hexdigest() + '.json')
                if not destination.exists():
                    _new_json(destination, identity)
            return True
    return False


def start_caffeinate(state):
    """One owned macOS assertion across independent installer/identity stages."""
    _validate_state(state)
    if sys.platform != 'darwin':
        return
    record_path = Path(state['registry']) / 'caffeinate.json'
    if record_path.exists():
        record = strict_json(read_private(record_path))
        require(not _process_absent(record['identity']), 'caffeinate_failed')
        return
    child = subprocess.Popen(['/usr/bin/caffeinate', '-dimsu'], stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        env=child_environment(state, runtime=False), close_fds=True, start_new_session=True)
    try:
        _new_json(record_path, {'identity': process_identity(child.pid), 'executable': '/usr/bin/caffeinate'})
    except BaseException:
        child.kill()
        child.wait(timeout=2)
        raise


def _stop_caffeinate(state):
    path = Path(state['registry']) / 'caffeinate.json'
    if not path.exists():
        return
    record = strict_json(read_private(path))
    require(record['executable'] == '/usr/bin/caffeinate', 'cleanup_incomplete')
    identity = record['identity']
    for number in (signal.SIGTERM, signal.SIGKILL):
        if _process_absent(identity):
            return
        require(process_identity(identity['pid']) == identity, 'process_identity_changed')
        os.kill(identity['pid'], number)
        until = continuous_time() + 2
        while continuous_time() < until:
            if _process_absent(identity):
                return
            # Reap only a child we still own, without confusing PID replacement.
            with contextlib.suppress(ChildProcessError):
                os.waitpid(identity['pid'], os.WNOHANG)
            time.sleep(.02)
    require(_process_absent(identity), 'cleanup_incomplete')


def _native_cleanup(state):
    if sys.platform.startswith('linux'):
        _clean_linux(state)
        return
    from marmish_transport_macos import (DarwinConfinedDaemon, cleanup_control, control_dir,
                                         _open_control, _lock_control)
    for record in _registry_records(state, 'child-'):
        if record['runtime']:
            uncertain = (not record.get('finished') or record.get('cleanup') != 'complete' or
                         record.get('timed_out') or record.get('output_limited') or
                         record.get('returncode') is None or record.get('returncode', 0) < 0)
            require(not uncertain or record.get('control_verified') is True, 'cleanup_incomplete')
    for name in ('server', 'client'):
        home = Path(state['root']) / name / 'mdk'
        if os.path.lexists(control_dir(home)):
            # Reuse the production authenticated recovery protocol under its
            # existing lock; never infer daemon ownership from a PID alone.
            daemon = DarwinConfinedDaemon(home, '/unused', RELAYS, '0' * 64)
            descriptor = _open_control(home)
            lock = None
            try:
                lock = _lock_control(descriptor)
                daemon._control_fd = descriptor
                daemon._recover()
            finally:
                if lock is not None:
                    os.close(lock)
                os.close(descriptor)
            cleanup_control(home)
    for identity in _registry_records(state, 'native-'):
        require(_process_absent(identity), 'cleanup_incomplete')


def cleanup_state(state, *, runtime_cleanup=None):
    started = clock_sample()
    checks = {name: None for name in CHECKS}
    error = 'cleanup_incomplete'
    stage = 'cleanup_validate'
    try:
        _validate_state(state)
        stage = 'cleanup_caffeinate'
        _stop_caffeinate(state)
        stage = 'cleanup_native'
        _native_cleanup(state)
        stage = 'cleanup_frontends'
        for record in _registry_records(state, 'child-'):
            require(_process_absent(record['identity']), 'cleanup_incomplete')
        checks['frontend_absent'] = checks['caffeinate_absent'] = True
        checks['mdk_absent'] = True
        stage = 'cleanup_runtime'
        if runtime_cleanup is not None:
            result = runtime_cleanup(state)
            require(isinstance(result, dict) and result.get('workers_absent') is True
                    and result.get('engine_cleanup_ready') is True, 'cleanup_incomplete')
        else:
            require(all(not (Path(state['root']) / name / 'engine').exists()
                        for name in ('server', 'client')), 'cleanup_incomplete')
        checks['workers_absent'] = checks['engine_cleanup_ready'] = True
        stage = 'cleanup_uninstall'
        prefix = Path(state['prefix'])
        marker = prefix / '.marmish-install.json'
        if os.path.lexists(marker):
            _file_bytes(marker, 16384)
            raw = _file_bytes(Path(state['installer']))
            require(hashlib.sha256(raw).hexdigest() == state['installer_sha256'], 'uninstall_failed')
            result = run_bounded(['/bin/sh', state['installer'], '--uninstall', '--prefix', str(prefix)],
                                 state, timeout=30, output_limit=2 * 1024 * 1024, runtime=False)
            require(result['returncode'] == 0 and not result['timed_out'] and
                    not result['output_limited'] and result['cleanup'] == 'complete', 'uninstall_failed')
        require(not any(os.path.lexists(path) for path in
                        (marker, prefix / 'current', prefix / 'bin/marmish', prefix / 'bin/marmishd')),
                'uninstall_failed')
        checks['uninstall_complete'] = True
        stage = 'cleanup_final'
        _native_cleanup(state)
        _validate_state(state)
        pointer = Path(state['state_file'])
        record = strict_json(_file_bytes(pointer, 8192))
        require(record['root_identity'] == state['root_identity'] and
                record['state_sha256'] == hashlib.sha256(_canonical(state)).hexdigest(),
                'state_identity_changed')
        root = Path(state['root'])
        parent = _directory_fd(root.parent)
        stage = 'cleanup_remove'
        try:
            current = os.stat(root.name, dir_fd=parent, follow_symlinks=False)
            require(_root_identity(current) == state['root_identity'], 'state_identity_changed')
            shutil.rmtree(root.name, dir_fd=parent)
        finally:
            os.close(parent)
        require(not os.path.lexists(root), 'cleanup_incomplete')
        pointer.unlink()
        checks['root_removed'] = True
        error = None
    except (OSError, SmokeError, SandboxError, ValueError, KeyError, TypeError, AttributeError,
            RuntimeError) as failure:
        if isinstance(failure, SmokeError) and str(failure) == 'uninstall_failed':
            error = 'uninstall_failed'
        failure_diagnostic(failure, stage=stage, category=error)
    return {'outcome': 'complete' if error is None else 'incomplete', 'checks': checks,
            'error': error, 'duration_ns': elapsed(started, clock_sample())['elapsed_ns']}
