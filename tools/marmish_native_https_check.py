#!/usr/bin/env python3
"""CI-only bounded supervisor for real HTTPS native installer qualification.

The downloaded shell and installed Rust children get a utility-only PATH. Private
terminal/provider output is never evidence; this module emits finite reports.
"""
import argparse
import errno
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import pty
import pwd
import re
import select
import shutil
import stat
import subprocess
import tempfile
import time
import urllib.request


def load_helper(name):
    spec = importlib.util.spec_from_file_location(name,
        Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


quick = load_helper('marmish_native_quickstart_check')
supervisor = load_helper('marmish_native_keychain_check')
Refused, require = supervisor.Refused, supervisor.require
TARGETS = ('x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin')
SOURCES = {
    'iris': 'https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh',
    'github': 'https://raw.githubusercontent.com/Epiphytic/marmish/master/scripts/install.sh',
}
FILES = ('marmish', 'marmishd', 'marmish-worker', 'THIRD-PARTY-LICENSES')
CONTEXT_KEYS = {'schema', 'qualification_run', 'qualification_attempt', 'workflow_commit',
    'candidate_run', 'candidate_attempt', 'source_commit', 'bootstrap_commit',
    'bootstrap_sha256', 'version', 'manifest_pins'}
REPORT_KEYS = {'schema', 'kind', 'context_sha256', 'qualification_run', 'qualification_attempt',
    'source_commit', 'bootstrap_commit', 'source', 'target', 'ok', 'stage', 'failure',
    'both_sources_verified', 'bootstrap_sha256', 'manifest_sha256', 'install_verified',
    'setup_verified', 'status_verified', 'repeat_verified', 'provenance_verified',
    'owned_processes_stopped', 'profile_lease_released', 'runtime_removed',
    'owned_state_removed', 'elapsed_ms'}
BOOL_KEYS = ('both_sources_verified', 'install_verified', 'setup_verified', 'status_verified',
    'repeat_verified', 'provenance_verified', 'owned_processes_stopped',
    'profile_lease_released', 'runtime_removed', 'owned_state_removed')
STAGES = ('context', 'download', 'install', 'setup', 'manifest', 'provenance', 'status',
    'repeat', 'cleanup', 'complete')
MAX_U64 = 18446744073709551615


def decimal(value, positive=True):
    require(isinstance(value, str) and re.fullmatch(r'0|[1-9][0-9]{0,19}', value)
        and int(value) <= MAX_U64 and (not positive or int(value) > 0), 'invalid_decimal')
    return value


def hex_value(value, size):
    require(isinstance(value, str) and re.fullmatch('[0-9a-f]{' + str(size) + '}', value),
        'invalid_digest')
    return value


def version_value(value):
    require(isinstance(value, str) and len(value) <= 48, 'invalid_version')
    found = re.fullmatch(r'0\.2\.(0|[1-9][0-9]*)(?:-rc\.([1-9][0-9]*))?', value)
    require(found is not None, 'invalid_version')
    decimal(found[1], False)
    if found[2]:
        decimal(found[2])
    return value


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate_json_key')
        result[key] = value
    return result


def decode(raw):
    return json.loads(raw, object_pairs_hook=unique_object)


def bounded_read(path, maximum):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and info.st_size <= maximum, 'input_size_or_type')
    with path.open('rb') as source:
        raw = source.read(maximum + 1)
    require(len(raw) <= maximum, 'input_size_or_type')
    return raw


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':')) + '\n').encode()


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def validate_context(context):
    require(type(context) is dict and set(context) == CONTEXT_KEYS and type(context['schema']) is int and context['schema'] == 1,
        'context_schema')
    for key in ('qualification_run', 'qualification_attempt', 'candidate_run', 'candidate_attempt'):
        decimal(context[key])
    for key in ('workflow_commit', 'source_commit', 'bootstrap_commit'):
        hex_value(context[key], 40)
    require(context['workflow_commit'] == context['bootstrap_commit'], 'workflow_commit_mismatch')
    hex_value(context['bootstrap_sha256'], 64)
    version_value(context['version'])
    require(type(context['manifest_pins']) is dict and set(context['manifest_pins']) == set(TARGETS),
        'manifest_pins')
    for pin in context['manifest_pins'].values():
        hex_value(pin, 64)
    return context


