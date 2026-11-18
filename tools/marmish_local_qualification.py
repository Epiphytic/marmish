#!/usr/bin/env python3
"""Opt-in same-source native installer and Terminal qualification on one host."""
import argparse
import json
import os
from pathlib import Path
import pwd
import re
import shutil
import signal
import socket
import stat
import subprocess
import tempfile
import time

from marmish_native_https_check import (
    bounded_read, check_manifest, decode, release_contract, require, safe_version,
    setup_contract, sha, status_contract, supervisor, target_host, terminal,
)
from marmish_native_quickstart_check import DIALOG, UTILITIES, interactive, parse_release_version, reports
from mdk_smoke import SmokeError, public_key, relay_url

Refused, execute, stop = supervisor.Refused, supervisor.execute, supervisor.stop

PINS = {
    'x86_64-unknown-linux-gnu': 'AMD64',
    'aarch64-unknown-linux-gnu': 'ARM64',
    'aarch64-apple-darwin': 'MACOS',
}
LOCAL_CASES = (
    'frontend_local::terminal_frontend_renders_native_tmux',
    'frontend_local::terminal_socket_frontend_native_pty',
)
WSS_MODULE = ('mdk_adapter::terminal_route_proof_tests::terminal_command_fixture::'
              'terminal_installed_runtime_tests::')
WSS_CASES = (
    'terminal_installed_runtime_native_writer',
    'observe::terminal_installed_runtime_observer_read_only',
    'observe::terminal_installed_runtime_takeover_named_resume',
)
LIBTEST_SINGLE_FAILURE = re.compile(
    rb'test result: FAILED\. 0 passed; 1 failed; 0 ignored; 0 measured; '
    rb'[0-9]+ filtered out; finished in [0-9]+\.[0-9]+s'
)
STOCK_TMUX = ('/usr/bin/tmux', '/opt/homebrew/bin/tmux', '/usr/local/bin/tmux', '/opt/local/bin/tmux')


def verify_checkout(source, commit):
    environment = {name: os.environ[name] for name in ('HOME', 'PATH') if name in os.environ}
    command = ['git', '-C', source]
    _, head = execute([*command, 'rev-parse', '--verify', 'HEAD'], environment)
    _, dirty = execute([*command, 'status', '--porcelain=v1', '--untracked-files=all'], environment)
    require(head.strip() == commit.encode('ascii') and not dirty, 'checkout_binding')


def render_bootstrap(template, context, target):
    substitutions = {'@MARMISH_VERSION@': context['version']}
    substitutions.update({f'@MARMISH_SHA_{pin}@': context['manifest_pins'].get(platform, '0' * 64)
                          for platform, pin in PINS.items()})
    require(target in context['manifest_pins'], 'manifest_binding')
    for marker, value in substitutions.items():
        require(template.count(marker) == 1, 'bootstrap_tokens')
        template = template.replace(marker, value)
    require('@MARMISH_' not in template, 'bootstrap_tokens')
    return template


def expect_failure(argv, environment, timeout, category, message):
    try:
        execute(argv, environment, timeout=timeout, error_map={message: category, None: 'operation_failed'})
    except Refused as error:
        if error.args == (category,) and error.exit_code == 1:
            return
        raise
    raise Refused('expected_failure_missing')


def install_arguments(assets, prefix, bins, state):
    return ['--asset-dir', str(assets), '--prefix', str(prefix), '--bin-dir', str(bins),
            '--state-dir', str(state)]


def reject_tampered_asset(bootstrap, assets, manifest, environment, root, *, existing=None, timeout=60):
    changed = root / 'tamper'
    changed.mkdir(mode=0o700)
    shutil.copyfile(assets / manifest, changed / manifest)
    name = manifest.removesuffix('.manifest')
    source = assets / name
    info = source.lstat()
    require(stat.S_ISREG(info.st_mode) and 0 < info.st_size <= 268435456, 'asset_size_or_type')
    altered = changed / name
    shutil.copyfile(source, altered)
    with altered.open('r+b') as output:
        byte = output.read(1)
        output.seek(0)
        output.write(bytes([byte[0] ^ 1]))
    locations = existing or tuple(changed / name for name in ('prefix', 'bin', 'state'))
    prefix, bins, state = locations
    expect_failure(['/bin/sh', bootstrap, *install_arguments(changed, prefix, bins, state)],
                   environment, timeout, 'asset_digest', b'marmish installer: asset_digest (stage=asset_verify)')
    if existing is None:
        require(not any(os.path.lexists(path) for path in locations), 'tamper_mutated_install')
    shutil.rmtree(changed)


