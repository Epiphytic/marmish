#!/usr/bin/env python3
"""CI-only qualification of installed Auto custody in disposable OS contexts.

Never uploads terminal/provider output, key references or private paths. This
supervisor is build tooling; no Python is placed on product children's PATH.
"""
import argparse
from contextlib import contextmanager
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
    def __init__(self, category, exit_code=None):
        super().__init__(category)
        self.exit_code = exit_code


STAGES = frozenset('''diagnostic_invalid runner_default_snapshot runner_search_snapshot
account_uid_list account_uid_parse account_identity_lookup account_home_check account_create
account_attribute_uid account_attribute_gid account_attribute_home account_attribute_shell
account_attribute_realname account_attribute_password account_attribute_auth account_identity_check
account_home_create account_home_chown input_stage stage_copy stage_permissions interpreter_lookup
interpreter_validate interpreter_hash inner_budget inner_launch inner_report_read inner_report_parse
inner_report_validate interpreter_recheck qualification_validate cleanup_identity cleanup_bootout
cleanup_processes cleanup_delete_account cleanup_delete_home cleanup_verify_home cleanup_runner_default
cleanup_runner_search cleanup_runner_compare'''.split())


def provider_stage(result, stage):
    result['_stage'] = stage


def provider_failure(result, error, cleanup=False):
    prefix = 'cleanup_' if cleanup else ''
    category = str(error) if isinstance(error, Refused) else (
        'owned_account_cleanup_incomplete' if cleanup else 'owned_account_prerequisite_failed')
    if cleanup:
        result.setdefault('cleanup_failure', category)
    else:
        # Inner reports are untrusted; preserve the supervisor's finite category.
        result['failure'] = category
    if '_stage' in result:
        result.setdefault(prefix + 'failure_stage', result['_stage'])
    if getattr(error, 'exit_code', None) is not None:
        result.setdefault(prefix + 'exit_code', error.exit_code)
    result['ok'] = False


def sanitize_provider_report(result):
    """Only source-defined stages and bounded numeric statuses leave the supervisor."""
    result.pop('_stage', None)
    for prefix in ('', 'cleanup_'):
        stage = prefix + 'failure_stage'
        if stage in result and (not isinstance(result[stage], str) or result[stage] not in STAGES):
            result[stage] = 'diagnostic_invalid'
        code = prefix + 'exit_code'
        if code in result and not (type(result[code]) is int and -128 <= result[code] <= 255):
            del result[code]
    return result


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
    except PermissionError:
        # Darwin can report EPERM for a group containing only an exited,
        # unreaped leader. Permission denial itself never proves absence.
        try:
            child.wait(timeout=3)
        except (subprocess.TimeoutExpired, OSError):
            raise Refused('child_cleanup_incomplete') from None
        try:
            os.killpg(child.pid, 0)
        except ProcessLookupError:
            return
        except OSError:
            pass
        raise Refused('child_cleanup_incomplete') from None
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


def execute(argv, environment, timeout=15, data=None, success=True, error_map=None):
    require(data is None or len(data) <= 4096, 'operation_input_limit')
    child = subprocess.Popen(argv, env=environment, stdin=subprocess.PIPE if data is not None
        else subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        start_new_session=True)
    try:
        if data is not None:
            child.stdin.write(data)
            child.stdin.close()
        out, errors, total = bytearray(), bytearray(), 0
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
                    elif error_map is not None:
                        errors.extend(raw)
        child.wait(timeout=max(0.001, deadline - time.monotonic()))
        category = error_map.get(bytes(errors).strip(), error_map[None]) if error_map else 'operation_failed'
        if success and child.returncode != 0:
            raise Refused(category, exit_code=child.returncode)
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

    def daemon_reload(self, name, public):
        # Exercise the distinct daemon frontend, without claiming full startup.
        _, raw = execute([str(self.bins / 'marmishd'), '--state-dir', str(self.state),
            '-i', name, 'init'], self.environment, 120)
        require(json.loads(raw).get('identity') == public, 'fresh_identity_reload')

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
        self.daemon_reload('default', first['public_key'])
        self.daemon_reload('second', second['public_key'])
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
        self.daemon_reload('default', first['public_key'])
        self.daemon_reload('second', second['public_key'])
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