def load_context(path):
    raw = bounded_read(path, 8192)
    context = validate_context(decode(raw))
    require(raw == encoded(context), 'context_encoding')
    return context, sha(raw)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *_args, **_kwargs):
        raise Refused('api_redirect')


def api(path, token):
    require(path.startswith('/repos/Epiphytic/marmish/actions/'), 'api_path')
    request = urllib.request.Request('https://api.github.com' + path,
        headers={'Authorization': 'Bearer ' + token, 'Accept': 'application/vnd.github+json',
                 'X-GitHub-Api-Version': '2022-11-28'})
    with urllib.request.build_opener(NoRedirect).open(request, timeout=30) as response:
        require(response.status == 200, 'api_status')
        raw = response.read(1024 * 1024 + 1)
    require(len(raw) <= 1024 * 1024, 'api_size')
    return decode(raw)


def validate_run(run, context):
    require(run.get('id') == int(context['candidate_run'])
        and run.get('run_attempt') == int(context['candidate_attempt'])
        and run.get('head_sha') == context['source_commit']
        and run.get('head_branch') == 'master' and run.get('event') in ('workflow_dispatch', 'push')
        and run.get('status') == 'completed' and run.get('conclusion') == 'success'
        and run.get('path') == '.github/workflows/v02-release.yml'
        and run.get('repository', {}).get('full_name') == 'Epiphytic/marmish'
        and run.get('head_repository', {}).get('full_name') == 'Epiphytic/marmish',
        'candidate_run_binding')


def validate_artifacts(artifacts, name):
    found = [item for item in artifacts if item.get('name') == name]
    require(len(found) == 1 and found[0].get('expired') is False
        and type(found[0].get('size_in_bytes')) is int
        and 0 < found[0]['size_in_bytes'] <= 131072, 'candidate_artifact_binding')


def git(arguments):
    environment = {key: os.environ[key] for key in ('PATH', 'HOME') if key in os.environ}
    environment['LC_ALL'] = 'C'
    return supervisor.execute(['git', *arguments], environment, timeout=30)[1]


def resolve(options):
    context = {key: getattr(options, key) for key in CONTEXT_KEYS
        - {'schema', 'manifest_pins'}}
    context.update(schema=1, manifest_pins={target: '0' * 64 for target in TARGETS})
    validate_context(context)
    require(git(['rev-parse', 'HEAD']).decode().strip() == context['bootstrap_commit'], 'checkout_binding')
    git(['merge-base', '--is-ancestor', context['source_commit'], context['bootstrap_commit']])
    require(git(['diff', '--no-renames', '--name-only', context['source_commit'], context['bootstrap_commit'], '--'])
        == b'scripts/install.sh\n', 'bootstrap_changed_paths')
    git(['cat-file', '-e', context['source_commit'] + ':.github/workflows/v02-installer-https.yml'])
    token = os.environ.get('GH_TOKEN', '')
    require(bool(token), 'api_token_missing')
    validate_run(api('/repos/Epiphytic/marmish/actions/runs/' + context['candidate_run']
        + '/attempts/' + context['candidate_attempt'], token), context)
    name = 'v02-pinned-bootstrap-' + context['candidate_run'] + '-' + context['candidate_attempt']
    artifacts = []
    for page in range(1, 21):
        value = api('/repos/Epiphytic/marmish/actions/runs/' + context['candidate_run']
            + '/artifacts?per_page=100&page=' + str(page), token)
        batch = value.get('artifacts')
        require(type(batch) is list and len(batch) <= 100, 'artifact_list')
        artifacts.extend(batch)
        if len(batch) < 100:
            break
    else:
        raise Refused('artifact_list_limit')
    validate_artifacts(artifacts, name)
    options.output.write_bytes(encoded(context))


