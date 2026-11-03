#!/usr/bin/env python3
"""CI-only qualification of installed Auto custody in disposable OS contexts.

Never uploads terminal/provider output, key references or private paths. This
supervisor is build tooling; no Python is placed on product children's PATH.
"""
import argparse
import hashlib
import html
import importlib.util
import json
import os
from pathlib import Path
import platform
import pwd
import re
import selectors
import shutil
import signal
import stat
import subprocess
import tempfile
import time
import uuid

_quickstart = importlib.util.spec_from_file_location('marmish_quickstart_fixture',
    Path(__file__).with_name('marmish_native_quickstart_check.py'))
_fixture = importlib.util.module_from_spec(_quickstart)
_quickstart.loader.exec_module(_fixture)
UTILITIES, interactive, reports = _fixture.UTILITIES, _fixture.interactive, _fixture.reports


PASSWORD = b'marmish-disposable-provider-check\n'
MAX_CAPTURE = 2 * 1024 * 1024


class Refused(Exception):
    """Finite public category, never a provider's raw error string."""


def require(condition, category):
    if not condition:
        raise Refused(category)


def stop(child):
    exited = child.poll() is not None
    try:
        os.killpg(child.pid, 0)
    except ProcessLookupError:
        child.wait(timeout=3)
        return
    if child.poll() is None:
        os.killpg(child.pid, signal.SIGTERM)
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=3)
    # A completed leader is not proof that all of its descendants exited.
    try:
        os.killpg(child.pid, signal.SIGTERM)
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            try:
                os.killpg(child.pid, 0)
            except ProcessLookupError:
                require(not exited, 'owned_descendant_remained')
                return
            time.sleep(0.05)
        os.killpg(child.pid, signal.SIGKILL)
        raise Refused('child_cleanup_incomplete')
    except ProcessLookupError:
        require(not exited, 'owned_descendant_remained')
    require(child.poll() is not None, 'child_cleanup_incomplete')


def execute(argv, environment, timeout=15, data=None, success=True):
    require(data is None or len(data) <= 4096, 'operation_input_limit')
    child = subprocess.Popen(argv, env=environment, stdin=subprocess.PIPE if data is not None
        else subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True)
    try:
        if data is not None:
            child.stdin.write(data)
            child.stdin.close()
        out, total = bytearray(), 0
        deadline = time.monotonic() + timeout
        with selectors.DefaultSelector() as selector:
            for stream in (child.stdout, child.stderr):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                require(remaining > 0, 'operation_timeout')
                for key, _ in selector.select(min(remaining, 0.1)):
                    raw = os.read(key.fileobj.fileno(), 65536)
                    if not raw:
                        selector.unregister(key.fileobj)
                        continue
                    total += len(raw)
                    require(total <= MAX_CAPTURE, 'operation_output_limit')
                    if key.fileobj is child.stdout:
                        out.extend(raw)
        child.wait(timeout=max(0.001, deadline - time.monotonic()))
        require(not success or child.returncode == 0, 'operation_failed')
        return child.returncode, bytes(out)
    except subprocess.TimeoutExpired:
        raise Refused('operation_timeout') from None
    finally:
        try:
            stop(child)
        finally:
            child.stdout.close()
            child.stderr.close()


def private(directory):
    directory.mkdir(mode=0o700)
    require(stat.S_IMODE(directory.stat().st_mode) == 0o700, 'private_mode')
    return directory


def digest(path):
    require(path.is_file() and not path.is_symlink(), 'unsafe_input')
    require(path.stat().st_size <= 256 * 1024 * 1024, 'input_size')
    return hashlib.sha256(path.read_bytes()).hexdigest()


