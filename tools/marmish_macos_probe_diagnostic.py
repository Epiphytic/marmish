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
MATRIX_LIMIT = 512
REPORT_LIMIT = 65536
# Diagnostic-only fixed permissions; production never imports these additions.
LOADER_RULES = (
    ('root', '(allow file-read-data (literal "/"))\n'),
    ('map', '(allow file-map-executable (literal (param "EXEC"))\n'
     ' (subpath "/usr/lib") (subpath "/System/Library/Frameworks")\n'
     ' (subpath "/System/Library/PrivateFrameworks") (subpath "/System/Library/dyld"))\n'),
    ('cryptex', '(allow file-read* file-map-executable\n'
     ' (subpath "/System/Cryptexes/OS/System/Library/dyld")\n'
     ' (subpath "/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld")\n'
     ' (subpath "/private/preboot/Cryptexes/OS/System/Library/dyld"))\n'))
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


def loader_profiles():
    from marmish_transport_macos import PROFILE
    for mask in range(8):
        selected = [(name, rule) for bit, (name, rule) in enumerate(LOADER_RULES)
                    if mask & (1 << bit)]
        yield ('+'.join(name for name, _ in selected) or 'baseline',
               [name for name, _ in selected],
               (PROFILE + ''.join(rule for _, rule in selected)).encode('utf-8'))


def matrix_case(case_id, command, redactions, control):
    result = run_case(case_id, command, redactions, control)
    raw = result['stderr'].encode('utf-8')
    result['stderr'] = raw[:MATRIX_LIMIT].decode('utf-8', errors='ignore')
    result['stderr_truncated'] |= len(raw) > MATRIX_LIMIT
    return result


def exact_exit(result, code):
    return (result['category'] == 'completed' and type(result['exit_code']) is int
            and result['exit_code'] == code and result['signal'] == (None if code >= 0 else -code)
            and result['timeout'] is False)


def loader_abort(cases):
    return all(exact_exit(case, -6) for case in cases[:2])


def collect_loader_matrix(daemon, probe, redactions):
    outside = daemon.home.parent / 'outside'
    outside.mkdir(mode=0o700)
    target = outside / 'synthetic'
    fd = os.open(target, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(b'marmish synthetic outside fixture\n')
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as unselected:
        unselected.bind(('127.0.0.1', 0))
        unselected.listen(32)
        specs = [('read', ['read', '/dev/null'], 0),
                 ('tcp', ['tcp', '127.0.0.1', str(daemon._broker.address[1])], 0),
                 ('outside_read', ['read', str(target)], 2),
                 ('outside_write', ['write', str(target)], 2),
                 ('unselected_tcp', ['tcp', '127.0.0.1', str(unselected.getsockname()[1])], 2)]
        controls = [matrix_case(name + '_control', [str(probe), *args], redactions, 'self')
                    for name, args, _ in specs]
        valid = all(exact_exit(case, 0) for case in controls)
        variants = []
        for name, factors, raw in loader_profiles():
            profile = daemon._directory / 'profile.sb'
            fd = os.open(profile, os.O_WRONLY | os.O_TRUNC | os.O_NOFOLLOW)
            with os.fdopen(fd, 'wb') as stream:
                stream.write(raw)
            actual = profile.read_bytes()
            if actual != raw or hashlib.sha256(actual).digest() != hashlib.sha256(raw).digest():
                raise ValueError('profile_bytes_changed')
            cases = [matrix_case(case_id + '_confined', daemon._command(probe, *args),
                                 redactions, case_id + '_control')
                     for case_id, args, _ in specs]
            if not valid:
                for case in cases:
                    case['category'] = 'control_invalid'
            passed = all(exact_exit(case, code) for case, (_, _, code) in zip(cases, specs))
            variants.append({'id': name, 'factors': factors,
                             'profile_sha256': hashlib.sha256(raw).hexdigest(),
                             'cases': cases, 'ok': passed,
                             'classification': 'passed' if passed else
                             'loader_abort' if loader_abort(cases) else 'other'})
    reproduced = loader_abort(variants[0]['cases'])
    return {'controls': controls, 'variants': variants, 'baseline_reproduced': reproduced,
            'ok': valid and reproduced and any(variant['ok'] for variant in variants)}


def write_report(output, report):
    raw = (json.dumps(report, sort_keys=True, indent=2, ensure_ascii=False) + '\n').encode('utf-8')
    if len(raw) > REPORT_LIMIT:
        fallback = {'schema': report['schema'], 'ok': False,
                    'failure': {'category': 'report_too_large', 'exit_code': None}}
        if 'mode' in report:
            fallback['mode'] = report['mode']
        report.clear()
        report.update(fallback)
        raw = (json.dumps(report, sort_keys=True) + '\n').encode('utf-8')
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
    fd = os.open(output, flags, 0o600)
    with os.fdopen(fd, 'wb') as stream:
        stream.write(raw)


def diagnose(output, *, loader_matrix=False):
    report = {'schema': 1, 'platform': platform.system(),
              'architecture': platform.machine(), 'compiler_flags': FLAGS,
              'profile_sha256': None, 'probe_source_sha256': None,
              'cases': [], 'failure': None, 'ok': False}
    if loader_matrix:
        report.update({'schema': 2, 'mode': 'loader-matrix', 'base_profile_sha256': None,
                       'controls': [], 'variants': [], 'baseline_reproduced': False})
        del report['profile_sha256'], report['cases']
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
                listener.listen(32)
                daemon, profile = make_fixture(root, listener.getsockname())
                matrix = report.get('mode') == 'loader-matrix'
                report['base_profile_sha256' if matrix else 'profile_sha256'] = hashlib.sha256(profile.read_bytes()).hexdigest()
                report['probe_source_sha256'] = hashlib.sha256(SOURCE.read_bytes()).hexdigest()
                probe = daemon._directory / 'marmish-probe'
                report['failure'] = build_probe(probe)
                if report['failure'] is None:
                    if matrix:
                        report.update(collect_loader_matrix(daemon, probe, replacements_for(root)))
                    else:
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
    parser.add_argument('--loader-matrix', action='store_true')
    args = parser.parse_args(argv)
    try:
        return diagnose(args.output.absolute(), loader_matrix=True) if args.loader_matrix else diagnose(args.output.absolute())
    except (OSError, ValueError):
        print('marmish probe diagnostic: report_or_cleanup_failed', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