def pins_from_script(script, template, version):
    substitutions = (('version', '@MARMISH_VERSION@', version),
        ('sha_amd64', '@MARMISH_SHA_AMD64@', None),
        ('sha_arm64', '@MARMISH_SHA_ARM64@', None),
        ('sha_macos', '@MARMISH_SHA_MACOS@', None))
    pins = []
    for name, placeholder, expected in substitutions:
        matches = re.findall(rb"^    " + name.encode() + rb"='([^'\n]*)'$", script, re.MULTILINE)
        require(len(matches) == 1 and template.count(placeholder.encode()) == 1, 'script_pins')
        value = matches[0].decode('ascii')
        require(value == expected if expected is not None else bool(re.fullmatch('[0-9a-f]{64}', value)),
            'script_pins')
        template = template.replace(placeholder.encode(), matches[0])
        if expected is None:
            pins.append(value)
    require(template == script, 'script_template_mismatch')
    return dict(zip(TARGETS, pins))


def bind_context(options):
    context, _ = load_context(options.context)
    entries = list(options.artifact.iterdir())
    require(len(entries) == 1 and entries[0].name == 'install.sh', 'bootstrap_artifact_entries')
    script = bounded_read(entries[0], 65536)
    require(sha(script) == context['bootstrap_sha256'], 'bootstrap_digest')
    require(git(['show', context['bootstrap_commit'] + ':scripts/install.sh']) == script,
        'bootstrap_commit_bytes')
    context['manifest_pins'] = pins_from_script(script,
        git(['show', context['source_commit'] + ':scripts/install-native.sh']), context['version'])
    options.context.write_bytes(encoded(validate_context(context)))


class Budget:
    def __init__(self):
        self.begin = time.monotonic()
        self.deadline = self.begin + 1320

    def remaining(self, step):
        available = self.deadline - time.monotonic() - 120
        require(available > 0, 'observation_deadline')
        return min(step, available)


class PtyChild:
    def __init__(self, pid):
        self.pid, self.returncode = pid, None

    def poll(self):
        if self.returncode is None:
            child, status = os.waitpid(self.pid, os.WNOHANG)
            if child:
                self.returncode = os.waitstatus_to_exitcode(status)
        return self.returncode

    def wait(self, timeout):
        until = time.monotonic() + timeout
        while self.poll() is None:
            if time.monotonic() >= until:
                raise subprocess.TimeoutExpired('owned_pty', timeout)
            time.sleep(0.02)
        return self.returncode


def terminal(arguments, environment, timeout, dialog=quick.DIALOG):
    pid, master = pty.fork()
    if pid == 0:
        try:
            os.execve('/bin/sh', ['/bin/sh', *arguments], environment)
        finally:
            os._exit(127)
    child, output, prompt = PtyChild(pid), bytearray(), 0
    deadline = time.monotonic() + timeout
    try:
        while True:
            require(time.monotonic() < deadline, 'terminal_timeout')
            if select.select([master], [], [], 0.05)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
                    data = b''
                output.extend(data)
                require(len(output) <= supervisor.MAX_CAPTURE, 'terminal_output_limit')
                while prompt < len(dialog) and dialog[prompt][0] in output:
                    os.write(master, dialog[prompt][1])
                    prompt += 1
            if child.poll() is not None:
                # Drain bytes already buffered by our owned terminal; no deadline reset.
                while select.select([master], [], [], 0)[0]:
                    require(time.monotonic() < deadline, 'terminal_timeout')
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not data:
                        break
                    output.extend(data)
                    require(len(output) <= supervisor.MAX_CAPTURE, 'terminal_output_limit')
                break
        require(child.returncode == 0 and prompt == len(dialog), 'terminal_failed')
        return bytes(output)
    finally:
        try:
            supervisor.stop(child)
        finally:
            os.close(master)


def target_host():
    pair = (platform.system(), platform.machine())
    choices = {('Linux', 'x86_64'): TARGETS[0], ('Linux', 'aarch64'): TARGETS[1],
        ('Darwin', 'arm64'): TARGETS[2]}
    require(pair in choices, 'host_target')
    return choices[pair]


