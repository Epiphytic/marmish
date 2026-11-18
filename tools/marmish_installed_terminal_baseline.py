"""Build and exercise one trusted native checkout with private fixture transport."""

from argparse import Namespace
import hashlib
import os
from pathlib import Path
import re
import shutil
import stat
import tempfile
import time
import tomllib

from marmish_native_https_check import bounded_read, decode, require, supervisor, version_value

PAYLOADS = ('marmish', 'marmishd', 'marmish-worker')
CONTROLS = ('mdk_adapter::terminal_route_proof_tests::terminal_command_fixture::'
            'terminal_installed_runtime_tests::foreground::terminal_installed_runtime_native_controls')
CARGO_CAPTURE = 16 * 1024 * 1024
MIN_BUILD_BYTES = 12 * 1024 ** 3
MAX_EXECUTABLE = 256 * 1024 * 1024
RUNTIME_ROOT_ENV = 'MARMISH_TEST_INSTALLED_RUNTIME_ROOT'


class Deadline:
    def __init__(self, expires):
        self.expires = expires

    def remaining(self, limit):
        remaining = self.expires - time.monotonic()
        require(remaining > 0, 'observation_deadline')
        return min(remaining, limit)


def digest(path, limit=MAX_EXECUTABLE):
    before = path.lstat()
    require(stat.S_ISREG(before.st_mode) and before.st_size <= limit, 'input_size_or_type')
    value = hashlib.sha256()
    size = 0
    with path.open('rb') as handle:
        for data in iter(lambda: handle.read(1024 * 1024), b''):
            size += len(data)
            require(size <= limit, 'input_size_or_type')
            value.update(data)
    require(size == before.st_size, 'input_changed')
    return value.hexdigest()


def environment(root, source, commit, account, target):
    result = {key: os.environ[key] for key in ('PATH', 'CARGO_HOME', 'RUSTUP_HOME',
              'EPIPHYTIC_EXECUTION_ID', 'RUNNER_TRACKING_ID') if key in os.environ}
    result.update(HOME=account.pw_dir, USER=account.pw_name, LOGNAME=account.pw_name,
                  LC_ALL='C', TMPDIR=str(root), CARGO_TARGET_DIR=str(root / 'target'),
                  CARGO_NET_OFFLINE='true', CARGO_TERM_COLOR='never',
                  MARMISH_BUILD_COMMIT=commit, CARGO_PROFILE_TEST_DEBUG='0',
                  GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT='0')
    toolchain = tomllib.loads(bounded_read(source / 'rust-toolchain.toml', 4096).decode('ascii'))
    channel = toolchain['toolchain']['channel']
    require(re.fullmatch(r'[1-9][0-9]{0,2}\.[0-9]{1,3}\.[0-9]{1,3}', channel), 'toolchain_pin')
    result['RUSTUP_TOOLCHAIN'] = channel
    if target.endswith('apple-darwin'):
        result['MACOSX_DEPLOYMENT_TARGET'] = '15.0'
    return result


def check_preflight(root, target, tmux, env, budget, report):
    require(shutil.disk_usage(root).free >= MIN_BUILD_BYTES, 'insufficient_build_disk')
    _, raw = supervisor.execute(['rustc', '-Vv'], env, timeout=budget.remaining(30))
    require(f"host: {target}".encode() in raw.splitlines()
            and f"release: {env['RUSTUP_TOOLCHAIN']}".encode() in raw.splitlines(), 'native_toolchain')
    report['rust'] = env['RUSTUP_TOOLCHAIN']

    # The path is stock and resolved. Native execution retains its own authoritative check.
    metadata = tmux.lstat()
    uid = os.geteuid()
    require(stat.S_ISREG(metadata.st_mode) and bool(metadata.st_mode & 0o111)
            and metadata.st_uid in (0, uid) and not metadata.st_mode & 0o6022, 'tmux_untrusted')
    for ancestor in tmux.parents:
        metadata = ancestor.lstat()
        require(stat.S_ISDIR(metadata.st_mode) and metadata.st_uid in (0, uid)
                and not metadata.st_mode & 0o6022, 'tmux_untrusted')
    _, raw = supervisor.execute([tmux, '-V'], env, timeout=budget.remaining(5))
    version = re.fullmatch(rb'tmux ([1-9][0-9]{0,2})\.(0|[1-9][0-9]{0,2})([a-z]?)\n', raw)
    require(version and (int(version[1]), int(version[2])) >= (3, 3), 'tmux_version')
    report['tmux'] = raw.removeprefix(b'tmux ').strip().decode('ascii')


def package_contract(target):
    return ('adhoc', 'macos=15.0') if target.endswith('apple-darwin') else ('none', 'glibc=2.39')