def run_live_foreground(daemon, status, environment, expected):
    child = subprocess.Popen(daemon, env=environment, stdin=subprocess.DEVNULL,
                             stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                             start_new_session=True)
    try:
        deadline = time.monotonic() + 155
        ready = False
        while child.poll() is None and time.monotonic() < deadline:
            try:
                code, output = execute(status, environment, timeout=3, success=False)
            except Refused as error:
                if error.args != ('operation_timeout',):
                    raise
                continue
            if code == 0:
                value = decode(output)
                snapshot = value['result']
                if value['ok'] is True and value['error'] is None and snapshot.get('state') == 'active':
                    require(snapshot['identity'] == expected, 'daemon_identity')
                    ready = True
                    break
            time.sleep(0.25)
        require(ready and child.poll() is None, 'daemon_not_active')
        expect_failure(daemon, environment, 15, 'profile_in_use', b'marmish: identity: profile_in_use')
        _, output = execute(status, environment, timeout=3)
        value = decode(output)
        require(value['ok'] is True and value['error'] is None and value['result']['state'] == 'active'
                and value['result']['identity'] == expected and child.poll() is None, 'daemon_lost_owner')
        child.send_signal(signal.SIGTERM)
        require(child.wait(timeout=40) == 0, 'daemon_shutdown_exit')
    finally:
        stop(child)


def verify_installation(root, raw, context, target, environment, remaining):
    prefix, bins = root / 'prefix', root / 'bin'
    _, output = execute([bins / 'marmish', 'install-status', '--prefix', prefix, '--bin-dir', bins], environment,
                        timeout=remaining(15))
    status_contract(decode(output), context, target, prefix, bins)
    directory = safe_version(prefix, bins)
    formats = check_manifest(raw, context, target, directory)
    for binary in (bins / 'marmish', bins / 'marmishd', directory / 'marmish-worker'):
        _, output = execute([binary, 'release-info'], environment, timeout=remaining(15))
        release_contract(decode(output), context, target, formats)
    return directory


def installed_snapshot(locations):
    from marmish_installed_terminal_baseline import digest
    entries = {}
    for location in locations:
        info = location.lstat()
        require(stat.S_ISDIR(info.st_mode), 'install_snapshot_type')
        entries[location] = (stat.S_IMODE(info.st_mode), None)
        for directory, subdirs, files in os.walk(location, followlinks=False):
            for name in (*subdirs, *files):
                path = Path(directory) / name
                info = path.lstat()
                require(len(entries) < 256, 'install_snapshot_count')
                mode = stat.S_IMODE(info.st_mode)
                if stat.S_ISLNK(info.st_mode):
                    value = os.readlink(path)
                elif stat.S_ISREG(info.st_mode):
                    value = digest(path)
                else:
                    require(stat.S_ISDIR(info.st_mode), 'install_snapshot_type')
                    value = None
                entries[path] = (mode, value)
    return entries


