"""Trusted CI coordinator for two native hosts; never accepts API credentials."""
import argparse
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import secrets
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import time
from types import SimpleNamespace

from marmish_app import Client, Profile, exit_code
from marmish_platform import continuous_time, execution_marker, process_identity
from marmish_protocol import b64, load_json, save_json, group_session, check_members
from marmish_rendezvous import RELAYS, context, validate
from mdk_smoke import SmokeError, require
from marmish_rpc import RpcError, strict_json, MdkRpc
from marmish_transport_sandbox import SandboxError


def refuse_tokens():
    from marmish_ci import refuse_api_tokens
    refuse_api_tokens()


def profile_args(config, name):
    root = Path(config['root'])
    return SimpleNamespace(identity=root / name, relay=RELAYS, wnd=Path(config['bundle']) / 'wnd',
        wnd_sha256=config['wnd_sha256'], wnd_manifest=Path(config['bundle']) / 'wnd.manifest.json')


def common(config, name):
    args = profile_args(config, name)
    result = ['-i', str(args.identity), '--wnd', str(args.wnd), '--wnd-sha256', args.wnd_sha256,
              '--wnd-manifest', str(args.wnd_manifest)]
    for relay in RELAYS:
        result += ['--relay', relay]
    return result


def subprocess_env():
    from marmish_ci_bwrap import environment
    return {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin', 'LANG': 'C', **execution_marker(),
            **environment(),
            **({key: os.environ[key] for key in ('RUNNER_TRACKING_ID',) if key in os.environ})}


def marker(config, sender):
    local, peer = config['ready'], config['peer']
    first, second = (local, peer) if sender == local['role'] else (peer, local)
    return ':'.join([local['run'], local['attempt'], local['sha'], first['role'],
                     first['nonce'], second['nonce']])


def prepare(args):
    refuse_tokens()
    bundle = args.bundle.resolve(strict=True)
    from marmish_ci import validate_bundle
    bundle_record = validate_bundle(strict_json((bundle / 'bundle.json').read_bytes()))
    hashes = bundle_record['hashes']
    require(isinstance(hashes, dict) and set(hashes) >= {'wnd', 'fixture', 'marmish-worker',
                                                     'marmish-engine-bridge'}, 'invalid_bundle')
    for filename, expected in hashes.items():
        require('/' not in filename and filename not in ('.', '..') and
                hashlib.sha256((bundle / filename).read_bytes()).hexdigest() == expected,
                'bundle_digest_mismatch')
    root = Path(tempfile.mkdtemp(prefix='mci-', dir='/tmp')).resolve()
    root.chmod(0o700)
    config = {'root': str(root), 'bundle': str(bundle), 'role': args.role,
              'wnd_sha256': hashes['wnd'], 'start': continuous_time(),
              'monotonic_start': time.monotonic(), 'wall_start': time.time(),
              'deadline': continuous_time() + 4200, 'bundle_record': bundle_record}
    require(len(os.fsencode(root / 'intruder/mdk/s')) < 104, 'socket_path_too_long')
    if getattr(args, 'clock', None):
        config.update(load_json(args.clock))
    require(continuous_time() < config['deadline'] - 120, 'overall_deadline')
    args.state.write_text(str(root))
    args.state.chmod(0o600)
    for relative in ('f/main', 'f/nested/x', 'f/deny-default', 'f/deny-key', 'f/other/x', 'f/narrowed', 'outside'):
        destination = root / relative
        destination.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
        shutil.copyfile(bundle / 'fixture', destination)
        destination.chmod(0o700)
    (root / 'cwd').mkdir(mode=0o700)
    save_json(root / 'config.json', config)
    with Profile(profile_args(config, 'server')) as server:
        server_key = server.account
    with Profile(profile_args(config, 'client')) as client:
        client_key = client.account
    with Profile(profile_args(config, 'intruder')) as intruder:
        intruder_key = intruder.account
    ready = dict(context(), version=1, role=args.role, nonce=secrets.token_hex(32),
                 server=server_key, client=client_key, intruder=intruder_key, relays=RELAYS,
                 fixture=str(root / 'f/main'), fixture_sha256=hashes['fixture'],
                 platform=sys.platform, published_at=int(time.time()), expires_at=int(time.time()) + 4200)
    config['ready'] = validate(ready, context(), args.role, 'ready')
    save_json(root / 'config.json', config)