def lifecycle_versions(version):
    require(isinstance(version, str) and len(version) <= 43
        and re.fullmatch(r'0\.2\.(0|[1-9][0-9]{0,19})', version)
        and int(version[4:]) <= 18446744073709551615, 'lifecycle_arguments')
    return version, version + '-rc.1'


def lifecycle_arguments(a_assets, a_pin, b_version):
    values = (a_assets, a_pin, b_version)
    require(all(value is None for value in values) or all(value is not None for value in values),
        'lifecycle_arguments')
    if a_assets is not None:
        require(re.fullmatch('[0-9a-f]{64}', a_pin), 'lifecycle_arguments')
        lifecycle_versions(b_version)
    return a_assets is not None


class LifecycleQualification(Qualification):
    """Actual A/B helper processes; a denial never becomes an upgrade pass."""
    def __init__(self, root, bootstrap, assets, a_assets, a_pin, b_version):
        super().__init__(root, bootstrap, assets)
        self.b_version, self.a_version = lifecycle_versions(b_version)
        self.a_assets, self.a_pin = a_assets, a_pin

    def select(self, version):
        execute([str(self.bins / 'marmish'), 'install', '--select', version,
            '--prefix', str(self.prefix), '--bin-dir', str(self.bins)], self.environment, 60)
        _, raw = execute([str(self.bins / 'marmish'), 'release-info'], self.environment)
        require(json.loads(raw).get('version') == version, 'lifecycle_selected_version')

    def retained(self, path, expected, selected=False):
        info = path.stat()
        require((info.st_dev, info.st_ino, info.st_size, digest(path)) == expected
            and (not selected or (self.bins / 'marmish').resolve(strict=True) == path),
            'lifecycle_old_helper_retained')

    def upgrade_reload(self, public):
        _, raw = execute([str(self.bins / 'marmish'), '--state-dir', str(self.state),
            '-i', 'default', 'init'], self.environment, 60, error_map={
                b'marmish: identity: locked_or_denied': 'lifecycle_upgrade_locked_or_denied',
                None: 'lifecycle_upgrade_reload_failed'})
        require(json.loads(raw).get('identity') == public, 'lifecycle_upgrade_identity')

    def run(self, result):
        self.environment = self.provider.start()
        manifests = list(self.a_assets.glob('*.manifest'))
        require(len(manifests) == 1 and digest(manifests[0]) == self.a_pin,
            'lifecycle_a_manifest_pin')
        binaries = list(self.a_assets.glob('marmish-' + self.a_version + '-*'))
        binaries = [path for path in binaries if not path.name.endswith('.manifest')]
        require(len(binaries) == 1, 'lifecycle_a_binary')
        executable = self.root / 'a-installer'
        shutil.copyfile(binaries[0], executable)
        executable.chmod(0o500)
        execute([str(executable), 'install', '--source-dir', str(self.a_assets),
            '--manifest', str(manifests[0]), '--manifest-sha256', self.a_pin,
            '--prefix', str(self.prefix), '--bin-dir', str(self.bins)], self.environment, 60)
        self.wait_probe()
        _, raw = self.command('default', '--relay', 'wss://relay.invalid/',
            'setup', '--role', 'both', '--no-grants')
        require(json.loads(raw).get('backend') == 'keychain', 'lifecycle_a_auto_keychain')
        first = self.manifest('default')
        references = self.items(2)
        self.reload('default', first['public_key'])
        old_helper = (self.bins / 'marmish').resolve(strict=True)
        info = old_helper.stat()
        old_identity = (info.st_dev, info.st_ino, info.st_size, digest(old_helper))
        result['lifecycle_stage'] = 'upgrade_install'
        execute(['/bin/sh', str(self.bootstrap), '--asset-dir', str(self.assets),
            '--prefix', str(self.prefix), '--bin-dir', str(self.bins)], self.environment, 60)
        _, raw = execute([str(self.bins / 'marmish'), 'release-info'], self.environment)
        require(json.loads(raw).get('version') == self.b_version, 'lifecycle_b_version')
        self.retained(old_helper, old_identity)
        result['lifecycle_stage'] = 'upgrade_reload'
        self.upgrade_reload(first['public_key'])
        self.upgrade_reload(first['public_key'])
        require(self.manifest('default') == first, 'lifecycle_profile_changed')
        require(self.items(2) == references, 'lifecycle_upgrade_items_changed')
        result['lifecycle_upgrade_loaded'] = True
        result['lifecycle_stage'] = 'rollback_reload'
        self.select(self.a_version)
        self.retained(old_helper, old_identity, selected=True)
        self.reload('default', first['public_key'])
        self.retained(old_helper, old_identity, selected=True)
        require(self.manifest('default') == first, 'lifecycle_profile_changed')
        self.select(self.b_version)
        self.retained(old_helper, old_identity)
        self.reload('default', first['public_key'])
        require(self.manifest('default') == first, 'lifecycle_profile_changed')
        require(self.items(2) == references, 'lifecycle_rollback_items_changed')
        result['lifecycle_rollback_loaded'] = True
        result['lifecycle_stage'] = 'locked_denial'
        before = self.snapshot('default')
        self.provider.lock()
        self.wait_probe('locked_or_denied')
        require(self.command('default', 'init', success=False)[0] != 0,
            'lifecycle_locked_existing_accepted')
        require(self.command('locked-new', '--relay', 'wss://relay.invalid/',
            'setup', '--role', 'both', '--no-grants', success=False)[0] != 0,
            'lifecycle_locked_fresh_accepted')
        require(self.snapshot('default') == before
            and not (self.state / 'locked-new' / 'keys').exists(), 'lifecycle_locked_fallback')
        self.manifest('default')
        self.provider.unlock()
        self.wait_probe()
        self.reload('default', first['public_key'])
        require(self.items(2) == references, 'lifecycle_locked_items_changed')
        result['lifecycle_locked_denied'] = True
        result['lifecycle_stage'] = 'complete'