def setup_contract(value, repeated=False, original=None):
    require(type(value) is dict and value.get('schema') == 1
        and value.get('role') == 'both' and value.get('inbound_enabled') is False
        and value.get('backend') == 'file'
        and all(value.get(key) is True for key in ('engine_stop_complete', 'mdk_stop_complete',
            'runtime_cleanup_complete')), 'setup_contract')
    require(value.get('identity_created') is (not repeated)
        and value.get('owner_opened') is (not repeated), 'setup_identity_contract')
    if repeated:
        require(value.get('npub') == original.get('npub') and value.get('config_changed') is False,
            'repeat_setup_contract')
    else:
        require(isinstance(value.get('npub'), str) and value['npub'].startswith('npub1'), 'setup_identity_contract')


def check_manifest(raw, context, target, directory):
    require(sha(raw) == context['manifest_pins'][target] and raw.endswith(b'\n'), 'manifest_digest')
    lines = raw.decode('ascii').splitlines()
    require(len(lines) == 12 and lines[0] == 'marmish-manifest 1' and lines[11] == 'end'
        and lines[1] == 'version ' + context['version'] and lines[2] == 'target ' + target
        and lines[3] == 'commit ' + context['source_commit'], 'manifest_binding')
    require(re.fullmatch(r'signing (none|adhoc|developer-id) notarized=(yes|no)', lines[4])
        and re.fullmatch(r'minimum (glibc|macos)=[0-9]+\.[0-9]+', lines[5]), 'manifest_metadata')
    values = lines[6].split(' ')
    require(len(values) == 6 and values[0] == 'state', 'manifest_formats')
    formats = []
    for token, name in zip(values[1:], ('profile', 'journal', 'engine', 'config', 'commands')):
        require(re.fullmatch(name + r'=[1-9][0-9]{0,9}', token), 'manifest_formats')
        value = int(token.split('=')[1])
        require(value <= 4294967295, 'manifest_formats')
        formats.append(value)
    for line, name in zip(lines[7:11], FILES):
        parts = line.split(' ')
        mode = '0444' if name == FILES[-1] else '0555'
        require(len(parts) == 5 and parts[0] == 'file' and parts[4] == name
            and parts[3] == mode, 'manifest_files')
        hex_value(parts[1], 64)
        decimal(parts[2])
        maximum = 16777216 if name == FILES[-1] else 268435456
        require(int(parts[2]) <= maximum, 'manifest_file_size')
        path = directory / name
        info = path.lstat()
        require(stat.S_ISREG(info.st_mode) and stat.S_IMODE(info.st_mode) == int(mode, 8)
            and info.st_size == int(parts[2]), 'installed_file_metadata')
        with path.open('rb') as stream:
            hasher = hashlib.sha256()
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                hasher.update(block)
            digest = hasher.hexdigest()
        require(digest == parts[1], 'installed_file_digest')
    return formats


def release_contract(value, context, target, formats):
    require(value == {'schema': 1, 'version': context['version'], 'target': target,
        'commit': context['source_commit'], 'formats': formats}, 'release_provenance')


def status_contract(value, context, target, prefix, bins):
    require(value == {'schema': 1, 'operation': 'status', 'version': context['version'],
        'target': target, 'active': True, 'generation': 1, 'prefix': str(prefix),
        'bin_dir': str(bins)}, 'install_status')
    for name in ('marmish', 'marmishd'):
        require((bins / name).is_symlink() and os.readlink(bins / name) == str(prefix / 'current' / name),
            'installed_launcher')


def safe_version(prefix, bins):
    launcher = bins / 'marmish'
    require(launcher.is_symlink(), 'installed_launcher')
    resolved = launcher.resolve(strict=True)
    versions = (prefix / 'versions').resolve(strict=True)
    require(resolved.name == 'marmish' and resolved.parent.parent == versions
        and not resolved.is_symlink(), 'unsafe_launcher_target')
    return resolved.parent


def fetch_script(url, path, environment, budget):
    supervisor.execute(['curl', '-q', '--proto', '=https', '--proto-redir', '=https', '--tlsv1.2',
        '-fsSL', '--connect-timeout', '20', '--max-time', '60', '--max-filesize', '65536',
        '--output', str(path), url], environment, timeout=budget.remaining(65))
    return bounded_read(path, 65536)