class LinuxProvider:
    def __init__(self, root, environment):
        self.root = root
        self.environment = dict(environment)
        self.children = []
        self.keyring = None

    def bus(self, name):
        directory = private(self.root / name)
        config = directory / 'bus.conf'
        path = directory / 'session-bus'
        config.write_text('<busconfig><type>session</type><listen>unix:path=' + html.escape(str(path)) +
            '</listen><auth>EXTERNAL</auth><policy context="default">'
            '<allow send_destination="*"/><allow receive_sender="*"/>'
            '<allow own="*"/></policy></busconfig>')
        child = subprocess.Popen(['/usr/bin/dbus-daemon', '--nofork', '--config-file',
            str(config)], env=self.environment, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        self.children.append(child)
        deadline = time.monotonic() + 10
        while not path.exists():
            require(child.poll() is None and time.monotonic() < deadline, 'private_bus_start')
            time.sleep(0.05)
        info = path.lstat()
        require(stat.S_ISSOCK(info.st_mode) and info.st_uid == os.geteuid(), 'private_bus_socket')
        address = 'unix:path=' + str(path)
        require(';' not in address and path.parent == directory, 'private_bus_address')
        return address

    def start(self):
        for binary in ('/usr/bin/dbus-daemon', '/usr/bin/dbus-send', '/usr/bin/gnome-keyring-daemon'):
            require(Path(binary).is_file(), 'provider_prerequisite')
        for name in ('home', 'run', 'data', 'config', 'cache', 'control'):
            private(self.root / name)
        self.environment.update(HOME=str(self.root / 'home'),
            XDG_RUNTIME_DIR=str(self.root / 'run'), XDG_DATA_HOME=str(self.root / 'data'),
            XDG_CONFIG_HOME=str(self.root / 'config'), XDG_CACHE_HOME=str(self.root / 'cache'))
        self.environment['DBUS_SESSION_BUS_ADDRESS'] = self.bus('active-bus')
        self.absent = self.bus('absent-bus')
        self.unlock()
        return self.environment

    def restart(self, unlock):
        if self.keyring:
            stop(self.keyring)
            self.wait_service(False)
        argv = ['/usr/bin/gnome-keyring-daemon', '--foreground', '--components=secrets',
            '--control-directory', str(self.root / 'control')]
        if unlock:
            argv.append('--unlock')
        self.keyring = subprocess.Popen(argv, env=self.environment,
            stdin=subprocess.PIPE if unlock else subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        self.children.append(self.keyring)
        if unlock:
            self.keyring.stdin.write(PASSWORD)
            self.keyring.stdin.close()
        self.wait_service(True)

    def wait_service(self, present):
        deadline = time.monotonic() + 15
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'owned_provider_service_state')
            _, raw = execute(['/usr/bin/dbus-send', '--bus=' + self.environment['DBUS_SESSION_BUS_ADDRESS'],
                '--print-reply', '--dest=org.freedesktop.DBus', '/org/freedesktop/DBus',
                'org.freedesktop.DBus.NameHasOwner', 'string:org.freedesktop.secrets'],
                self.environment, min(2, remaining))
            expected = b'boolean true' if present else b'boolean false'
            if expected in raw:
                return
            time.sleep(0.05)

    def lock(self):
        self.restart(False)

    def unlock(self):
        self.restart(True)

    def close(self):
        for child in reversed(self.children):
            stop(child)


class MacProvider:
    def __init__(self, root, environment):
        self.root, self.environment = root, environment
        self.keychain = root / 'provider.keychain-db'

    def security(self, *arguments):
        return execute(['/usr/bin/security', *arguments], self.environment)[1]

    def start(self):
        password = PASSWORD.decode().strip()
        self.security('create-keychain', '-p', password, str(self.keychain))
        self.security('default-keychain', '-d', 'user', '-s', str(self.keychain))
        self.security('list-keychains', '-d', 'user', '-s', str(self.keychain))
        self.unlock()
        self.security('set-keychain-settings', str(self.keychain))
        selected = self.security('default-keychain', '-d', 'user').decode().strip().strip('"')
        require(Path(selected) == self.keychain, 'owned_default_keychain')
        return self.environment

    def lock(self):
        self.security('lock-keychain', str(self.keychain))

    def unlock(self):
        self.security('unlock-keychain', '-p', PASSWORD.decode().strip(), str(self.keychain))

    def close(self):
        self.security('delete-keychain', str(self.keychain))
        require(not self.keychain.exists(), 'keychain_cleanup_incomplete')


class Qualification:
    def __init__(self, root, bootstrap, assets):
        self.root, self.bootstrap, self.assets = root, bootstrap, assets
        self.environment = {'HOME': pwd.getpwuid(os.geteuid()).pw_dir,
            'USER': pwd.getpwuid(os.geteuid()).pw_name,
            'LOGNAME': pwd.getpwuid(os.geteuid()).pw_name, 'LC_ALL': 'C', 'TMPDIR': str(root)}
        utilities = private(root / 'utilities')
        for name in UTILITIES:
            executable = shutil.which(name)
            if executable:
                (utilities / name).symlink_to(executable)
        self.environment['PATH'] = str(utilities)
        self.provider = MacProvider(root, self.environment) if platform.system() == 'Darwin' else LinuxProvider(root, self.environment)
        self.state, self.prefix, self.bins = (root / name for name in ('state', 'prefix', 'bin'))

    def command(self, name, *arguments, success=True, environment=None):
        return execute([str(self.bins / 'marmish'), '--state-dir', str(self.state),
            '-i', name, *arguments], environment or self.environment, 120, success=success)

    def helper(self, action, environment=None):
        require(action in ('probe', 'list'), 'forbidden_helper_operation')
        code, raw = execute([str(self.bins / 'marmish'), '_vault-helper'],
            environment or self.environment, 15,
            json.dumps({'version': 1, 'action': action, 'namespace': 'marmish-v2'}).encode(), False)
        require(len(raw) <= 131072, 'provider_metadata_limit')
        value = json.loads(raw)
        require(value.get('version') == 1 and value.get('ok') == (code == 0), 'helper_contract')
        return value

    def wait_probe(self, category=None):
        deadline = time.monotonic() + 15
        while True:
            value = self.helper('probe')
            if value.get('ok') if category is None else value.get('error') == category:
                return
            require(time.monotonic() < deadline, 'provider_state_unavailable')
            time.sleep(0.1)

    def manifest(self, name):
        path = self.state / name / 'profile.json'
        require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 8192, 'profile_metadata')
        value = json.loads(path.read_bytes())
        require(value['backend'] == 'keychain' and value['phase'] == 'ready', 'profile_custody')
        require(not (path.parent / 'keys').exists(), 'plaintext_fallback')
        require(value['nostr_ref'] != value['sqlcipher_ref'], 'separate_key_references')
        return value

    def reload(self, name, public):
        _, raw = self.command(name, 'init')
        value = json.loads(raw)
        # owner status is public metadata, not a provider-load response.
        require(value.get('identity') == public, 'fresh_identity_reload')

    def items(self, expected):
        value = self.helper('list')
        require(value.get('ok'), 'provider_list')
        items = value['result']['items']
        require(len(items) == expected, 'provider_item_count')
        references = {(item['reference']['purpose'], item['reference']['id']) for item in items}
        require(len(references) == expected, 'provider_item_uniqueness')
        return references

    def run(self, result):
        self.environment = self.provider.start()
        # Install without opening an identity first, so the real installed helper
        # can prove provider readiness before the interactive Auto setup starts.
        execute(['/bin/sh', str(self.bootstrap), '--asset-dir', str(self.assets),
            '--prefix', str(self.prefix), '--bin-dir', str(self.bins)], self.environment, 120)
        self.wait_probe()
        output = interactive([str(self.bootstrap), '--quickstart', '--debug', '--asset-dir',
            str(self.assets), '--state-dir', str(self.state), '--prefix', str(self.prefix),
            '--bin-dir', str(self.bins)], self.environment, timeout=120)
        initial = [value for value in reports(output) if 'identity_created' in value]
        require(len(initial) == 1, 'quickstart_report')
        initial = initial[0]
        require(initial['backend'] == 'keychain' and initial['role'] == 'both'
            and initial['identity_created'] and initial['owner_opened'] and not initial['inbound_enabled']
            and all(initial[key] for key in ('engine_stop_complete', 'mdk_stop_complete',
                'runtime_cleanup_complete')), 'quickstart_auto_keychain')
        first = self.manifest('default')
        self.items(2)
        self.reload('default', first['public_key'])
        _, raw = self.command('second', '--relay', 'wss://relay.invalid/', 'setup', '--role', 'both', '--no-grants')
        second_setup = json.loads(raw)
        second = self.manifest('second')
        require(second_setup['npub'] != initial['npub'], 'named_identity_separation')
        references = self.items(4)
        require(len({first['nostr_ref'], first['sqlcipher_ref'], second['nostr_ref'], second['sqlcipher_ref']}) == 4,
            'named_reference_separation')
        self.reload('default', first['public_key'])
        self.reload('second', second['public_key'])
        result.update(auto_keychain=True, distinct_named_identities=True, fresh_reload=True, item_count=4)
        before = {name: self.snapshot(name) for name in ('default', 'second')}
        self.provider.lock()
        self.wait_probe('locked_or_denied')
        require(self.command('default', 'init', success=False)[0] != 0, 'locked_existing_accepted')
        require(self.command('locked-new', '--relay', 'wss://relay.invalid/', 'setup', '--role', 'both',
            '--no-grants', success=False)[0] != 0, 'locked_fresh_accepted')
        require(all(before[name] == self.snapshot(name) for name in before), 'locked_profile_mutated')
        require(all(not (self.state / name / 'keys').exists() for name in before), 'locked_files_fallback')
        require(not (self.state / 'locked-new' / 'keys').exists(), 'locked_files_fallback')
        self.provider.unlock()
        self.wait_probe()
        self.reload('default', first['public_key'])
        self.reload('second', second['public_key'])
        require(self.items(4) == references, 'locked_native_items_changed')
        result.update(locked_refused=True, unlocked_fresh_reload=True, no_plaintext_fallback=True)
        if isinstance(self.provider, LinuxProvider):
            absent = dict(self.environment, DBUS_SESSION_BUS_ADDRESS=self.provider.absent)
            require(self.helper('probe', absent).get('error') == 'unavailable', 'absent_provider_category')
            require(self.command('default', 'init', success=False, environment=absent)[0] != 0,
                'absent_existing_accepted')
            require(before['default'] == self.snapshot('default'), 'absent_profile_mutated')
            require(all(not (self.state / name / 'keys').exists() for name in before), 'absent_existing_files_fallback')
            _, raw = self.command('absent-new', '--relay', 'wss://relay.invalid/', 'setup', '--role', 'both',
                '--no-grants', environment=absent)
            created = json.loads(raw)
            require(created['backend'] == 'file' and created['identity_created']
                and (self.state / 'absent-new' / 'keys').is_dir(),
                'absent_auto_files')
            result['absent_provider_files'] = True

    def snapshot(self, name):
        directory = self.state / name
        return tuple((directory / filename).read_bytes() for filename in ('profile.json', 'config.json'))