def inner(bootstrap, assets, report, pins=None, deadline_seconds=540,
          lifecycle_a_assets=None, lifecycle_a_pin=None, lifecycle_b_version=None):
    lifecycle_arguments(lifecycle_a_assets, lifecycle_a_pin, lifecycle_b_version)
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
    qualification = (LifecycleQualification(root, bootstrap, assets, lifecycle_a_assets, lifecycle_a_pin, lifecycle_b_version)
        if lifecycle_a_assets else Qualification(root, bootstrap, assets))
    def expired(_signum, _frame):
        raise Refused('qualification_deadline')
    signal.signal(signal.SIGALRM, expired)
    signal.signal(signal.SIGTERM, expired)
    require(1 <= deadline_seconds <= (420 if lifecycle_a_assets else 540), 'qualification_budget_invalid')
    signal.alarm(deadline_seconds)
    result = {'schema': 1, 'kind': 'installed_auto_provider', 'ok': False,
        'python_in_runtime_path': False, 'network_activation': False,
        'owned_children_reaped': False, 'owned_state_removed': False}
    if lifecycle_a_assets:
        result.update(lifecycle_upgrade_loaded=False, lifecycle_rollback_loaded=False,
            lifecycle_locked_denied=False, lifecycle_stage='initial_setup')
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
            result['ok'] = False
            result.setdefault('failure', 'provider_cleanup_incomplete')
        if result['ok'] and result['owned_children_reaped']:
            shutil.rmtree(root)
            result['owned_state_removed'] = not root.exists()
        signal.alarm(0)
        result['elapsed_ms'] = round((time.monotonic() - started) * 1000)
        report.write_text(json.dumps(result, indent=2) + '\n')
    return 0 if result['ok'] and result['owned_state_removed'] else 1