def state(args):
    root = Path(args.state.read_text())
    require(not root.is_symlink() and root.is_dir() and root.stat().st_uid == os.geteuid() and
            root.stat().st_mode & 0o777 == 0o700, 'unsafe_interop_root')
    config = load_json(root / 'config.json')
    require(config['root'] == str(root), 'invalid_interop_state')
    return root, config


def ready(args):
    root, config = state(args)
    config['pair_deadline'] = min(config['deadline'] - 120, continuous_time() + 1200)
    save_json(root / 'config.json', config)
    args.public_output.write_text(json.dumps(config['ready'], sort_keys=True))


def policy(config):
    root = Path(config['root'])
    base = str(root / 'f')
    return {'version': 1, 'defaults': {'allow_exec': [base + '/*', base + '/nested/**'],
                                     'deny_exec': [base + '/deny-default']},
            'keys': [{'principal': config['peer']['client'],
                      'restrictions': {'allow_exec': [base + '/main', base + '/nested/**',
                                                     base + '/deny-default', base + '/deny-key']}},
                     {'principal': config['peer']['client'],
                      'restrictions': {'deny_exec': [base + '/deny-key']}}]}


def start(args):
    root, config = state(args)
    peer_role = 'macos' if config['role'] == 'linux' else 'linux'
    config['peer'] = validate(strict_json(args.peer.read_bytes()), context(), peer_role, 'ready')
    save_json(root / 'config.json', config)
    command = [sys.executable, str(Path(__file__).resolve()), 'service', '--state', str(args.state)]
    child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL, start_new_session=True, env=subprocess_env())
    save_json(root / 'supervisor.json', process_identity(child.pid))
    until = min(config['pair_deadline'], continuous_time() + 120)
    while not (root / 'paired.json').exists():
        require(child.poll() is None, 'server_readiness_failed')
        require(continuous_time() < until, 'server_readiness_timeout')
        time.sleep(.1)
    args.public_output.write_bytes((root / 'paired.json').read_bytes())


def running_rpc(root):
    home = root / 'server/mdk'
    options = {}
    if sys.platform == 'darwin':
        from marmish_transport_macos import read_running_identity
        options = {'rpc_home': str(home), 'rpc_socket': str(home / 's'),
                   'peer_identity': read_running_identity(home)}
    return MdkRpc(home / 's', timeout=10, **options)


def observe_intruder(rpc, config):
    peer, server = config['peer']['intruder'], config['ready']['server']
    for invite in rpc.invites(server):
        if invite['welcomer_account_id'] != peer:
            continue
        group_session(invite, peer)
        check_members(rpc.members(server, invite['group_id']), server, peer)
        require(invite['pending_confirmation'] is True, 'intruder_room_accepted')
        return True
    return False


def completion_matches(path, expected):
    with path.open('rb') as stream:
        value = stream.read(513)
    require(len(value) <= len(expected) and expected.startswith(value), 'invalid_peer_completion')
    return value == expected


