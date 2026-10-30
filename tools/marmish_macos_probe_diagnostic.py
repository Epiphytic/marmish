#!/usr/bin/env python3
"""Synthetic macOS probe diagnostics; never opens identities or Keychain state."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import pwd
import socket
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unicodedata


ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / 'tools/native/marmish-probe.c'
FLAGS = ['-Wall', '-Wextra', '-Werror', '-O2', '-mmacosx-version-min=15.0']
LIMIT = 4096
ENV = {'PATH': '/usr/bin:/bin'}


def redact_stderr(raw, replacements):
    """Redact before bounding, including a token split by the capture boundary."""
    truncated = len(raw) > LIMIT
    value = raw.decode('utf-8', errors='replace')
    value = ''.join(c for c in value if c in '\n\t' or not unicodedata.category(c).startswith('C'))
    tokens = sorted(((str(token), label) for token, label in replacements if token),
                    key=lambda pair: len(pair[0]), reverse=True)
    if truncated:
        # Remove a possibly partial sensitive suffix before full-token replacements.
        for token, label in tokens:
            overlap = next((n for n in range(min(len(token), len(value)), 0, -1)
                            if value.endswith(token[:n])), 0)
            if overlap:
                value = value[:-overlap] + label
    for token, label in tokens:
        value = value.replace(token, label)
    encoded = value.encode('utf-8')
    return encoded[:LIMIT].decode('utf-8', errors='ignore'), truncated or len(encoded) > LIMIT


def replacements_for(root):
    values = [(str(root), '<fixture>'), (str(Path.home()), '<home>'),
              (str(ROOT), '<checkout>'), (str(Path.cwd()), '<workspace>'),
              (tempfile.gettempdir(), '<temp>'),
              (str(Path(tempfile.gettempdir()).resolve()), '<temp>'),
              (pwd.getpwuid(os.getuid()).pw_name, '<user>')]
    # macOS aliases /tmp and /var can appear in loader error messages.
    values.extend([(str(root).replace('/private/tmp/', '/tmp/'), '<fixture>')])
    return values


def run_case(case_id, command, redactions, control):
    result = {'id': case_id, 'exit_code': None, 'signal': None,
              'timeout': False, 'category': 'completed', 'control': control,
              'stderr': '', 'stderr_truncated': False}
    with tempfile.TemporaryFile() as capture:
        try:
            completed = subprocess.run(command, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=capture,
                                       timeout=5, close_fds=True, env=ENV)
            result['exit_code'] = completed.returncode
            if completed.returncode < 0:
                result['signal'] = -completed.returncode
        except subprocess.TimeoutExpired:
            result['timeout'] = True
            result['category'] = 'timeout'
        except OSError:
            result['category'] = 'launch_failed'
        capture.seek(0)
        result['stderr'], result['stderr_truncated'] = redact_stderr(capture.read(LIMIT + 1), redactions)
    return result


def succeeded(result):
    return result['category'] == 'completed' and result['exit_code'] == 0


def build_probe(probe):
    commands = [('compile', ['clang', *FLAGS, str(SOURCE), '-o', str(probe)]),
                ('sign', ['/usr/bin/codesign', '--force', '--sign', '-', str(probe)]),
                ('verify', ['/usr/bin/codesign', '--verify', '--strict', str(probe)])]
    for stage, command in commands:
        try:
            completed = subprocess.run(command, stdin=subprocess.DEVNULL,
                                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                       timeout=30, close_fds=True, env=ENV)
        except subprocess.TimeoutExpired:
            return {'category': stage + '_timeout', 'exit_code': None}
        except OSError:
            return {'category': stage + '_unavailable', 'exit_code': None}
        if completed.returncode:
            return {'category': stage + '_failed', 'exit_code': completed.returncode}
    return None


def make_fixture(root, address):
    # Lazy import permits fixed refusal reporting on pre-3.11 Python installations.
    from marmish_transport_macos import DarwinConfinedDaemon, PROFILE
    daemon = DarwinConfinedDaemon.__new__(DarwinConfinedDaemon)
    daemon.home = root / 'home'
    daemon._directory = root / 'control'
    daemon.home.mkdir(mode=0o700)
    daemon._directory.mkdir(mode=0o700)
    daemon.socket = daemon.home / 's'
    daemon._broker = SimpleNamespace(address=address)
    profile = daemon._directory / 'profile.sb'
    profile.write_bytes(PROFILE.encode('utf-8'))
    profile.chmod(0o600)
    if profile.read_bytes() != PROFILE.encode('utf-8'):
        raise ValueError('profile_bytes_changed')
    return daemon, profile


def collect_cases(daemon, probe, redactions):
    port = str(daemon._broker.address[1])
    cases = [('read_control', [str(probe), 'read', '/dev/null'], 'self'),
             ('read_confined', daemon._command(probe, 'read', '/dev/null'), 'read_control'),
             ('tcp_control', [str(probe), 'tcp', '127.0.0.1', port], 'self'),
             ('tcp_confined', daemon._command(probe, 'tcp', '127.0.0.1', port), 'tcp_control'),
             ('system_true_confined', daemon._command('/usr/bin/true'), 'absent')]
    results = []
    controls = {}
    for case_id, command, control in cases:
        result = run_case(case_id, command, redactions, control)
        if control not in ('self', 'absent') and not controls[control]:
            result['category'] = 'control_invalid'
        controls[case_id] = succeeded(result)
        results.append(result)
    return results


def write_report(output, report):
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
    fd = os.open(output, flags, 0o600)
    with os.fdopen(fd, 'w', encoding='utf-8') as stream:
        json.dump(report, stream, sort_keys=True, indent=2)
        stream.write('\n')


def diagnose(output):
    report = {'schema': 1, 'platform': platform.system(),
              'architecture': platform.machine(), 'compiler_flags': FLAGS,
              'profile_sha256': None, 'probe_source_sha256': None,
              'cases': [], 'failure': None, 'ok': False}
    if sys.version_info < (3, 11):
        report['failure'] = {'category': 'python_too_old', 'exit_code': None}
    elif platform.system() != 'Darwin':
        report['failure'] = {'category': 'darwin_required', 'exit_code': None}
    elif os.geteuid() == 0:
        report['failure'] = {'category': 'ordinary_user_required', 'exit_code': None}
    else:
        return diagnose_native(output, report)
    write_report(output, report)
    return 1


def diagnose_native(output, report):
    with tempfile.TemporaryDirectory(prefix='marmish-probe-', dir='/private/tmp') as directory:
        root = Path(directory).resolve()
        root.chmod(0o700)
        resolved_output = output.resolve()
        if root == resolved_output or root in resolved_output.parents:
            raise ValueError('report_outside_fixture_required')
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
                listener.bind(('127.0.0.1', 0))
                listener.listen(8)
                daemon, profile = make_fixture(root, listener.getsockname())
                report['profile_sha256'] = hashlib.sha256(profile.read_bytes()).hexdigest()
                report['probe_source_sha256'] = hashlib.sha256(SOURCE.read_bytes()).hexdigest()
                probe = daemon._directory / 'marmish-probe'
                report['failure'] = build_probe(probe)
                if report['failure'] is None:
                    report['cases'] = collect_cases(daemon, probe, replacements_for(root))
                    report['ok'] = all(succeeded(case) for case in report['cases'])
        except (OSError, ValueError, AssertionError, ImportError):
            report['failure'] = {'category': 'fixture_failed', 'exit_code': None}
        # Report survives all owned fixture cleanup; no raw captures leave the fixture.
        write_report(output, report)
    return 0 if report['ok'] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        return diagnose(args.output.absolute())
    except (OSError, ValueError):
        print('marmish probe diagnostic: report_or_cleanup_failed', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