@contextmanager
def staged_provider_inputs(bootstrap, assets, lifecycle_a_assets=None, lifecycle_b_version=None):
    """Pin the exact unprivileged inputs before copying into the owned account."""
    stage = Path(tempfile.mkdtemp(prefix='marmish-provider-stage-'))
    stage.chmod(0o755)
    try:
        pins = {}
        inputs = [('', [Path(__file__),
            Path(__file__).with_name('marmish_native_quickstart_check.py'), bootstrap])]
        versions = lifecycle_versions(lifecycle_b_version) if lifecycle_a_assets else (None, None)
        directories = [('assets', assets, versions[0])]
        if lifecycle_a_assets:
            directories.append(('a-assets', lifecycle_a_assets, versions[1]))
        for directory, source_dir, version in directories:
            sources = list(source_dir.iterdir())
            if lifecycle_a_assets:
                suffix = version + '-aarch64-apple-darwin'
                expected = {name + '-' + suffix for name in
                    ('marmish', 'marmishd', 'marmish-worker', 'THIRD-PARTY-LICENSES')}
                expected.add('marmish-' + suffix + '.manifest')
                require({path.name for path in sources} == expected, 'lifecycle_asset_set')
            (stage / directory).mkdir(mode=0o755)
            inputs.append((directory, sources))
        for directory, sources in inputs:
            for source in sources:
                destination = stage / directory / source.name
                expected = digest(source)
                shutil.copy2(source, destination)
                destination.chmod(0o644)
                require(digest(destination) == expected, 'staged_input_changed')
                pins[str(destination.relative_to(stage))] = expected
        require(len(pins) == 13 if lifecycle_a_assets else len(pins) <= 16, 'staged_input_count')
        (stage / 'input-pins.json').write_text(json.dumps(pins))
        (stage / 'input-pins.json').chmod(0o644)
        yield stage
    finally:
        shutil.rmtree(stage)


def provider_inner_budget(started, total, lifecycle, now):
    remaining = (total - 60 if lifecycle else 570) - (now - started)
    require(remaining > 30, 'owned_account_setup_deadline')
    return remaining, min(420 if lifecycle else 540, int(remaining - 20))



class ProviderSignals:
    """Own lifecycle-only alarms and restore every inherited signal disposition."""
    def __init__(self, enabled, seconds):
        self.enabled, self.seconds = enabled, seconds
        self.cancelled = False

    def __enter__(self):
        if self.enabled:
            def expired(_signum, _frame):
                raise Refused('lifecycle_provider_deadline')
            self.previous = tuple(signal.signal(sig, expired) for sig in
                (signal.SIGALRM, signal.SIGTERM, signal.SIGINT))
            signal.alarm(min(60, self.seconds - 60))
        return self

    def limit_inner(self, remaining):
        if self.enabled:
            signal.alarm(max(1, int(remaining)))

    def cleanup(self):
        if self.enabled:
            signal.alarm(0)
            # Python handlers reset on exec: cleanup children retain normal TERM.
            def recorded(_signum, _frame):
                self.cancelled = True
            signal.signal(signal.SIGTERM, recorded)
            signal.signal(signal.SIGINT, recorded)

    def __exit__(self, *_exception):
        if self.enabled:
            signal.alarm(0)
            for sig, handler in zip((signal.SIGALRM, signal.SIGTERM, signal.SIGINT), self.previous):
                signal.signal(sig, handler)


def provider_result_check(code, result, lifecycle):
    if lifecycle and result.get('failure') in ('lifecycle_upgrade_locked_or_denied',
            'lifecycle_upgrade_reload_failed', 'operation_timeout'):
        raise Refused(result['failure'])
    require(code == 0 and result['ok'], 'owned_account_qualification_failed')
    require(not lifecycle or (result.get('lifecycle_stage') == 'complete' and all(
        result.get(key) is True for key in ('lifecycle_upgrade_loaded', 'lifecycle_rollback_loaded',
            'lifecycle_locked_denied'))), 'lifecycle_provider_incomplete')