def inner(bootstrap, assets, report, pins=None, deadline_seconds=540):
    if pins:
        staging = pins.parent
        values = json.loads(pins.read_bytes())
        require(len(values) <= 16, 'staged_input_count')
        for name, expected in values.items():
            path = staging / name
            require(path.resolve().is_relative_to(staging) and digest(path) == expected,
                'staged_input_changed')
    account = pwd.getpwuid(os.geteuid())
    root = Path(tempfile.mkdtemp(prefix='.marmish-provider-', dir=account.pw_dir))
    root.chmod(0o700)
    qualification = Qualification(root, bootstrap, assets)
    def expired(_signum, _frame):
        raise Refused('qualification_deadline')
    signal.signal(signal.SIGALRM, expired)
    signal.signal(signal.SIGTERM, expired)
    require(1 <= deadline_seconds <= 540, 'qualification_budget_invalid')
    signal.alarm(deadline_seconds)
    result = {'schema': 1, 'kind': 'installed_auto_provider', 'ok': False,
        'python_in_runtime_path': False, 'network_activation': False,
        'owned_children_reaped': False, 'owned_state_removed': False}
    started = time.monotonic()
    try:
        qualification.run(result)
        result['ok'] = True
    except Refused as error:
        result['failure'] = str(error)
    except Exception:
        result['failure'] = 'provider_qualification_failed'
    finally:
        try:
            qualification.provider.close()
            result['owned_children_reaped'] = True
        except Exception:
            result.update(ok=False, failure='provider_cleanup_incomplete')
        if result['ok'] and result['owned_children_reaped']:
            shutil.rmtree(root)
            result['owned_state_removed'] = not root.exists()
        signal.alarm(0)
        result['elapsed_ms'] = round((time.monotonic() - started) * 1000)
        report.write_text(json.dumps(result, indent=2) + '\n')
    return 0 if result['ok'] and result['owned_state_removed'] else 1


