#!/usr/bin/env python3
"""CI-only PTY driver for the real POSIX bootstrap and installed native setup."""
import argparse
import errno
import json
import os
from pathlib import Path
import pty
import pwd
import select
import shutil
import signal
import subprocess
import tempfile
import time


UTILITIES = ('curl', 'uname', 'id', 'mktemp', 'chmod', 'mkdir', 'rm', 'rmdir',
             'mv', 'cat', 'wc', 'tr', 'awk', 'sh', 'sha256sum', 'shasum', 'sysctl')
DIALOG = (
    (b'Role [c]lient/[s]erver/[b]oth [b]: ', b'\n'),
    (b'Explicit relay URL(s), separated by spaces: ', b'wss://relay.invalid/\n'),
    (b'Tor auto/always/off [auto, experimental]: ', b'off\n'),
    (b'Npub(s) allowed to execute here (empty disables inbound): ', b'\n'),
    (b'Trusted outbound server npub(s), independently (empty allowed): ', b'\n'),
)


def interactive(arguments, environment):
    child, master = pty.fork()
    if child == 0:
        os.execve('/bin/sh', ['/bin/sh', *arguments], environment)
    output = bytearray()
    next_prompt = 0
    status = None
    deadline = time.monotonic() + 240
    try:
        while status is None:
            if time.monotonic() >= deadline or len(output) > 2 * 1024 * 1024:
                raise RuntimeError('quickstart_observation_limit')
            readable, _, _ = select.select([master], [], [], 0.1)
            if readable:
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b''
                output.extend(data)
            while next_prompt < len(DIALOG) and DIALOG[next_prompt][0] in output:
                os.write(master, DIALOG[next_prompt][1])
                next_prompt += 1
            observed, current = os.waitpid(child, os.WNOHANG)
            if observed == child:
                status = current
                # Reap only our child, then drain its already-buffered terminal output.
                while select.select([master], [], [], 0)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not data:
                        break
                    output.extend(data)
                    if len(output) > 2 * 1024 * 1024:
                        raise RuntimeError('quickstart_observation_limit')
        if not os.WIFEXITED(status) or os.WEXITSTATUS(status) != 0 or next_prompt != len(DIALOG):
            raise RuntimeError('quickstart_terminal_failure')
        return bytes(output)
    finally:
        if status is None:
            # The PID/session was created here, never loaded from a file or global search.
            try:
                os.killpg(child, signal.SIGTERM)
            except ProcessLookupError:
                pass
            until = time.monotonic() + 2
            while time.monotonic() < until:
                observed, _ = os.waitpid(child, os.WNOHANG)
                if observed == child:
                    break
                time.sleep(0.05)
            else:
                try:
                    os.killpg(child, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(child, 0)
        os.close(master)


def reports(output):
    found = []
    for line in output.decode('utf-8').splitlines():
        if line.startswith('{'):
            found.append(json.loads(line))
    return found


def run(bootstrap, assets, report):
    account = pwd.getpwuid(os.geteuid())
    root = Path(tempfile.mkdtemp(prefix='.marmish-quickstart-check-', dir=account.pw_dir))
    root.chmod(0o700)
    completed = False
    result = {'schema': 1, 'kind': 'installed_quickstart_pty', 'ok': False,
              'key_storage': 'file', 'network_activation': False,
              'python_in_runtime_path': False, 'owned_state_removed': False}
    try:
        utilities = root / 'utilities'
        utilities.mkdir(mode=0o700)
        for name in UTILITIES:
            executable = shutil.which(name)
            if executable:
                (utilities / name).symlink_to(executable)
        environment = {'HOME': account.pw_dir, 'USER': account.pw_name,
                       'LOGNAME': account.pw_name, 'PATH': str(utilities),
                       'LC_ALL': 'C', 'TMPDIR': str(root)}
        state, prefix, bins = (root / name for name in ('state', 'prefix', 'bin'))
        begin = time.monotonic()
        output = interactive([str(bootstrap), '--quickstart', '--key-storage', 'file',
            '--debug', '--asset-dir', str(assets), '--state-dir', str(state),
            '--prefix', str(prefix), '--bin-dir', str(bins)], environment)
        initial = [value for value in reports(output) if 'identity_created' in value]
        if len(initial) != 1:
            raise RuntimeError('quickstart_missing_setup_report')
        initial = initial[0]
        if not (initial['identity_created'] and initial['owner_opened']
                and initial['role'] == 'both' and not initial['inbound_enabled']
                and initial['backend'] == 'file'
                and all(initial[key] for key in ('engine_stop_complete',
                    'mdk_stop_complete', 'runtime_cleanup_complete'))):
            raise RuntimeError('quickstart_setup_contract')
        repeated = subprocess.run([str(bins / 'marmish'), '--state-dir', str(state),
            '--key-storage', 'file', 'setup', '--no-grants'], env=environment,
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            timeout=45, check=True)
        repeated = json.loads(repeated.stdout)
        if not (repeated['npub'] == initial['npub'] and not repeated['owner_opened']
                and not repeated['identity_created'] and not repeated['config_changed']
                and all(repeated[key] for key in ('engine_stop_complete',
                    'mdk_stop_complete', 'runtime_cleanup_complete'))):
            raise RuntimeError('quickstart_repeat_contract')
        result.update(ok=True, role='both', inbound_enabled=False,
            prompts_completed=len(DIALOG), identity_created=True,
            repeat_same_identity=True, repeat_owner_opened=False,
            stop_complete=True, elapsed_ms=round((time.monotonic() - begin) * 1000))
        completed = True
    except Exception:
        # Never publish raw terminal output, provider errors, keys or private state.
        result['failure'] = 'quickstart_validation_failed'
    finally:
        if completed:
            shutil.rmtree(root)
            result['owned_state_removed'] = not root.exists()
        report.write_text(json.dumps(result, indent=2) + '\n')
    return 0 if result['ok'] and result['owned_state_removed'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bootstrap', required=True, type=Path)
    parser.add_argument('--assets-dir', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    options = parser.parse_args()
    raise SystemExit(run(options.bootstrap.resolve(), options.assets_dir.resolve(),
                         options.report.resolve()))