def installed_case(options, root, account, relay, report, budget=None):
    remaining = budget.remaining if budget is not None else lambda limit: limit
    target = report['target']
    report['stage'] = 'native_assets'
    manifest = f'marmish-{options.version}-{target}.manifest'
    raw = bounded_read(options.assets / manifest, 4096)
    expected = f'marmish-manifest 1\nversion {options.version}\ntarget {target}\ncommit {options.commit}\n'
    require(raw.startswith(expected.encode('ascii')), 'manifest_binding')
    context = {'source_commit': options.commit, 'version': options.version, 'manifest_pins': {target: sha(raw)}}
    utilities = root / 'utilities'
    utilities.mkdir(mode=0o700)
    for name in UTILITIES:
        executable = shutil.which(name, path=supervisor.UTILITY_DISCOVERY_PATH)
        if executable:
            (utilities / name).symlink_to(executable)
    environment = {'HOME': account.pw_dir, 'USER': account.pw_name, 'LOGNAME': account.pw_name,
                   'PATH': str(utilities), 'LC_ALL': 'C', 'TMPDIR': str(root)}
    for name in ('EPIPHYTIC_EXECUTION_ID', 'RUNNER_TRACKING_ID'):
        if name in os.environ:
            environment[name] = os.environ[name]
    template = bounded_read(options.source / 'scripts/install-native.sh', 65536).decode('ascii')
    bootstrap = root / 'install.sh'
    bootstrap.write_text(render_bootstrap(template, context, target), encoding='ascii')
    bootstrap.chmod(0o500)
    report['stage'] = 'tampered_payload'
    reject_tampered_asset(bootstrap, options.assets, manifest, environment, root, timeout=remaining(60))
    report['tampered_payload_refused'] = True

    report['stage'] = 'quickstart'
    prefix, bins, state = (root / name for name in ('prefix', 'bin', 'state'))
    dialog = (*DIALOG[:1], (DIALOG[1][0], relay.encode('ascii') + b'\n'), *DIALOG[2:])
    args = install_arguments(options.assets, prefix, bins, state)
    quickstart = [str(bootstrap), *args, '--quickstart', '--key-storage', 'file']
    if budget is None:
        output = interactive(quickstart, environment, dialog=dialog,
                             failure_diagnostics=report['quickstart_diagnostics'])
    else:
        output = terminal(quickstart, environment, remaining(240), dialog)
    setups = [value for value in reports(output) if 'identity_created' in value]
    require(len(setups) == 1, 'quickstart_setup_report')
    initial = setups[0]
    setup_contract(initial)
    config = decode(bounded_read(state / 'default' / 'config.json', 1048576))
    require(config['relays'] == [relay], 'quickstart_relay_binding')
    report['installed_quickstart'] = True

    report['stage'] = 'installed_provenance'
    directory = verify_installation(root, raw, context, target, environment, remaining)
    report['installed_provenance'] = True
    if budget is not None:
        report['stage'] = 'tampered_replacement'
        locations = (prefix, bins, state)
        before = installed_snapshot(locations)
        reject_tampered_asset(bootstrap, options.assets, manifest, environment, root,
                              existing=locations, timeout=remaining(60))
        require(installed_snapshot(locations) == before, 'tamper_mutated_install')
        report['tampered_replacement_refused'] = True

    report['stage'] = 'repeat_setup'
    profile = ['--state-dir', state, '--key-storage', 'file']
    _, output = execute([bins / 'marmish', *profile, 'setup', '--no-grants'], environment, timeout=remaining(45))
    setup_contract(decode(output), repeated=True, original=initial)
    report['repeat_setup'] = True

    report['stage'] = 'foreground_daemon'
    daemon = [bins / 'marmishd', *profile, '--foreground']
    if options.live_relay:
        run_live_foreground(daemon, [bins / 'marmish', *profile, 'status'], environment,
                            public_key(initial['npub']))
        report['live_relay'] = 'verified_active_and_stopped'
    else:
        expect_failure(daemon, environment, remaining(180), 'relay_connect', b'marmish: transport: relay_connect')
        report['closed_relay_refusal'] = True
    require(not any(root.glob('mr2-*')), 'daemon_runtime_leftover')
    report['daemon_runtime_removed'] = True
    return directory


def fixture_tmux(tmux, mode):
    require(tmux is not None and tmux.is_absolute(), 'absolute_tmux_required')
    try:
        tmux = tmux.resolve(strict=True)
    except OSError as error:
        raise Refused('tmux_missing') from error
    if mode == 'wss':
        stock = next((Path(path).resolve() for path in STOCK_TMUX if Path(path).exists()), None)
        require(tmux == stock, 'stock_tmux_mismatch')
    return tmux


def terminal_cases(options, tmux, report):
    # Explicitly opted-in, trusted development environment and cached Cargo inputs.
    os.chdir(options.source)
    environment = dict(os.environ, MARMISH_TEST_TMUX=str(tmux), MARMISH_BUILD_COMMIT=options.commit)
    cargo = ['cargo']
    common = ['--quiet', '--locked', '--offline', '-p', 'marmish-native']
    if options.terminal == 'wss':
        report['stage'] = 'fixture_binaries'
        execute([*cargo, 'build', *common, '--bins'], environment, timeout=1200)
    cases = [(['--test', 'terminal_local_tmux'], name) for name in LOCAL_CASES]
    if options.terminal == 'wss':
        cases.extend((['--lib'], WSS_MODULE + name) for name in WSS_CASES)
    for scope, name in cases:
        report['stage'] = 'terminal_fixture'
        report['fixture_case'] = name
        exit_code, output = execute([*cargo, 'test', *common, *scope, name, '--', '--ignored',
                                     '--exact', '--test-threads=1'], environment, timeout=1200,
                                    success=False)
        check_fixture_result(exit_code, output, report)
        report['fixture_passes'].append(name)
    del report['fixture_case']