def cell(options):
    begin, root = time.monotonic(), None
    result = {key: False for key in BOOL_KEYS}
    result.update(schema=1, kind='native_https_installer', context_sha256='',
        qualification_run='', qualification_attempt='', source_commit='', bootstrap_commit='',
        source=options.source, target=options.target, ok=False, stage='context', failure=None,
        bootstrap_sha256='', manifest_sha256='', elapsed_ms=0)
    try:
        context, digest = load_context(options.context)
        require(options.source in SOURCES and options.target == target_host(), 'cell_target')
        require(context['qualification_run'] == os.environ.get('GITHUB_RUN_ID')
            and context['qualification_attempt'] == os.environ.get('GITHUB_RUN_ATTEMPT')
            and context['workflow_commit'] == os.environ.get('GITHUB_SHA'), 'cell_run_binding')
        result.update({key: context[key] for key in ('qualification_run', 'qualification_attempt',
            'source_commit', 'bootstrap_commit', 'bootstrap_sha256')})
        result['context_sha256'] = digest
        budget = Budget()
        account = pwd.getpwuid(os.geteuid())
        root = Path(tempfile.mkdtemp(prefix='.mh-', dir=account.pw_dir))
        root.chmod(0o700)
        utilities, temporary = root / 'u', root / 't'
        utilities.mkdir(mode=0o700)
        temporary.mkdir(mode=0o700)
        require(len(str(temporary).encode()) + 53 < 104, 'runtime_path_length')
        for name in quick.UTILITIES:
            executable = shutil.which(name)
            if executable:
                (utilities / name).symlink_to(executable)
        environment = {'HOME': account.pw_dir, 'USER': account.pw_name, 'LOGNAME': account.pw_name,
            'PATH': str(utilities), 'LC_ALL': 'C', 'TMPDIR': str(temporary)}
        prefix, bins, state = (root / name for name in ('p', 'b', 's'))
        result['stage'] = 'download'
        scripts = {}
        for source, url in SOURCES.items():
            path = root / (source + '.sh')
            scripts[source] = fetch_script(url, path, environment, budget)
            require(sha(scripts[source]) == context['bootstrap_sha256'], 'source_digest')
        require(scripts['iris'] == scripts['github'], 'source_equality')
        result['both_sources_verified'] = True
        result['stage'] = 'install'
        output = terminal([str(root / (options.source + '.sh')), '--quickstart', '--debug',
            '--key-storage', 'file', '--prefix', str(prefix), '--bin-dir', str(bins),
            '--state-dir', str(state)], environment, budget.remaining(1020))
        result['install_verified'] = True
        result['stage'] = 'setup'
        initial = [value for value in quick.reports(output) if 'identity_created' in value]
        require(len(initial) == 1, 'setup_report_count')
        setup_contract(initial[0])
        result['setup_verified'] = True
        result['stage'] = 'manifest'
        directory = safe_version(prefix, bins)
        raw = bounded_read(directory / 'manifest', 4096)
        formats = check_manifest(raw, context, options.target, directory)
        result['manifest_sha256'] = sha(raw)
        result['stage'] = 'provenance'
        for name in FILES[:3]:
            _, raw = supervisor.execute([str(directory / name), 'release-info'], environment,
                timeout=budget.remaining(30))
            release_contract(decode(raw), context, options.target, formats)
        cli = str(bins / 'marmish')
        _, raw = supervisor.execute([cli, '--version'], environment, timeout=budget.remaining(15))
        require(raw == ('marmish-native ' + context['version'] + '\n').encode(), 'installed_version')
        _, raw = supervisor.execute([cli, '--help'], environment, timeout=budget.remaining(15))
        require(b'Usage:' in raw, 'installed_help')
        result['provenance_verified'] = True
        result['stage'] = 'status'
        _, raw = supervisor.execute([cli, 'install-status', '--prefix', str(prefix), '--bin-dir',
            str(bins)], environment, timeout=budget.remaining(30))
        status_contract(decode(raw), context, options.target, prefix, bins)
        result['status_verified'] = True
        result['stage'] = 'repeat'
        _, raw = supervisor.execute([cli, '--state-dir', str(state), '--key-storage', 'file',
            'setup', '--no-grants'], environment, timeout=budget.remaining(45))
        setup_contract(decode(raw), True, initial[0])
        result['repeat_verified'] = True
        result['owned_processes_stopped'] = True  # All process groups checked by both supervisors.
        result['stage'] = 'cleanup'
        descriptor = os.open(state / 'default' / 'owner.lock', os.O_RDONLY | os.O_NOFOLLOW)
        try:
            require(stat.S_ISREG(os.fstat(descriptor).st_mode), 'profile_lock_type')
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
            result['profile_lease_released'] = True
            require(not list(temporary.glob('mr2-*')), 'runtime_remaining')
            result['runtime_removed'] = True
            shutil.rmtree(root)
            result['owned_state_removed'] = not root.exists()
            require(result['owned_state_removed'], 'owned_state_remaining')
        finally:
            os.close(descriptor)
        result.update(ok=True, stage='complete')
    except Exception as error:
        # Refused contains only our finite labels. No raw subprocess/provider output escapes.
        result['failure'] = str(error) if isinstance(error, Refused) else 'validation_failed'
    result['elapsed_ms'] = round((time.monotonic() - begin) * 1000)
    options.report.write_bytes(encoded(result))
    return 0 if result['ok'] else 1