def service(args):
    root, config = state(args)
    server = None
    inhibitor = None
    report = {'outcome': 'failed', 'cleanup': 'failed'}
    def stop(_signal, _frame):
        raise SmokeError('interrupted')
    signal.signal(signal.SIGTERM, stop)
    signal.signal(signal.SIGINT, stop)
    try:
        if sys.platform == 'darwin':
            inhibitor = subprocess.Popen(['/usr/bin/caffeinate', '-i', '-s'],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                env=subprocess_env())
        save_json(root / 'policy.json', policy(config))
        bundle = Path(config['bundle'])
        server = subprocess.Popen([sys.executable, str(Path(__file__).with_name('marmishd.py')),
            *common(config, 'server'), '--allow', config['peer']['client'], '--bridge',
            str(bundle / 'marmish-engine-bridge'), '--worker', str(bundle / 'marmish-worker'),
            '--cwd', str(root / 'cwd'), '--command-policy', str(root / 'policy.json'), '--allow-shell'],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            start_new_session=True, env=subprocess_env())
        require(select.select([server.stdout], [], [], 90)[0], 'server_readiness_timeout')
        require(json.loads(server.stdout.readline(1024)) ==
                {'ready': True, 'account': config['ready']['server']}, 'server_readiness_failed')
        paired = dict(config['ready'], peer_nonce=config['peer']['nonce'],
                      peer_server=config['peer']['server'], peer_client=config['peer']['client'], ready=True)
        save_json(root / 'paired.json', paired)
        completed_at = None
        intruder_observed = False
        next_probe = 0
        rpc = running_rpc(root)
        deadline = config['deadline'] - 120
        while continuous_time() < deadline and not (root / 'stop').exists():
            active = load_json(root / 'execution-deadline.json')
            if active:
                deadline = min(deadline, active['deadline'])
            require(server.poll() is None, 'server_exited')
            if inhibitor:
                require(inhibitor.poll() is None, 'sleep_assertion_failed')
            continuous = continuous_time() - config['start']
            require(abs(continuous - (time.monotonic() - config['monotonic_start'])) <= 5,
                    'host_suspended')
            if not intruder_observed and continuous_time() >= next_probe:
                try:
                    intruder_observed = observe_intruder(rpc, config)
                except RpcError:
                    pass
                next_probe = continuous_time() + 2
            completed = root / 'cwd/peer-completed'
            if (intruder_observed and completed.exists() and (root / 'tests-passed').exists() and
                    completion_matches(completed, marker(config, config['peer']['role']).encode())):
                completed_at = completed_at or continuous_time()
                if continuous_time() - completed_at >= 30:
                    report['outcome'] = 'passed'
                    report['peer_intruder_pending_exact_membership'] = True
                    break
            time.sleep(.2)
        require(report['outcome'] == 'passed', 'interop_deadline_or_cancelled')
    except (SmokeError, RpcError, SandboxError, OSError, ValueError, KeyError) as error:
        report['error'] = str(error) if isinstance(error, SmokeError) else 'local_service_failed'
    finally:
        if server:
            server.terminate()
            try:
                server.wait(timeout=25)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)
            server.stdout.close()
        if inhibitor:
            inhibitor.terminate()
            inhibitor.wait(timeout=5)
        # Fixed native fixtures self-terminate within 45s. Retain private state
        # unless the root/job guard proves every worker has stopped.
        from marmish_protocol import remove_engine_state
        engine_policy = {'version': 1, 'state_root': str(root / 'server/engine'),
            'worker_executable': str(Path(config['bundle']) / 'marmish-worker'),
            'server': config['ready']['server'], 'allowed_principals': [config['peer']['client']],
            'cwd': str(root / 'cwd'), 'allow_shell_command': True, 'command_policy': policy(config)}
        until = continuous_time() + 65
        while True:
            try:
                remove_engine_state(Path(config['bundle']) / 'marmish-engine-bridge', engine_policy)
                report['cleanup'] = 'complete'
                break
            except (SmokeError, OSError, ValueError):
                if continuous_time() >= until:
                    report['outcome'] = 'failed'
                    report['error'] = 'engine_cleanup_unverified'
                    break
                time.sleep(.5)
        save_json(root / 'service-result.json', report)


def command(executable, *args):
    return {'mode': 'exec', 'argv': [b64(os.fsencode(x)) for x in [executable, *args]]}


def collect(client, request):
    invocation = secrets.token_hex(16)
    invocation, _new = client.submit(request, invocation=invocation)
    client.send_input(invocation, io.BytesIO())
    out, err = io.BytesIO(), io.BytesIO()
    status = client.wait(invocation, out, err, scrub=False)
    return invocation, status, out.getvalue(), err.getvalue()


