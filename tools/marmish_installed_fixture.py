"""CI-only fixed executable builder and independently specified byte vectors."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import sys

# Isolated Python omits the script directory; import only this fixed checkout's
# application modules after standard-library imports, never PYTHONPATH or cwd.
if __name__ == '__main__':
    sys.path.insert(0, str(Path(__file__).resolve().parent))

from marmish_installed_release import ROLES, canonical_bytes, digest_file, validate_context
from marmish_platform import continuous_time

FIXTURE_TIMEOUT = 120
FIXTURE_CLEANUP_GRACE = 125
EDGE_ARGS = (b'', b'two words', b"'single'", b'"double"', b'$HOME', b'$(id)',
             b'`id`', b'line\nbreak', b'back\\slash', b'**/*', '雪☃'.encode(), b'\xff')
SOURCE = Path(__file__).resolve().parent / 'installed-fixture'


def _hash(raw):
    return hashlib.sha256(raw).hexdigest()


def derive_pair(ctx, pin_sha256, client_ready, server_ready):
    """Derive labels from immutable identity-ready records, never server-ready."""
    validate_context(ctx)
    payload = dict(ctx, pin_sha256=pin_sha256,
                   client_role=client_ready['role'], server_role=server_ready['role'],
                   client_identity_ready_sha256=_hash(canonical_bytes(client_ready)),
                   server_identity_ready_sha256=_hash(canonical_bytes(server_ready)),
                   client=client_ready['client'], server=server_ready['server'],
                   client_nonce=client_ready['nonce'], server_nonce=server_ready['nonce'])
    context = hashlib.sha256(b'marmish-pair-v1\0' + canonical_bytes(payload)).digest()
    return {'context': context.hex(),
            'challenge': _hash(b'marmish-challenge-v1\0' + context),
            'invocation': _hash(b'marmish-invocation-v1\0' + context)[:32],
            'probe_invocation': _hash(b'marmish-probe-v1\0' + context)[:32]}


def command_argv(executable, challenge, invocation):
    return [os.fsencode(executable), challenge.encode('ascii'), invocation.encode('ascii'),
            *EDGE_ARGS]


def stdin_bytes(context):
    prefix = b'marmish-stdin-v1\0' + bytes.fromhex(context)
    return prefix + bytes(range(256)) * ((8192 - len(prefix)) // 256) + bytes(range((8192 - len(prefix)) % 256))


def _encoded_argv(executable, challenge, invocation):
    return b''.join(struct.pack('>I', len(arg)) + arg
                    for arg in command_argv(executable, challenge, invocation))


def expected_streams(executable, context, challenge, invocation):
    block = b''.join(hashlib.sha256(b'marmish-output-v1\0' + bytes.fromhex(context)
                                   + struct.pack('>I', counter)).digest()
                     for counter in range(2048))
    return (b'MARMISH-INTEROP\0\x01' + _encoded_argv(executable, challenge, invocation) + block,
            stdin_bytes(context)[::-1])


def expected_receipt(role, context, challenge, invocation, executable):
    stdout, stderr = expected_streams(executable, context, challenge, invocation)
    return {'format': 1, 'kind': 'fixture-receipt', 'role': role, 'context': context,
            'challenge': challenge, 'invocation': invocation,
            'argv_sha256': _hash(_encoded_argv(executable, challenge, invocation)),
            'stdin_sha256': _hash(stdin_bytes(context)), 'stdout_bytes': len(stdout),
            'stdout_sha256': _hash(stdout), 'stderr_bytes': len(stderr),
            'stderr_sha256': _hash(stderr), 'outcome': 7}


def source_sha256():
    """Names and exact source bytes, including this expected-data specification."""
    files = [SOURCE / 'Cargo.toml', SOURCE / 'Cargo.lock', SOURCE / 'src/main.rs', Path(__file__)]
    return _hash(b''.join(path.name.encode() + b'\0' + path.read_bytes() + b'\0' for path in files))


IDENTITY_RESERVE = 600
COMPILER_CLEANUP_MARGIN = 20
REPORT_RESERVE = 5
PREPARATION_RESERVE = IDENTITY_RESERVE + COMPILER_CLEANUP_MARGIN + REPORT_RESERVE


def dependency_deadline(state, deadline):
    cutoff = min(deadline, state['preparation_deadline'] - PREPARATION_RESERVE)
    if continuous_time() >= cutoff:
        raise ValueError('fixture_preparation_timeout')
    return cutoff


def _no_cargo_config(home):
    for base in [SOURCE, *SOURCE.parents]:
        if any(os.path.lexists(base / '.cargo' / name) for name in ('config', 'config.toml')):
            raise ValueError('fixture_cargo_config')
    if any(os.path.lexists(Path(home) / name) for name in ('config', 'config.toml')):
        raise ValueError('fixture_cargo_config')


def _fence(info):
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_nlink,
            info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def _copy_toolchain(source, destination, *, deadline):
    """Copy the earlier pinned workflow bootstrap, with no followed links."""
    import stat
    from marmish_ci import MANIFEST_SHA256, RUST
    source = Path(source).absolute()
    before = source.lstat()
    if not stat.S_ISDIR(before.st_mode) or before.st_uid != os.geteuid() or before.st_mode & 2:
        raise ValueError('fixture_toolchain_invalid')
    marker = source / '.marmish-toolchain.json'
    info = marker.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 4 * 1024 * 1024:
        raise ValueError('fixture_toolchain_invalid')
    manifest = json.loads(marker.read_bytes())
    hashes = manifest.get('hashes')
    if (set(manifest) != {'managed', 'rust', 'manifest', 'hashes'} or
            manifest['managed'] is not True or manifest['rust'] != RUST or
            manifest['manifest'] != MANIFEST_SHA256 or not isinstance(hashes, dict) or
            not 1 <= len(hashes) <= 20000):
        raise ValueError('fixture_toolchain_invalid')
    destination.mkdir(mode=0o700)
    copied, executables = {}, {}
    total = 0
    for directory, dirs, files in os.walk(source, followlinks=False):
        if continuous_time() >= deadline:
            raise ValueError('fixture_preparation_timeout')
        relative = Path(directory).relative_to(source)
        directory_info = Path(directory).lstat()
        if not stat.S_ISDIR(directory_info.st_mode) or directory_info.st_uid != os.geteuid() or directory_info.st_mode & 2:
            raise ValueError('fixture_toolchain_invalid')
        target = destination / relative
        if relative != Path('.'):
            target.mkdir(mode=0o700)
        for name in dirs:
            if not stat.S_ISDIR((Path(directory) / name).lstat().st_mode):
                raise ValueError('fixture_toolchain_invalid')
        for name in files:
            path = Path(directory) / name
            entry = path.lstat()
            if (not stat.S_ISREG(entry.st_mode) or entry.st_nlink != 1 or
                    entry.st_uid != os.geteuid() or entry.st_mode & 0o7002 or entry.st_size > 1024**3):
                raise ValueError('fixture_toolchain_invalid')
            total += entry.st_size
            if total > 4 * 1024**3:
                raise ValueError('fixture_toolchain_invalid')
            key = str(path.relative_to(source))
            output = target / name
            with path.open('rb') as stream, output.open('xb') as result:
                digest = hashlib.sha256()
                while block := stream.read(1024 * 1024):
                    if continuous_time() >= deadline:
                        raise ValueError('fixture_preparation_timeout')
                    digest.update(block)
                    result.write(block)
                result.flush()
                os.fsync(result.fileno())
            if _fence(path.lstat()) != _fence(entry):
                raise ValueError('fixture_toolchain_changed')
            output.chmod(0o700 if entry.st_mode & 0o111 else 0o600)
            if key != '.marmish-toolchain.json':
                copied[key] = digest.hexdigest()
                if copied[key] != hashes.get(key) or digest_file(output) != copied[key]:
                    raise ValueError('fixture_toolchain_changed')
                if entry.st_mode & 0o111 and (key.startswith('bin/') or key.startswith('lib/rustlib/')):
                    executables[key] = copied[key]
            if len(executables) > 64:
                raise ValueError('fixture_toolchain_invalid')
        if _fence(Path(directory).lstat()) != _fence(directory_info):
            raise ValueError('fixture_toolchain_changed')
    if _fence(source.lstat()) != _fence(before) or copied != hashes or not all(name in copied for name in ('bin/cargo', 'bin/rustc', 'bin/rustdoc')):
        raise ValueError('fixture_toolchain_changed')
    return executables


def _compiler_config(state):
    from marmish_installed_state import read_private
    config = json.loads(read_private(Path(state['root']) / 'compiler-provenance.json'))
    _no_cargo_config(config['cargo_home'])
    for name in ('cargo', 'rustc', 'rustdoc'):
        if digest_file(Path(config[name])) != config['executables']['bin/' + name]:
            raise ValueError('fixture_toolchain_changed')
    return config


def _compile(state, constants, deadline, *, stage='build'):
    from marmish_installed_state import run_bounded
    config = _compiler_config(state)
    config = dict(config, constants=constants, stage=stage, source_sha256=source_sha256())
    result = run_bounded([state['python'], '-I', '-B', str(Path(__file__).resolve()), '--compile-worker'],
                         state, timeout=deadline - continuous_time(), output_limit=1024 * 1024,
                         runtime=False, stdin=canonical_bytes(config), build_scope=True)
    if result['returncode'] != 0 or result['timed_out'] or result['output_limited'] or result['cleanup'] != 'complete':
        raise ValueError('fixture_build_failed')
    if source_sha256() != config['source_sha256']:
        raise ValueError('fixture_source_changed')
    return config['version']


def prepare_dependencies(state, *, deadline, toolchain_source):
    """Fetch locked public crates then warm privately before identity creation."""
    from marmish_installed_state import run_bounded, write_private
    deadline = dependency_deadline(state, deadline)
    root = Path(state['root'])
    compiler = root / 'compiler'
    executables = _copy_toolchain(toolchain_source, compiler, deadline=deadline)
    home = root / 'cargo-home'
    home.mkdir(mode=0o700)
    _no_cargo_config(home)
    linker = Path(shutil.which('cc') or '/nonexistent').resolve(strict=True)
    if sys.platform.startswith('linux'):
        if any(path.stat().st_uid != 0 or path.stat().st_mode & 0o022 for path in (linker, *linker.parents)):
            raise ValueError('fixture_linker_invalid')
    config = {'cargo': str(compiler / 'bin/cargo'), 'rustc': str(compiler / 'bin/rustc'),
              'rustdoc': str(compiler / 'bin/rustdoc'), 'cargo_home': str(home),
              'target': str(root / 'fixture-target'), 'triple': __import__('marmish_installed_release').ROLE_TARGETS[state['role']],
              'linker': str(linker), 'executables': executables}
    versions = {}
    for name in ('rustc', 'cargo'):
        result = run_bounded([config[name], '--version'], state,
            timeout=min(15, deadline - continuous_time()), output_limit=4096, runtime=False)
        line = result['stdout'].decode('ascii').strip()
        if (result['returncode'] != 0 or result['timed_out'] or result['output_limited'] or
                result['cleanup'] != 'complete' or not line.startswith(name + ' 1.97.1 ') or '\n' in line):
            raise ValueError('fixture_compiler_invalid')
        versions[name] = line
    config['version'] = versions['rustc']
    write_private(root / 'compiler-provenance.json', config)
    _compile(state, {}, deadline, stage='fetch')
    return _compile(state, {}, deadline)


def build_fixture(state, client_role, pair, *, deadline):
    if client_role not in ROLES:
        raise ValueError('fixture_role_invalid')
    root = Path(state['root'])
    executable = root / 'f' / client_role / 'main'
    receipt = root / 'r' / (client_role + '.json')
    for directory in (root / 'f', executable.parent, receipt.parent):
        directory.mkdir(mode=0o700, exist_ok=True)
        info = directory.lstat()
        import stat
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o777 != 0o700:
            raise ValueError('fixture_directory_invalid')
    if os.path.lexists(executable) or os.path.lexists(receipt):
        raise ValueError('fixture_exists')
    constants = {'ROLE': client_role, 'CONTEXT': pair['context'], 'CHALLENGE': pair['challenge'],
                 'INVOCATION': pair['invocation'], 'RECEIPT': str(receipt),
                 'EXECUTABLE': str(executable)}
    compiler = _compile(state, constants, deadline)
    artifact = root / 'fixture-target' / 'debug' / 'marmish-installed-fixture'
    with artifact.open('rb') as source, executable.open('xb') as output:
        shutil.copyfileobj(source, output)
        output.flush()
        os.fsync(output.fileno())
    executable.chmod(0o700)
    digest = digest_file(executable)
    from marmish_installed_state import register_executable
    register_executable(state, executable, digest)
    return {'path': str(executable), 'sha256': digest, 'receipt': str(receipt),
            'provenance': {'harness_sha': state['ctx']['sha'], 'source_sha256': source_sha256(),
                           'compiler': compiler, 'binary_sha256': digest}}


def _compile_worker():
    """No subprocess fork: replace the tracked leader with direct pinned cargo."""
    config = json.loads(sys.stdin.buffer.read(65537))
    if config['stage'] not in ('fetch', 'build') or config['source_sha256'] != source_sha256():
        raise ValueError('fixture_worker_invalid')
    _no_cargo_config(config['cargo_home'])
    env = {key: value for key, value in os.environ.items() if key in
           {'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'EPIPHYTIC_EXECUTION_ID', 'RUNNER_TRACKING_ID'}}
    env.update(CARGO_HOME=config['cargo_home'], CARGO_TARGET_DIR=config['target'],
               RUSTC=config['rustc'], RUSTDOC=config['rustdoc'], RUSTC_WRAPPER='',
               RUSTC_WORKSPACE_WRAPPER='', PATH=str(Path(config['cargo']).parent) + ':/usr/bin:/bin')
    env['CARGO_TARGET_' + config['triple'].upper().replace('-', '_') + '_LINKER'] = config['linker']
    env.update({'MARMISH_FIXTURE_' + key: value for key, value in config['constants'].items()})
    os.chdir(SOURCE)
    args = [config['cargo'], config['stage'], '--locked']
    if config['stage'] == 'build':
        args.append('--offline')
    args.extend(['--manifest-path', str(SOURCE / 'Cargo.toml')])
    os.execve(config['cargo'], args, env)


if __name__ == '__main__':
    if sys.argv[1:] != ['--compile-worker']:
        raise SystemExit(125)
    _compile_worker()
