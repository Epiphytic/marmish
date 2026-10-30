#!/usr/bin/env python3
"""Extract bounded crash facts only for an owned synthetic macOS probe child."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time
import uuid

import marmish_macos_probe_diagnostic as base

MAX_IPS = 2 * 1024 * 1024
ASI_KEYS = ('dyld', 'libsystem_secinit.dylib', 'libsystem_c.dylib',
            'libsystem_kernel.dylib', 'libc++abi.dylib', 'libsystem_sandbox.dylib')


def bounded(value, redactions, limit):
    if not isinstance(value, str):
        return None
    raw = value.encode('utf-8', errors='replace')
    if len(raw) > 65536:
        return '<oversized>'
    return base.redact_stderr(raw, redactions)[0][:limit]


def extract_named_fields(source, prefix, fields, redactions):
    result = {}
    if isinstance(source, dict):
        for field, limit in fields:
            value = bounded(source.get(field), redactions, limit)
            if value is not None:
                result[prefix + field] = value
    return result


def extract_termination(termination, redactions):
    result = extract_named_fields(termination, 'termination_',
                                  (('namespace', 256), ('indicator', 256)), redactions)
    if not isinstance(termination, dict):
        return result
    code = termination.get('code')
    if type(code) is int and -(2 ** 63) <= code < 2 ** 64:
        result['termination_code'] = code
    else:
        value = bounded(code, redactions, 128)
        if value is not None:
            result['termination_code'] = value
    details = termination.get('details')
    if isinstance(details, list):
        details = '\n'.join(item for item in details[:6] if isinstance(item, str) and len(item) <= 65536)
    value = bounded(details, redactions, 1024)
    if value is not None:
        result['termination_details'] = value
    return result


def extract_asi(asi, redactions):
    result = []
    if not isinstance(asi, dict):
        return result
    for library in ASI_KEYS:
        values = asi.get(library, [])
        if isinstance(values, str):
            values = [values]
        if not isinstance(values, list):
            continue
        for value in values:
            text = bounded(value, redactions, 1024)
            if text is not None:
                result.append({'library': library, 'value': text})
            if len(result) == 6:
                return result
    return result


def extract_crash_facts(payload, redactions):
    result = extract_named_fields(payload.get('exception'), 'exception_',
                                  (('type', 256), ('signal', 128)), redactions)
    result.update(extract_termination(payload.get('termination'), redactions))
    result['asi'] = extract_asi(payload.get('asi'), redactions)
    result['frames'] = extract_frames(payload, redactions)
    return result


def extract_frames(payload, redactions):
    result = []
    threads, images, index = payload.get('threads'), payload.get('usedImages'), payload.get('faultingThread')
    if not (isinstance(threads, list) and isinstance(images, list) and type(index) is int
            and 0 <= index < len(threads) and isinstance(threads[index], dict)):
        return result
    frames = threads[index].get('frames', [])
    if not isinstance(frames, list):
        return result
    for frame in frames[:24]:
        if not isinstance(frame, dict):
            continue
        symbol = bounded(frame.get('symbol'), redactions, 256)
        image_index = frame.get('imageIndex')
        image = None
        if type(image_index) is int and 0 <= image_index < len(images) and isinstance(images[image_index], dict):
            name = images[image_index].get('name') or images[image_index].get('path')
            if isinstance(name, str) and len(name) <= 65536:
                image = bounded(name.rsplit('/', 1)[-1], redactions, 256)
        result.append({'symbol': symbol, 'image': image})
    return result


def open_reports(directory):
    try:
        fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    except FileNotFoundError:
        return None, 'reports_dir_absent'
    except OSError:
        return None, 'reports_dir_unavailable'
    if os.fstat(fd).st_uid != os.getuid():
        os.close(fd)
        return None, 'reports_dir_unowned'
    return fd, None


def candidates(fd, name):
    return {entry for entry in os.listdir(fd)
            if (entry.startswith(name) or entry.startswith('sandbox-exec-')) and entry.endswith('.ips')}


def snapshot(directory, name):
    fd, category = open_reports(directory)
    if fd is None:
        return set(), category
    try:
        return candidates(fd, name), 'available'
    finally:
        os.close(fd)


def parse_macho_uuid(raw, architecture):
    cpu = {'arm64': 0x0100000c, 'x86_64': 0x01000007}.get(architecture)
    if not 32 <= len(raw) <= 4096 or cpu is None:
        raise ValueError('probe_uuid_invalid')
    magic, cpu_type, _, _, count, size, _, _ = struct.unpack_from('<8I', raw)
    if magic != 0xfeedfacf or cpu_type != cpu or not 1 <= count <= 128 or size > len(raw) - 32:
        raise ValueError('probe_uuid_invalid')
    cursor, found, end = 32, [], 32 + size
    for _ in range(count):
        if cursor + 8 > end:
            raise ValueError('probe_uuid_invalid')
        command, length = struct.unpack_from('<2I', raw, cursor)
        if length < 8 or length % 8 or cursor + length > end:
            raise ValueError('probe_uuid_invalid')
        if command == 0x1b:
            if length != 24:
                raise ValueError('probe_uuid_invalid')
            found.append(uuid.UUID(bytes=raw[cursor + 8:cursor + 24]))
        cursor += length
    if cursor != end or len(found) != 1 or found[0].int == 0:
        raise ValueError('probe_uuid_invalid')
    return found[0], architecture


def read_owned_uuid(probe):
    fd = os.open(probe, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
            raise ValueError('probe_uuid_invalid')
        raw = os.read(fd, 4096)
    finally:
        os.close(fd)
    return parse_macho_uuid(raw, platform.machine())


def uuid_image_matches(payload, probe, identity):
    images = payload.get('usedImages')
    if not isinstance(images, list):
        return False
    matching = [image for image in images if isinstance(image, dict) and image.get('name') == probe.name]
    if len(matching) != 1:
        return False
    image = matching[0]
    path = image.get('path')
    if 'path' in image and (not isinstance(path, str) or len(path) > 4096 or not path.endswith('/' + probe.name)):
        return False
    if (image.get('arch') != identity[1] or not isinstance(image.get('uuid'), str)
            or len(image['uuid']) > 64):
        return False
    try:
        image_uuid = uuid.UUID(image['uuid'])
    except ValueError:
        return False
    return image_uuid.int != 0 and image_uuid == identity[0]


def path_shape(payload, probe, identity):
    value = payload.get('procPath')
    path = value if isinstance(value, str) and len(value) <= 4096 else ''
    parts = path.split('/')[1:] if path.startswith('/') else []
    prefix = next((label for start, label in (('/private/tmp', 'private_tmp'), ('/tmp', 'tmp'), ('/Users', 'users'))
                   if path == start or path.startswith(start + '/')), 'other')
    return {'prefix': prefix, 'component_count': min(len(parts), 8),
            'star_component_count': min(parts.count('*'), 8),
            'basename_match': bool(parts) and parts[-1] == probe.name,
            'proc_name_match': payload.get('procName') == probe.name,
            'alias_match': False, 'uuid_image_match': uuid_image_matches(payload, probe, identity)}


def attribute_executable(payload, probe, identity):
    shape = path_shape(payload, probe, identity)
    value = payload.get('procPath')
    if value == str(probe):
        return 'probe_exact', shape
    if value == '/usr/bin/sandbox-exec':
        return 'sandbox_exec_exact', shape
    if not isinstance(value, str) or len(value) > 4096 or not value.startswith('/') or '\x00' in value:
        return None, shape
    parts = value.split('/')[1:]
    if any(part in ('', '.', '..') for part in parts):
        return None, shape
    if '*' not in value:
        try:
            shape['alias_match'] = Path(value).resolve(strict=True) == probe.resolve(strict=True)
        except (OSError, ValueError, RuntimeError):
            pass
        if shape['alias_match']:
            return 'probe_alias', shape
    patterns = ('/private/tmp/*/', '/tmp/*/', '/private/tmp/*/control/', '/tmp/*/control/')
    if (value in [prefix + probe.name for prefix in patterns] and shape['proc_name_match']
            and shape['uuid_image_match']):
        return 'probe_uuid_redacted', shape
    return None, shape


def read_context(fd, name, pid, probe, redactions, identity):
    try:
        child = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=fd)
    except OSError:
        return {'category': 'candidate_unavailable'}
    try:
        info = os.fstat(child)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
            return {'category': 'candidate_not_owned_regular'}
        if info.st_size > MAX_IPS:
            return {'category': 'candidate_oversized'}
        chunks, remaining = [], MAX_IPS + 1
        while remaining:
            data = os.read(child, min(65536, remaining))
            if not data:
                break
            chunks.append(data)
            remaining -= len(data)
        raw = b''.join(chunks)
    except OSError:
        return {'category': 'candidate_unavailable'}
    finally:
        os.close(child)
    if len(raw) > MAX_IPS:
        return {'category': 'candidate_oversized'}
    try:
        first, rest = raw.split(b'\n', 1)
        metadata, payload = json.loads(first), json.loads(rest)
    except (ValueError, UnicodeError, RecursionError):
        return {'category': 'candidate_parse_failed'}
    if not isinstance(metadata, dict) or not isinstance(payload, dict):
        return {'category': 'candidate_invalid'}
    if type(payload.get('pid')) is not int or payload['pid'] != pid:
        return {'category': 'candidate_pid_rejected'}
    attribution, shape = attribute_executable(payload, probe, identity)
    if attribution is None:
        return {'category': 'candidate_path_rejected', 'path_shape': shape}
    return {'category': 'matched', 'executable': 'sandbox_exec' if attribution == 'sandbox_exec_exact' else 'probe',
            'attribution': attribution, 'path_shape': shape, 'facts': extract_crash_facts(payload, redactions)}


def wait_context(directory, name, before, pid, probe, redactions, identity):
    deadline = time.monotonic() + 20
    seen = set()
    result = {'category': 'context_timed_out', 'candidate_count': 0}
    while True:
        fd, category = open_reports(directory)
        if fd is None:
            result = {'category': category, 'candidate_count': len(seen)}
        else:
            try:
                new = candidates(fd, name) - before
                seen.update(new)
                result['candidate_count'] = len(seen)
                if len(seen) > 8:
                    return {'category': 'candidate_limit', 'candidate_count': len(seen)}
                for candidate in sorted(new):
                    context = read_context(fd, candidate, pid, probe, redactions, identity)
                    result = {'candidate_count': len(seen), **context}
                    if context['category'] == 'matched':
                        return result
            finally:
                os.close(fd)
        if time.monotonic() >= deadline:
            return result
        time.sleep(1)


def run_child(case_id, command, redactions):
    result = {'id': case_id, 'pid': None, 'exit_code': None, 'signal': None,
              'timeout': False, 'category': 'completed', 'stderr': '', 'stderr_truncated': False}
    child = None
    with tempfile.TemporaryFile() as capture:
        try:
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=capture, close_fds=True, env=base.ENV)
            result['pid'] = child.pid
            result['exit_code'] = child.wait(timeout=5)
            if result['exit_code'] < 0:
                result['signal'] = -result['exit_code']
        except subprocess.TimeoutExpired:
            result['timeout'], result['category'] = True, 'timeout'
        except OSError:
            result['category'] = 'launch_failed'
        finally:
            if child is not None and child.poll() is None:
                child.kill()
                child.wait()
        capture.seek(0)
        result['stderr'], result['stderr_truncated'] = base.redact_stderr(capture.read(base.LIMIT + 1), redactions)
    return result


def collect(root, report):
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind(('127.0.0.1', 0))
        listener.listen(1)
        daemon, profile = base.make_fixture(root, listener.getsockname())
        name = 'mp-' + hashlib.sha256(root.name.encode()).hexdigest()[:8]
        probe = daemon._directory / name
        report['profile_sha256'] = hashlib.sha256(profile.read_bytes()).hexdigest()
        report['probe_source_sha256'] = hashlib.sha256(base.SOURCE.read_bytes()).hexdigest()
        report['failure'] = base.build_probe(probe)
        if report['failure'] is not None:
            return
        try:
            identity = read_owned_uuid(probe)
        except (OSError, ValueError):
            report['failure'] = {'category': 'probe_uuid_invalid', 'exit_code': None}
            return
        redactions = base.replacements_for(root)
        control = run_child('read_control', [str(probe), 'read', '/dev/null'], redactions)
        directory = Path.home() / 'Library/Logs/DiagnosticReports'
        before, baseline = snapshot(directory, name)
        confined = run_child('read_confined', daemon._command(probe, 'read', '/dev/null'), redactions)
        report['reports_baseline'] = baseline
        report['context'] = {'category': 'not_sigabrt', 'candidate_count': 0}
        if confined['signal'] == 6:
            report['context'] = wait_context(directory, name, before, confined['pid'], probe, redactions, identity)
        if not base.succeeded(control):
            confined['category'] = 'control_invalid'
        # PIDs are used for selecting owned crash context, never exported.
        control.pop('pid')
        confined.pop('pid')
        report['cases'] = [control, confined]
        report['ok'] = all(base.succeeded(case) for case in report['cases'])


def diagnose(output):
    report = {'schema': 1, 'platform': platform.system(), 'architecture': platform.machine(),
              'compiler_flags': base.FLAGS, 'profile_sha256': None, 'probe_source_sha256': None,
              'cases': [], 'context': {'category': 'unavailable', 'candidate_count': 0},
              'failure': None, 'ok': False}
    category = ('python_too_old' if sys.version_info < (3, 11) else
                'darwin_required' if platform.system() != 'Darwin' else
                'ordinary_user_required' if os.geteuid() == 0 else None)
    if category:
        report['failure'] = {'category': category, 'exit_code': None}
        base.write_report(output, report)
        return 1
    with tempfile.TemporaryDirectory(prefix='marmish-crash-', dir='/private/tmp') as owned:
        root = Path(owned).resolve()
        root.chmod(0o700)
        resolved_output = output.resolve()
        if root == resolved_output or root in resolved_output.parents:
            raise ValueError('report_outside_fixture_required')
        try:
            collect(root, report)
        except (OSError, ValueError, AssertionError, ImportError):
            report['failure'] = {'category': 'fixture_failed', 'exit_code': None}
        base.write_report(output, report)
    return 0 if report['ok'] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        return diagnose(args.output.absolute())
    except (OSError, ValueError):
        print('marmish crash diagnostic: report_or_cleanup_failed', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