def mac_account(bootstrap, assets, report, lifecycle_a_assets=None, lifecycle_a_pin=None,
                deadline_seconds=540, lifecycle_b_version=None):
    """Outer privilege is restricted to staging and one newly owned OS account."""
    lifecycle_arguments(lifecycle_a_assets, lifecycle_a_pin, lifecycle_b_version)
    environment = {'PATH': '/usr/bin:/bin:/usr/sbin:/sbin',
        'HOME': pwd.getpwuid(os.geteuid()).pw_dir, 'LC_ALL': 'C'}
    result = {'schema': 1, 'kind': 'installed_auto_provider', 'ok': False,
        'owned_account_removed': False, 'runner_keychain_unchanged': False}
    started = time.monotonic()
    lifecycle = lifecycle_a_assets is not None
    require(not lifecycle or 120 < deadline_seconds <= 540, 'qualification_budget_invalid')
    total_budget = deadline_seconds if lifecycle else 600
    def cleanup_execute(argv, success=True):
        remaining = total_budget - (time.monotonic() - started)
        require(remaining > 0, 'owned_account_cleanup_deadline')
        return execute(argv, environment, min(5, remaining), success=success)
    name, uid, home, created = 'marmishq' + uuid.uuid4().hex[:10], None, None, False
    runner = []
    with ProviderSignals(lifecycle, deadline_seconds) as signals:
        try:
            for command in ('default-keychain', 'list-keychains'):
                provider_stage(result, 'runner_default_snapshot' if command == 'default-keychain' else 'runner_search_snapshot')
                runner.append(execute(['/usr/bin/security', command, '-d', 'user'], environment)[1])
            provider_stage(result, 'account_uid_list')
            _, raw = execute(['/usr/bin/dscl', '.', '-list', '/Users', 'UniqueID'], environment)
            provider_stage(result, 'account_uid_parse')
            used = {int(line.split()[-1]) for line in raw.decode().splitlines()}
            uid = next((number for number in range(55000, 55100) if number not in used), None)
            require(uid is not None, 'owned_account_uid_exhausted')
            provider_stage(result, 'account_identity_lookup')
            try:
                pwd.getpwnam(name)
                raise Refused('owned_account_already_exists')
            except KeyError:
                pass
            provider_stage(result, 'account_home_check')
            home = Path('/Users') / name
            require(not home.exists() and not home.is_symlink(), 'owned_home_already_exists')
            account_deadline = min(time.monotonic() + 60, started + total_budget - 60)
            def administrative(*arguments):
                remaining = account_deadline - time.monotonic()
                require(remaining > 0, 'owned_account_setup_deadline')
                return execute(['/usr/bin/sudo', '-n', '/usr/bin/dscl', '.', *arguments],
                    environment, min(15, remaining))
            provider_stage(result, 'account_create')
            created = True
            administrative('-create', '/Users/' + name)
            for key, value in (('UniqueID', str(uid)), ('PrimaryGroupID', '-2'),
                    ('NFSHomeDirectory', str(home)), ('UserShell', '/usr/bin/false'),
                    ('RealName', 'Disposable Marmish CI'), ('Password', '*'),
                    ('AuthenticationAuthority', ';DisabledUser;')):
                provider_stage(result, {'UniqueID': 'account_attribute_uid', 'PrimaryGroupID': 'account_attribute_gid',
                    'NFSHomeDirectory': 'account_attribute_home', 'UserShell': 'account_attribute_shell',
                    'RealName': 'account_attribute_realname', 'Password': 'account_attribute_password',
                    'AuthenticationAuthority': 'account_attribute_auth'}[key])
                administrative('-create', '/Users/' + name, key, value)
            provider_stage(result, 'account_identity_check')
            account = pwd.getpwnam(name)
            require(account.pw_uid == uid and account.pw_dir == str(home), 'owned_account_identity')
            provider_stage(result, 'account_home_create')
            execute(['/usr/bin/sudo', '-n', '/bin/mkdir', '-m', '700', str(home)], environment)
            provider_stage(result, 'account_home_chown')
            execute(['/usr/bin/sudo', '-n', '/usr/sbin/chown', str(uid), str(home)], environment)
            # Stage outside the runner's private HOME; rehash every copied input.
            provider_stage(result, 'input_stage')
            with staged_provider_inputs(bootstrap, assets, lifecycle_a_assets, lifecycle_b_version) as stage:
                provider_stage(result, 'stage_copy')
                execute(['/usr/bin/sudo', '-n', '/bin/cp', '-R', str(stage), str(home / 'stage')], environment)
                provider_stage(result, 'stage_permissions')
                execute(['/usr/bin/sudo', '-n', '/bin/chmod', '-R', 'a+rX', str(home / 'stage')], environment)
            inner_report = home / 'provider-result.json'
            provider_stage(result, 'interpreter_lookup')
            _, raw = execute(['/usr/bin/xcrun', '--find', 'python3'], environment)
            provider_stage(result, 'interpreter_validate')
            python = Path(raw.decode().strip()).resolve()
            require(python.is_file() and not python.is_relative_to(Path(environment['HOME'])),
                'owned_account_interpreter')
            provider_stage(result, 'interpreter_hash')
            interpreter_digest = digest(python)
            provider_stage(result, 'inner_budget')
            remaining, inner_budget = provider_inner_budget(started, total_budget, lifecycle, time.monotonic())
            signals.limit_inner(remaining)
            argv = ['/usr/bin/sudo', '-n', '/bin/launchctl', 'asuser', str(uid),
                '/usr/bin/sudo', '-n', '-H', '-u', name, '/usr/bin/env', '-i',
                'HOME=' + str(home), 'USER=' + name, 'LOGNAME=' + name, 'PATH=/usr/bin:/bin',
                str(python), '-I', str(home / 'stage' / Path(__file__).name), '--inner',
                '--bootstrap', str(home / 'stage' / bootstrap.name),
                '--assets-dir', str(home / 'stage/assets'), '--report', str(inner_report),
                '--pins', str(home / 'stage/input-pins.json'), '--deadline-seconds', str(inner_budget)]
            if lifecycle:
                argv += ['--lifecycle-a-assets', str(home / 'stage/a-assets'),
                    '--lifecycle-a-manifest-sha256', lifecycle_a_pin,
                    '--lifecycle-b-version', lifecycle_b_version]
            provider_stage(result, 'inner_launch')
            code, _ = execute(argv, environment, remaining, success=False)
            provider_stage(result, 'inner_report_read')
            _, raw = execute(['/usr/bin/sudo', '-n', '/bin/cat', str(inner_report)], environment)
            require(len(raw) <= 16384, 'owned_account_report_limit')
            provider_stage(result, 'inner_report_parse')
            received = json.loads(raw)
            provider_stage(result, 'inner_report_validate')
            require(received.get('kind') == 'installed_auto_provider', 'owned_account_report')
            for key in ('ok', 'failure', 'auto_keychain', 'distinct_named_identities', 'fresh_reload',
                    'item_count', 'locked_refused', 'unlocked_fresh_reload', 'no_plaintext_fallback',
                    'python_in_runtime_path', 'network_activation', 'owned_children_reaped', 'owned_state_removed',
                    'lifecycle_upgrade_loaded', 'lifecycle_rollback_loaded', 'lifecycle_locked_denied',
                    'lifecycle_stage'):
                if key in received:
                    result[key] = received[key]
            provider_stage(result, 'interpreter_recheck')
            result['supervisor_interpreter_unchanged'] = digest(python) == interpreter_digest
            require(result['supervisor_interpreter_unchanged'], 'interpreter_changed')
            provider_stage(result, 'qualification_validate')
            provider_result_check(code, result, lifecycle)
        except Refused as error:
            provider_failure(result, error)
        except Exception as error:
            provider_failure(result, error)
        finally:
            signals.cleanup()
            if created and uid is not None:
                try:
                    provider_stage(result, 'cleanup_identity')
                    account = pwd.getpwnam(name)
                    require(account.pw_uid == uid and account.pw_dir == str(home), 'owned_account_cleanup_identity')
                    # Never kill by process name: this freshly allocated UID is the scope.
                    provider_stage(result, 'cleanup_bootout')
                    cleanup_execute(['/usr/bin/sudo', '-n', '/bin/launchctl', 'bootout', 'user/' + str(uid)],
                        success=False)
                    provider_stage(result, 'cleanup_processes')
                    _, raw = cleanup_execute(['/bin/ps', '-u', str(uid), '-o', 'pid='], success=False)
                    require(not raw.strip(), 'owned_account_children_remain')
                    if result['ok']:
                        account = pwd.getpwnam(name)
                        require(account.pw_uid == uid and account.pw_dir == str(home)
                            and home.lstat().st_uid == uid and not home.is_symlink(), 'owned_account_cleanup_identity')
                        provider_stage(result, 'cleanup_delete_account')
                        cleanup_execute(['/usr/bin/sudo', '-n', '/usr/bin/dscl', '.', '-delete', '/Users/' + name])
                        provider_stage(result, 'cleanup_delete_home')
                        cleanup_execute(['/usr/bin/sudo', '-n', '/bin/rm', '-rf', str(home)])
                        provider_stage(result, 'cleanup_verify_home')
                        require(not home.exists(), 'owned_account_home_remains')
                        result['owned_account_removed'] = True
                except Exception as error:
                    provider_failure(result, error, cleanup=True)
                    result.setdefault('failure', 'owned_account_cleanup_incomplete')
            try:
                after = []
                for command in ('default-keychain', 'list-keychains'):
                    provider_stage(result, 'cleanup_runner_default' if command == 'default-keychain' else 'cleanup_runner_search')
                    after.append(cleanup_execute(['/usr/bin/security', command, '-d', 'user'])[1])
                provider_stage(result, 'cleanup_runner_compare')
                result['runner_keychain_unchanged'] = len(runner) == 2 and runner == after
                require(result['runner_keychain_unchanged'], 'owned_account_cleanup_incomplete')
            except Exception as error:
                provider_failure(result, error, cleanup=True)
                result.setdefault('failure', 'owned_account_cleanup_incomplete')
            result['ok'] = result['ok'] and result['owned_account_removed'] and result['runner_keychain_unchanged']
            if signals.cancelled:
                result.update(ok=False, failure='lifecycle_provider_signal')
                result.pop('failure_stage', None)
                result.pop('exit_code', None)
            result['elapsed_ms'] = round((time.monotonic() - started) * 1000)
            report.write_text(json.dumps(sanitize_provider_report(result), indent=2) + '\n')
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
    parser.add_argument('--lifecycle-a-assets', type=Path)
    parser.add_argument('--lifecycle-a-manifest-sha256')
    parser.add_argument('--lifecycle-b-version')
    options = parser.parse_args()
    operation = mac_account if platform.system() == 'Darwin' and not options.inner else inner
    if options.inner:
        account = pwd.getpwuid(os.geteuid())
        require(platform.system() == 'Darwin' and account.pw_name.startswith('marmishq')
            and 55000 <= account.pw_uid < 55100 and Path(account.pw_dir) == Path('/Users') / account.pw_name,
            'owned_account_context_required')
    try:
        lifecycle = lifecycle_arguments(options.lifecycle_a_assets,
            options.lifecycle_a_manifest_sha256, options.lifecycle_b_version)
        if lifecycle:
            require(platform.system() == 'Darwin'
                and re.fullmatch('[0-9a-f]{64}', options.lifecycle_a_manifest_sha256), 'lifecycle_arguments')
        extra = ({'lifecycle_a_assets': options.lifecycle_a_assets.resolve(),
            'lifecycle_a_pin': options.lifecycle_a_manifest_sha256,
            'lifecycle_b_version': options.lifecycle_b_version} if lifecycle else {})
        if options.inner:
            require(options.pins is not None, 'staged_pins_required')
            code = inner(options.bootstrap.resolve(), options.assets_dir.resolve(), options.report.resolve(),
                options.pins.resolve(), options.deadline_seconds, **extra)
        else:
            if lifecycle:
                extra['deadline_seconds'] = options.deadline_seconds
            code = operation(options.bootstrap.resolve(), options.assets_dir.resolve(), options.report.resolve(), **extra)
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
    options.report.write_text(json.dumps(sanitize_provider_report(value), indent=2) + '\n')
    raise SystemExit(code)