def rollup(context_path, directory, output):
    context, digest = load_context(context_path)
    paths = list(directory.rglob('*.json'))
    require(len(paths) == 6, 'cell_report_count')
    cells = set()
    for path in paths:
        value = decode(bounded_read(path, 8192))
        require(type(value) is dict and set(value) == REPORT_KEYS, 'cell_report_schema')
        pair = (value['target'], value['source'])
        require(pair in {(target, source) for target in TARGETS for source in SOURCES}
            and pair not in cells, 'cell_report_duplicate')
        cells.add(pair)
        require(value['schema'] == 1 and value['kind'] == 'native_https_installer'
            and value['context_sha256'] == digest
            and all(value[key] == context[key] for key in ('qualification_run', 'qualification_attempt',
                'source_commit', 'bootstrap_commit', 'bootstrap_sha256'))
            and value['manifest_sha256'] == context['manifest_pins'][value['target']]
            and value['stage'] == 'complete' and value['failure'] is None
            and value['ok'] is True and all(value[key] is True for key in BOOL_KEYS)
            and type(value['elapsed_ms']) is int and 0 <= value['elapsed_ms'] <= 1320000,
            'cell_report_failed_or_foreign')
    output.write_bytes(encoded({'schema': 1, 'kind': 'native_https_rollup', 'ok': True,
        'context_sha256': digest, 'cells': 6, 'qualification_run': context['qualification_run'],
        'qualification_attempt': context['qualification_attempt']}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    resolve_parser = commands.add_parser('resolve')
    for key in CONTEXT_KEYS - {'schema', 'manifest_pins'}:
        resolve_parser.add_argument('--' + key.replace('_', '-'), required=True)
    resolve_parser.add_argument('--output', required=True, type=Path)
    binder = commands.add_parser('bind')
    binder.add_argument('--context', required=True, type=Path)
    binder.add_argument('--artifact', required=True, type=Path)
    runner = commands.add_parser('cell')
    runner.add_argument('--context', required=True, type=Path)
    runner.add_argument('--source', required=True, choices=tuple(SOURCES))
    runner.add_argument('--target', required=True, choices=TARGETS)
    runner.add_argument('--report', required=True, type=Path)
    roll = commands.add_parser('rollup')
    roll.add_argument('--context', required=True, type=Path)
    roll.add_argument('--reports', required=True, type=Path)
    roll.add_argument('--output', required=True, type=Path)
    options = parser.parse_args()
    try:
        if options.command == 'resolve':
            resolve(options)
        elif options.command == 'bind':
            bind_context(options)
        elif options.command == 'cell':
            return cell(options)
        else:
            rollup(options.context, options.reports, options.output)
        return 0
    except Exception as error:
        category = str(error) if isinstance(error, Refused) else 'validation_failed'
        failure = {'schema': 1, 'ok': False, 'failure': category}
        if options.command == 'rollup':
            options.output.write_bytes(encoded(failure))
        print(json.dumps(failure))
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