def run_tests(args):
    refuse_tokens()
    root, config = state(args)
    save_json(root / 'execution-deadline.json', {'deadline': min(config['deadline'] - 120, continuous_time() + 1200)})
    paired = validate(strict_json(args.peer.read_bytes()), context(), config['peer']['role'], 'paired')
    require(all(paired[key] == config['peer'][key] for key in config['peer']) and
            paired['peer_nonce'] == config['ready']['nonce'] and
            paired['peer_server'] == config['ready']['server'] and
            paired['peer_client'] == config['ready']['client'], 'invalid_pair_binding')
    executable = config['peer']['fixture']
    execution_deadline = min(config['deadline'] - 120, continuous_time() + 1200)
    def client_timeout():
        remaining = execution_deadline - continuous_time()
        require(remaining > 0, 'execution_deadline')
        return min(900, remaining)
    assertions = []
    with Profile(profile_args(config, 'intruder')) as intruder:
        try:
            Client(intruder, config['peer']['server'], timeout=20)
            raise SmokeError('unlisted_identity_accepted')
        except SmokeError as error:
            require(str(error) == 'client_timeout', 'unlisted_identity_wrong_error')
    assertions.append('unlisted_nostr_identity_cannot_open_command_session')
    special = [b'with spaces', b'', b"'double\"quote", b'$HOME', b'$(id)', b'`id`',
               b'line\nline', b'\\backslash', 'marmotte 🦫'.encode(), b'\xff']
    with Profile(profile_args(config, 'client')) as profile:
        client = Client(profile, config['peer']['server'], timeout=client_timeout())
        original_group = client.group
        _, status, out, err = collect(client, command(executable, 'argv', *special))
        require(out == b''.join(struct.pack('!I', len(x)) + x for x in special) and
                err == b'stderr\0\xff' and exit_code(status) == 7, 'argv_or_exit_mismatch')
        assertions.append('byte_exact_argv_stderr_exit')
        binary, _ = client.submit(command(executable, 'binary'))
        data = bytes(range(256)) * 2
        client.send_input(binary, io.BytesIO(data))
        out, err = io.BytesIO(), io.BytesIO()
        status = client.wait(binary, out, err)
        require(out.getvalue() == data and err.getvalue() == data[::-1] and
                exit_code(status) == 0, 'binary_stream_mismatch')
        assertions.append('binary_stdin_stdout_stderr_eof')
        base = str(Path(executable).parent)
        for path in [base + '/deny-default', base + '/deny-key', base + '/other/x', base + '/narrowed',
                     str(Path(base).parent / 'outside'), base + '/../f/main', 'main']:
            try:
                client.submit(command(path, 'count'))
                raise SmokeError('policy_denial_missing')
            except SmokeError as error:
                require(str(error) == 'command_denied', 'policy_denial_mismatch')
        try:
            client.submit({'mode': 'shell', 'command': b64(b'exit 0')})
            raise SmokeError('shell_denial_missing')
        except SmokeError as error:
            require(str(error) == 'command_denied', 'shell_denial_mismatch')
        _, nested_status, _, _ = collect(client, command(base + '/nested/x', 'noop'))
        require(exit_code(nested_status) == 0, 'double_star_allow_failed')
        assertions.append('additive_policy_denials_and_star_double_star')
        counted, _, out, _ = collect(client, command(executable, 'count'))
        require(out == b'once\n', 'denial_side_effect_or_count_failed')
        client.retry(counted)
        out, err = io.BytesIO(), io.BytesIO()
        require(exit_code(client.wait(counted, out, err)) == 0 and out.getvalue() == b'once\n',
                'retry_duplicate_execution')
        token = client.call('token')['token']
        try:
            client.call('submit', invocation=counted, token=token, command=command(executable, 'noop'))
            raise SmokeError('conflict_accepted')
        except SmokeError as error:
            require(str(error) == 'invocation_conflict', 'conflict_mismatch')
        assertions.append('retry_executes_once_conflicting_content_denied')
        detached, _ = client.submit(command(executable, 'sleep', '12'))
        client.send_input(detached, io.BytesIO())
    with Profile(profile_args(config, 'client')) as profile:
        client = Client(profile, config['peer']['server'], timeout=client_timeout())
        require(client.group == original_group, 'session_not_resumed')
        out, err = io.BytesIO(), io.BytesIO()
        require(exit_code(client.wait(detached, out, err)) == 0 and
                out.getvalue() == b'survived\n', 'disconnect_recovery_failed')
        fresh = Client(profile, config['peer']['server'], timeout=client_timeout(), new_session=True)
        require(fresh.group != original_group and fresh.call('status', invocation=detached)['state'] == 'exited',
                'fresh_group_or_ownership_failed')
        assertions.append('disconnect_recovery_mls_resume_fresh_group_ownership')
    target = config['peer']['server'] + '@relay.eu.whitenoise.chat'
    cli_timeout = max(1, int(min(120, client_timeout())))
    cli = subprocess.run([sys.executable, str(Path(__file__).with_name('marmish.py')),
        *common(config, 'client'), '--timeout', str(cli_timeout), target, '--', executable, 'noop'],
        input=b'', stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=min(150, client_timeout()), env=subprocess_env())
    require(cli.returncode == 0 and cli.stdout == b'' and cli.stderr == b'', 'cli_interop_failed')
    assertions.append('actual_cli_process_cross_host_execution')
    with Profile(profile_args(config, 'client')) as profile:
        final = Client(profile, config['peer']['server'], timeout=client_timeout())
        _, status, out, _ = collect(final, command(executable, 'marker', marker(config, config['role'])))
        require(exit_code(status) == 0 and out == b'marked\n', 'peer_marker_failed')
    save_json(root / 'tests-passed', {'assertions': assertions})
    while not (root / 'service-result.json').exists():
        require(continuous_time() < config['deadline'], 'completion_timeout')
        time.sleep(.5)
    result = load_json(root / 'service-result.json')
    require(result['outcome'] == 'passed' and result['cleanup'] == 'complete', 'service_cleanup_failed')
    result.update(context(), direction=config['role'] + '-client-to-' + config['peer']['role'] + '-server',
                  assertions=assertions, relays=RELAYS, platform=sys.platform,
                  wnd_sha256=config['wnd_sha256'],
                  build=config['bundle_record'], fixture_sha256=config['ready']['fixture_sha256'],
                  wnd_manifest=strict_json((Path(config['bundle']) / 'wnd.manifest.json').read_bytes()),
                  elapsed_seconds=round(continuous_time() - config['start'], 3),
                  peer_wall_clock_skew_seconds=round(time.time() - config['peer']['published_at']))
    from marmish_ci_bwrap import provenance
    result['bubblewrap'] = provenance()
    args.output.write_text(json.dumps(result, sort_keys=True))
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as summary:
            summary.write('Marmot **' + result['direction'] + '**: passed; ' +
                          str(len(assertions)) + ' assertions; cleanup complete; commit `' +
                          result['sha'] + '`; run/attempt `' + result['run'] + '/' + result['attempt'] + '`.\n')