def check_fixture_result(exit_code, output, report):
    lines = output.rstrip(b'\r\n').splitlines()
    if exit_code:
        selected_test_failed = (lines.count(b'running 1 test') == 1 and
                                LIBTEST_SINGLE_FAILURE.fullmatch(lines[-1]))
        report['fixture_failure_kind'] = (
            'selected_test_failed' if selected_test_failed else 'no_recognized_failure_summary'
        )
        if type(exit_code) is int and -128 <= exit_code <= 255:
            report['fixture_exit_code'] = exit_code
        raise Refused('operation_failed', exit_code=exit_code)
    require(lines.count(b'running 1 test') == 1 and any(line.startswith(b'test result: ok. 1 passed;')
            for line in lines), 'fixture_test_count')


def qualify(options, report):
    require(os.geteuid() != 0, 'nonroot_required')
    require(re.fullmatch('[0-9a-f]{40}', options.commit) is not None, 'source_commit')
    if options.build_installed_terminal:
        require(options.version is None and options.assets is None and options.live_relay is None
                and options.terminal == 'skip' and options.licenses is not None, 'build_mode_arguments')
    else:
        require(options.version is not None and options.assets is not None and options.licenses is None,
                'provided_asset_arguments')
        parse_release_version(options.version)
    try:
        live_relay = relay_url(options.live_relay) if options.live_relay else None
    except SmokeError as error:
        raise Refused('live_relay_invalid') from error
    mode = 'wss' if options.build_installed_terminal else options.terminal
    tmux = fixture_tmux(options.tmux, mode) if mode != 'skip' else None
    verify_checkout(options.source, options.commit)
    target = target_host()
    report.update(target=target, source_commit=options.commit, version=options.version)
    account = pwd.getpwuid(os.geteuid())
    with tempfile.TemporaryDirectory(prefix='.mq-', dir=account.pw_dir) as path, socket.socket() as guard:
        guard.bind(('127.0.0.1', 0))
        relay = live_relay or f'wss://127.0.0.1:{guard.getsockname()[1]}/'
        root = Path(path).resolve()
        if options.build_installed_terminal:
            from marmish_installed_terminal_baseline import run
            run(options, root, account, relay, tmux, report, installed_case, check_fixture_result)
        else:
            installed_case(options, root, account, relay, report)
    report['owned_state_removed'] = True
    if tmux is not None and not options.build_installed_terminal:
        terminal_cases(options, tmux, report)
    report['stage'] = 'source_recheck'
    verify_checkout(options.source, options.commit)
    report.update(stage='complete', ok=True)


def interrupt(_signal, _frame):
    raise Refused('interrupted')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', required=True, type=lambda path: Path(path).resolve())
    parser.add_argument('--commit', required=True, help='full SHA shared by checkout and staged binaries')
    parser.add_argument('--version')
    parser.add_argument('--assets', type=lambda path: Path(path).resolve())
    parser.add_argument('--build-installed-terminal', action='store_true',
                        help='build and install the exact native source for a private Terminal controls fixture')
    parser.add_argument('--licenses', type=lambda path: Path(path).absolute(),
                        help='trusted license inventory collected from the exact locked candidate')
    parser.add_argument('--terminal', choices=('skip', 'local', 'wss'), default='skip')
    parser.add_argument('--tmux', type=Path, help='absolute native tmux executable for fixture modes')
    parser.add_argument('--live-relay', help='optional explicitly authorized, production-trusted WSS relay')
    options = parser.parse_args()
    report = {'schema': 1, 'kind': 'local_native_qualification', 'ok': False, 'stage': 'source',
              'terminal_scope': options.terminal, 'fixture_passes': [], 'quickstart_diagnostics': {},
              'live_relay': 'requested' if options.live_relay else 'not_exercised',
              'owned_state_removed': False}
    if options.build_installed_terminal:
        report.update(terminal_scope='installed_controls_subset', fixture_count=0,
                      broader_terminal_qualification='unrun', observe='unrun', takeover='unrun',
                      stock_private_wss='unrun', cross_host='unrun', peer='unrun',
                      fixture_root_removed=False, fixture_tmux_cleanup='unverified',
                      owned_fixture_join='unverified')
    began = time.monotonic()
    signal.signal(signal.SIGINT, interrupt)
    signal.signal(signal.SIGTERM, interrupt)
    try:
        qualify(options, report)
    except Refused as error:
        report['failure'] = str(error)
    except Exception:
        report['failure'] = 'validation_failed'
    report['elapsed_ms'] = round((time.monotonic() - began) * 1000)
    print(json.dumps(report, sort_keys=True))
    return 0 if report['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