def package(root, source, version, target, env, budget, report):
    inputs, assets = root / 'inputs', root / 'assets'
    inputs.mkdir(mode=0o700)
    bin_dir = root / 'target' / 'release'
    for name in PAYLOADS:
        shutil.copyfile(bin_dir / name, inputs / name)
        (inputs / name).chmod(0o700)
    signing, minimum = package_contract(target)
    report['stage'] = 'native_signing'
    if signing == 'adhoc':
        for name in PAYLOADS:
            path = inputs / name
            for operation in (['--force', '--sign', '-', path], ['--verify', '--strict', path]):
                supervisor.execute(['/usr/bin/codesign', *operation], env, timeout=budget.remaining(45))
            _, details = supervisor.execute(['/bin/sh', '-c',
                'exec /usr/bin/codesign -dv --verbose=4 "$1" 2>&1', 'sh', path], env,
                timeout=budget.remaining(15))
            require(b'Signature=adhoc' in details.splitlines() and not any(line.startswith(b'Authority=') for line in details.splitlines()),
                    'adhoc_signature')
    report['stage'] = 'native_abi'
    code, _ = supervisor.execute(['/bin/sh', source / 'scripts/v02-inspect-native.sh', inputs],
                                 env, timeout=budget.remaining(120), success=False)
    require(code == 0, 'native_abi_blocked')
    report['stage'] = 'native_package'
    supervisor.execute([bin_dir / 'marmish-release-tool', 'package', '--input-dir', inputs,
                        '--output-dir', assets, '--licenses', root / 'licenses',
                        '--signing', signing, '--minimum', minimum], env, timeout=budget.remaining(120))
    manifest = assets / f'marmish-{version}-{target}.manifest'
    lines = bounded_read(manifest, 4096).decode('ascii').splitlines()
    require(lines[4:6] == [f'signing {signing} notarized=no', f'minimum {minimum}'], 'manifest_metadata')
    report['manifest_sha256'] = digest(manifest, limit=4096)
    report['package_asset_sha256'] = {
        name: digest(assets / f'{name}-{version}-{target}')
        for name in (*PAYLOADS, 'THIRD-PARTY-LICENSES')
    }
    report['package_inputs_sha256'] = {name: digest(inputs / name) for name in PAYLOADS}
    return assets


def executable(raw, source, cargo_target, version):
    expected_package = "path+" + (source / "crates/marmish-native").as_uri() + "#" + version
    found = []
    for line in raw.splitlines():
        item = decode(line)
        if item.get('reason') != 'compiler-artifact' or not item.get('executable'):
            continue
        package = item.get('package_id')
        if (package == expected_package
                and item.get('profile', {}).get('test') is True
                and item.get('target', {}).get('name') == 'marmish_native'
                and item['target'].get('kind') == ['lib']):
            found.append(Path(item['executable']))
    require(len(found) == 1, 'fixture_controller_count')
    path = found[0]
    require(path.is_absolute() and path.resolve() == path
            and path.parent == cargo_target / 'debug' / 'deps'
            and re.fullmatch(r'marmish_native-[0-9a-f]{16}', path.name)
            and path.is_file() and os.access(path, os.X_OK), 'fixture_controller_path')
    return path


def cleanup_tmux(root, tmux, env):
    # Only these two exact filesystem levels occur in the test-only short root.
    for attempt in root.iterdir():
        require(re.fullmatch(r'ir-[A-Za-z0-9]+', attempt.name)
                and private_directory(attempt), 'fixture_cleanup_incomplete')
        folder = attempt / 's' / 't'
        if not os.path.lexists(folder):
            continue
        require(private_directory(attempt / 's') and private_directory(folder), 'fixture_cleanup_incomplete')
        slots = list(folder.iterdir())
        require(len(slots) <= 8, 'fixture_cleanup_incomplete')
        for slot in slots:
            require(re.fullmatch(r'[0-9a-f]{32}', slot.name) and private_directory(slot),
                    'fixture_cleanup_incomplete')
            sock = slot / 's'
            if not os.path.lexists(sock):
                continue
            metadata = sock.lstat()
            require(stat.S_ISSOCK(metadata.st_mode) and metadata.st_uid == os.geteuid(),
                    'fixture_cleanup_incomplete')
            try:
                supervisor.execute([tmux, '-S', sock, 'kill-server'], env, timeout=3)
            except supervisor.Refused as error:
                if error.args != ('operation_failed',):
                    raise
            until = time.monotonic() + 2
            while os.path.lexists(sock) and time.monotonic() < until:
                time.sleep(0.05)
            require(not os.path.lexists(sock), 'fixture_cleanup_incomplete')