def cleanup(args):
    if not args.state.exists():
        return
    root, _config = state(args)
    if not (root / 'supervisor.json').exists():
        require(not (root / 'server/engine').exists(), 'cleanup_unverified_state_retained')
        cleanup_controls(root)
        shutil.rmtree(root)
        args.state.unlink()
        return
    (root / 'stop').touch(mode=0o600)
    until = continuous_time() + 120
    while (root / 'supervisor.json').exists() and not (root / 'service-result.json').exists():
        require(continuous_time() < until, 'cleanup_timeout')
        time.sleep(.2)
    result = load_json(root / 'service-result.json', {})
    require(result.get('cleanup') == 'complete', 'cleanup_unverified_state_retained')
    cleanup_controls(root)
    shutil.rmtree(root)
    args.state.unlink()


def cleanup_controls(root):
    if sys.platform == 'darwin':
        from marmish_transport_macos import cleanup_control, control_dir
        for name in ('server', 'client', 'intruder'):
            home = root / name / 'mdk'
            if os.path.lexists(control_dir(home)):
                cleanup_control(home)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('prepare', 'ready', 'start', 'service', 'run', 'cleanup'))
    parser.add_argument('--state', type=Path, required=True)
    parser.add_argument('--bundle', type=Path)
    parser.add_argument('--clock', type=Path)
    parser.add_argument('--role', choices=('linux', 'macos'))
    parser.add_argument('--peer', type=Path)
    parser.add_argument('--public-output', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    os.umask(0o077)
    try:
        {'prepare': prepare, 'ready': ready, 'start': start, 'service': service,
         'run': run_tests, 'cleanup': cleanup}[args.operation](args)
        return 0
    except (SmokeError, RpcError, SandboxError, OSError, ValueError, KeyError, TypeError,
            RuntimeError, subprocess.TimeoutExpired) as error:
        category = str(error) if isinstance(error, SmokeError) else 'local_interop_failed'
        print(json.dumps({'outcome': 'failed', 'error': category}))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