def mac_account(bootstrap, assets, report):
    """Outer privilege is restricted to staging and one newly owned OS account."""
    environment = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin',
        'HOME': pwd.getpwuid(os.geteuid()).pw_dir, 'LC_ALL': 'C'}
    result = {'schema': 1, 'kind': 'installed_auto_provider', 'ok': False,
        'owned_account_removed': False, 'runner_keychain_unchanged': False}
    started = time.monotonic()
    def cleanup_execute(argv, success=True):
        remaining = 600 - (time.monotonic() - started)
        require(remaining > 0, 'owned_account_cleanup_deadline')
        return execute(argv, environment, min(5, remaining), success=success)
    name, uid, home, created = 'marmishq' + uuid.uuid4().hex[:10], None, None, False
    runner = []
    try:
        for command in ('default-keychain', 'list-keychains'):
            runner.append(execute(['/usr/bin/security', command, '-d', 'user'], environment)[1])
        _, raw = execute(['/usr/bin/dscl', '.', '-list', '/Users', 'UniqueID'], environment)
        used = {int(line.split()[-1]) for line in raw.decode().splitlines()}
        uid = next((number for number in range(55000, 55100) if number not in used), None)
        require(uid is not None, 'owned_account_uid_exhausted')
        try:
            pwd.getpwnam(name)
            raise Refused('owned_account_already_exists')
        except KeyError:
            pass
        home = Path('/Users') / name
        require(not home.exists() and not home.is_symlink(), 'owned_home_already_exists')
        account_deadline = time.monotonic() + 60
        def administrative(*arguments):
            remaining = account_deadline - time.monotonic()
            require(remaining > 0, 'owned_account_setup_deadline')
            return execute(['/usr/bin/sudo', '-n', '/usr/bin/dscl', '.', *arguments],
                environment, min(15, remaining))
        administrative('-create', '/Users/' + name)
        created = True
        for key, value in (('UniqueID', str(uid)), ('PrimaryGroupID', '-2'),
                ('NFSHomeDirectory', str(home)), ('UserShell', '/usr/bin/false'),
                ('RealName', 'Disposable Marmish CI'), ('Password', '*'),
                ('AuthenticationAuthority', ';DisabledUser;')):
            administrative('-create', '/Users/' + name, key, value)
        account = pwd.getpwnam(name)
        require(account.pw_uid == uid and account.pw_dir == str(home), 'owned_account_identity')
        execute(['/usr/bin/sudo', '-n', '/bin/mkdir', '-m', '700', str(home)], environment)
        execute(['/usr/bin/sudo', '-n', '/usr/sbin/chown', str(uid) + ':-2', str(home)], environment)
        # Stage outside the runner's private HOME; rehash every copied input.
        stage = Path(tempfile.mkdtemp(prefix='marmish-provider-stage-'))
        stage.chmod(0o755)
        try:
            names = [Path(__file__), Path(__file__).with_name('marmish_native_quickstart_check.py'), bootstrap]
            pins = {}
            for source in names:
                expected = digest(source)
                shutil.copy2(source, stage / source.name)
                (stage / source.name).chmod(0o644)
                require(digest(stage / source.name) == expected, 'staged_input_changed')
                pins[source.name] = expected
            staged_assets = stage / 'assets'
            staged_assets.mkdir(mode=0o755)
            for source in assets.iterdir():
                expected = digest(source)
                shutil.copy2(source, staged_assets / source.name)
                (staged_assets / source.name).chmod(0o644)
                require(digest(staged_assets / source.name) == expected, 'staged_asset_changed')
                pins['assets/' + source.name] = expected
            (stage / 'input-pins.json').write_text(json.dumps(pins))
            (stage / 'input-pins.json').chmod(0o644)
            execute(['/usr/bin/sudo', '-n', '/bin/cp', '-R', str(stage), str(home / 'stage')], environment)
            # Read/execute staging only, writable state is created by the new UID.
            execute(['/usr/bin/sudo', '-n', '/bin/chmod', '-R', 'a+rX', str(home / 'stage')], environment)
        finally:
            shutil.rmtree(stage)
        inner_report = home / 'provider-result.json'
        _, raw = execute(['/usr/bin/xcrun', '--find', 'python3'], environment)
        python = Path(raw.decode().strip()).resolve()
        require(python.is_file() and not python.is_relative_to(Path(environment['HOME'])),
            'owned_account_interpreter')
        interpreter_digest = digest(python)
        remaining = 570 - (time.monotonic() - started)
        require(remaining > 30, 'owned_account_setup_deadline')
        inner_budget = min(540, int(remaining - 20))
        argv = ['/usr/bin/sudo', '-n', '/bin/launchctl', 'asuser', str(uid),
            '/usr/bin/sudo', '-n', '-H', '-u', name, '/usr/bin/env', '-i',
            'HOME=' + str(home), 'USER=' + name, 'LOGNAME=' + name, 'PATH=/usr/bin:/bin',
            str(python), '-I', str(home / 'stage' / Path(__file__).name), '--inner',
            '--bootstrap', str(home / 'stage' / bootstrap.name),
            '--assets-dir', str(home / 'stage/assets'), '--report', str(inner_report),
            '--pins', str(home / 'stage/input-pins.json'), '--deadline-seconds', str(inner_budget)]
        code, _ = execute(argv, environment, remaining, success=False)
        _, raw = execute(['/usr/bin/sudo', '-n', '/bin/cat', str(inner_report)], environment)
        require(len(raw) <= 16384, 'owned_account_report_limit')
        received = json.loads(raw)
        require(received.get('kind') == 'installed_auto_provider', 'owned_account_report')
        for key in ('ok', 'failure', 'auto_keychain', 'distinct_named_identities', 'fresh_reload',
                'item_count', 'locked_refused', 'unlocked_fresh_reload', 'no_plaintext_fallback',
                'python_in_runtime_path', 'network_activation', 'owned_children_reaped', 'owned_state_removed'):
            if key in received:
                result[key] = received[key]
        result['supervisor_interpreter_unchanged'] = digest(python) == interpreter_digest
        require(result['supervisor_interpreter_unchanged'], 'interpreter_changed')
        require(code == 0 and result['ok'], 'owned_account_qualification_failed')
    except Refused as error:
        result.update(ok=False, failure=str(error))
    except Exception:
        result.update(ok=False, failure='owned_account_prerequisite_failed')
    finally:
        if created and uid is not None:
            try:
                # Never kill by process name: this freshly allocated UID is the scope.
                cleanup_execute(['/usr/bin/sudo', '-n', '/bin/launchctl', 'bootout', 'user/' + str(uid)],
                    success=False)
                _, raw = cleanup_execute(['/bin/ps', '-u', str(uid), '-o', 'pid='], success=False)
                require(not raw.strip(), 'owned_account_children_remain')
                if result['ok']:
                    account = pwd.getpwnam(name)
                    require(account.pw_uid == uid and account.pw_dir == str(home)
                        and home.lstat().st_uid == uid and not home.is_symlink(), 'owned_account_cleanup_identity')
                    cleanup_execute(['/usr/bin/sudo', '-n', '/usr/bin/dscl', '.', '-delete', '/Users/' + name])
                    cleanup_execute(['/usr/bin/sudo', '-n', '/bin/rm', '-rf', str(home)])
                    require(not home.exists(), 'owned_account_home_remains')
                    result['owned_account_removed'] = True
            except Exception:
                result.update(ok=False, failure='owned_account_cleanup_incomplete')
        try:
            after = [cleanup_execute(['/usr/bin/security', command, '-d', 'user'])[1]
                for command in ('default-keychain', 'list-keychains')]
            result['runner_keychain_unchanged'] = len(runner) == 2 and runner == after
        except Exception:
            pass
        result['ok'] = result['ok'] and result['owned_account_removed'] and result['runner_keychain_unchanged']
        result['elapsed_ms'] = round((time.monotonic() - started) * 1000)
        report.write_text(json.dumps(result, indent=2) + '\n')
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--bootstrap', required=True, type=Path)
    parser.add_argument('--assets-dir', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--inner', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--pins', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--source-commit', default=os.environ.get('MARMISH_BUILD_COMMIT'))
    parser.add_argument('--target')
    parser.add_argument('--package-versions', type=Path)
    parser.add_argument('--deadline-seconds', type=int, default=540, help=argparse.SUPPRESS)
    options = parser.parse_args()
    operation = mac_account if platform.system() == 'Darwin' and not options.inner else inner
    if options.inner:
        account = pwd.getpwuid(os.geteuid())
        require(platform.system() == 'Darwin' and account.pw_name.startswith('marmishq')
            and 55000 <= account.pw_uid < 55100 and Path(account.pw_dir) == Path('/Users') / account.pw_name,
            'owned_account_context_required')
    try:
        if options.inner:
            require(options.pins is not None, 'staged_pins_required')
            code = inner(options.bootstrap.resolve(), options.assets_dir.resolve(), options.report.resolve(),
                options.pins.resolve(), options.deadline_seconds)
        else:
            code = operation(options.bootstrap.resolve(), options.assets_dir.resolve(), options.report.resolve())
    except Exception:
        options.report.write_text(json.dumps({'schema': 1, 'kind': 'installed_auto_provider',
            'ok': False, 'failure': 'qualification_prerequisite_failed',
            'owned_children_reaped': False, 'owned_state_removed': False}) + '\n')
        code = 1
    value = json.loads(options.report.read_bytes())
    target = {('Linux', 'x86_64'): 'x86_64-unknown-linux-gnu',
        ('Linux', 'aarch64'): 'aarch64-unknown-linux-gnu',
        ('Darwin', 'arm64'): 'aarch64-apple-darwin'}.get((platform.system(), platform.machine()))
    if options.target is not None and options.target != target:
        value.update(ok=False, failure='qualification_target_mismatch')
        code = 1
    source = options.source_commit
    if not options.inner and not (source and re.fullmatch('[0-9a-f]{40}', source)):
        value.update(ok=False, failure='qualification_source_missing')
        code = 1
    value.update(source_commit=source, target=target,
        run_id=os.environ.get('GITHUB_RUN_ID'), run_attempt=os.environ.get('GITHUB_RUN_ATTEMPT'))
    versions = []
    if options.package_versions and options.package_versions.exists():
        raw = options.package_versions.read_bytes()
        require(len(raw) <= 4096, 'package_metadata_limit')
        versions = raw.decode('ascii').splitlines()
        require(len(versions) <= 8 and all(re.fullmatch('[a-z0-9.+:-]+=[A-Za-z0-9.+:~_-]+', line)
            for line in versions), 'package_metadata_invalid')
    value['provider_packages'] = versions
    options.report.write_text(json.dumps(value, indent=2) + '\n')
    raise SystemExit(code)