def private_directory(path):
    metadata = path.lstat()
    return (stat.S_ISDIR(metadata.st_mode) and stat.S_IMODE(metadata.st_mode) == 0o700
            and metadata.st_uid == os.geteuid())


def installed_fixture(root, installed, controller, tmux, env, budget, report, check_case):
    shadow = root / 'shadow'
    deps = shadow / 'deps'
    shadow.mkdir(mode=0o700)
    deps.mkdir(mode=0o700)
    original = {name: digest(installed / name) for name in PAYLOADS}
    require(original == report['package_inputs_sha256'], 'fixture_payload_digest')
    for name in PAYLOADS:
        shutil.copyfile(installed / name, shadow / name)
        (shadow / name).chmod(0o555)
    shutil.copyfile(controller, deps / controller.name)
    executable_path = deps / controller.name
    executable_path.chmod(0o500)
    require({name: digest(shadow / name) for name in PAYLOADS} == original, 'fixture_payload_digest')

    report['stage'] = 'installed_terminal_controls'
    report['fixture_case'] = CONTROLS
    began = time.monotonic()
    runtime_root = None
    try:
        with tempfile.TemporaryDirectory(prefix='mq-', dir=Path('/tmp').resolve()) as path:
            runtime_root = Path(path)
            fixture_path, ipc_path = runtime_root / 'pty', runtime_root / 'ipc'
            fixture_path.mkdir(mode=0o700)
            ipc_path.mkdir(mode=0o700)
            run_env = dict(env, TMPDIR=str(ipc_path), **{RUNTIME_ROOT_ENV: str(fixture_path)})
            try:
                result = supervisor.execute([executable_path, CONTROLS, '--ignored', '--exact',
                                             '--test-threads=1'], run_env,
                                            timeout=budget.remaining(240), success=False)
                check_case(*result, report)
                require(not any(ipc_path.glob('mr2-*')), 'fixture_ipc_cleanup_incomplete')
            except supervisor.Refused as failure:
                report['fixture_failure'] = str(failure)
                raise
            finally:
                cleanup_tmux(fixture_path, tmux, run_env)
                report['fixture_tmux_cleanup'] = 'checked'
    finally:
        if runtime_root is not None:
            report['fixture_root_removed'] = not os.path.lexists(runtime_root)
    report['owned_fixture_join'] = 'checked'
    report['controls_elapsed_ms'] = round((time.monotonic() - began) * 1000)
    require({name: digest(shadow / name) for name in PAYLOADS} == original
            and {name: digest(installed / name) for name in PAYLOADS} == original, 'fixture_payload_digest')
    report.update(installed_payload_sha256=original, fixture_payload='digest_identical_shadow',
                  fixture_count=1, fixture_passes=[CONTROLS])
    del report['fixture_case']


def run(options, root, account, relay, tmux, report, install, check_case):
    deadline = Deadline(time.monotonic() + 90 * 60)
    env = environment(root, options.source, options.commit, account, report['target'])
    report['stage'] = 'native_preflight'
    check_preflight(root, report['target'], tmux, env, deadline, report)
    cargo_toml = options.source / 'crates/marmish-native/Cargo.toml'
    version = version_value(tomllib.loads(bounded_read(cargo_toml, 16384).decode('utf-8'))['package']['version'])
    report['cargo_lock_sha256'] = digest(options.source / 'Cargo.lock', 8 * 1024 * 1024)
    (root / 'licenses').write_bytes(bounded_read(options.licenses, 16777216))
    report['licenses_sha256'] = digest(root / 'licenses', 16777216)
    cargo = ['cargo']
    common = ['--quiet', '--offline', '--locked', '-j2', '-p', 'marmish-native']
    report['stage'] = 'native_build'
    supervisor.execute([*cargo, 'build', *common, '--release', '--bins'], env,
                       timeout=deadline.remaining(3600), cwd=options.source)
    assets = package(root, options.source, version, report['target'], env, deadline, report)
    report['stage'] = 'fixture_controller_build'
    _, raw = supervisor.execute([*cargo, 'test', *common, '--no-run', '--lib', '--message-format=json'],
                                env, timeout=deadline.remaining(2400), cwd=options.source,
                                max_capture=CARGO_CAPTURE)
    controller = executable(raw, options.source, root / 'target', version)
    runtime = Deadline(min(time.monotonic() + 1200, deadline.expires))
    report['version'] = version
    installed = install(Namespace(**dict(vars(options), version=version, assets=assets)), root, account, relay, report,
                        runtime)
    installed_fixture(root, installed, controller, tmux, env, runtime, report, check_case)
