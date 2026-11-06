#!/usr/bin/env python3
"""Actual rsync/Marmot qualification; disposable native identities, explicit relay.

Uses the stream qualification harness's ownership, private logs and cleanup.
Python is test tooling only. Products and installation remain native Rust.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import time

from marmish_stream_runtime import Qualification, QualificationError, main


def tree_digest(root: Path) -> tuple[str, int, int]:
    """Independent literal-name/content/link oracle; never follow directory links."""
    digest = hashlib.sha256()
    total = count = 0
    def inaccessible(_error):
        raise QualificationError('tree', 'unreadable_subtree')

    for directory, directories, files in os.walk(root, followlinks=False, onerror=inaccessible):
        directories.sort()
        for name in sorted(directories + files):
            path = Path(directory) / name
            relative = os.fsencode(path.relative_to(root))
            if path.is_symlink():
                kind, content = b'L', os.fsencode(os.readlink(path))
            elif path.is_dir():
                kind, content = b'D', b''
            elif path.is_file():
                kind, content = b'F', path.read_bytes()
                total += len(content)
            else:
                raise QualificationError('tree', 'unexpected_file_type')
            for value in (relative, kind, content):
                digest.update(len(value).to_bytes(8, 'big'))
                digest.update(value)
            count += 1
    return digest.hexdigest(), total, count


class RsyncQualification(Qualification):
    def __init__(self, options):
        super().__init__(options)
        self.rsync = shutil.which('rsync')
        self.report['suite'] = 'rsync_native_source'
        self.environment['LC_ALL'] = 'C'
        self.report.pop('latency_samples', None)
        self.report.update(scope='same_host_linux_partial_qualification', not_covered=[
            'denied executable and untrusted identity', 'invalid relay and failed remote rsync',
            'rsync-parent TERM/HUP and direct adapter TERM/HUP',
            'repeated-signal uncertain cancel and another-terminal cancel',
            'EPIPE and SIGKILL parent detach without cancel', 'daemon restart and authority withdrawal',
            'cross-platform and installer runs', 'command latency',
            'direct adapter exit code during outer rsync cancellation',
            'source without trailing slash and outside-directory symlink fixtures',
            'compression ratio and compressed push/pull combinations',
        ])

    def command(self, identity, *arguments):
        if identity == 'rsync':
            return [self.rsync, *arguments]
        return super().command(identity, *arguments)

    def shell(self, compressed=False):
        return shlex.join([
            *self.command('client', '--no-autostart', '--relay', self.options.relay),
            'rsh', *(['--compress'] if compressed else []),
        ])

    def transfer(self, stage, source, destination, *, compressed=False, extra=(), expected=None):
        before = time.monotonic()
        result = self.run(stage, 'rsync', '-as', '--checksum', '--stats', '-e', self.shell(compressed),
                          *extra, '--', str(source), str(destination))
        self.require_success(result, stage)
        elapsed = time.monotonic() - before
        item = {'case': stage, 'seconds': round(elapsed, 6), 'exit': result.returncode,
                'marmish_compression': compressed, 'rsync_compression': '-z' in extra}
        if expected is not None:
            target, oracle = expected
            actual = tree_digest(target)
            if actual != oracle:
                raise QualificationError(stage, 'tree_mismatch')
            item.update(sha256=actual[0], payload_bytes=actual[1], entries=actual[2],
                        tree_bytes_per_second=round(actual[1] / elapsed, 1))
        for label, field in ((b'Literal data', 'literal_bytes'), (b'Matched data', 'matched_bytes')):
            match = re.search(rb'^' + label + rb': ([0-9,]+) bytes$', result.stdout, re.MULTILINE)
            if match is None:
                raise QualificationError(stage, 'stats_schema')
            item[field] = int(match[1].replace(b',', b''))
        self.report['results'].append(item)
        return item

    def identify_binary(self):
        super().identify_binary()
        if self.rsync is None:
            raise QualificationError('rsync', 'rsync_unavailable')
        version = self.require_success(self.run('rsync_version', 'rsync', '--version'), 'rsync_version')
        match = re.match(rb'rsync\s+version\s+([0-9]+\.[0-9]+\.[0-9]+)\s+protocol version\s+([0-9]+)', version.stdout)
        if match is None or int(match[1].split(b'.')[0]) < 3:
            raise QualificationError('rsync_version', 'unsupported_rsync')
        self.report['rsync_version'] = match[1].decode('ascii')
        self.report['rsync_protocol'] = int(match[2])
        with Path(self.rsync).open('rb') as binary:
            self.report['rsync_binary_sha256'] = hashlib.file_digest(binary, 'sha256').hexdigest()

    def rsync_cases(self, remote):
        work = self.root / 'work'
        source, destination, pulled = (work / name for name in ('source', 'destination', 'pulled'))
        for path in (source, destination, pulled):
            path.mkdir()
        (source / 'empty-directory').mkdir()
        names = ['empty', 'space value', "single'quote", 'double"quote', 'tab\tname', 'line\nname',
                 'back\\slash', 'dollar$(touch SHOULD_NOT_EXIST)', 'semi;colon', '(parentheses)',
                 'Unicode-🌲', '-leading', 'wild*card?[x]']
        for index, name in enumerate(names):
            (source / name).write_bytes(bytes(range(256)) * index)
        (source / 'large-binary').write_bytes(os.urandom(max(self.options.bytes, 262144 + 8192)))
        (source / 'symlink').symlink_to('large-binary')
        oracle = tree_digest(source)
        self.transfer('push_raw', str(source) + '/', remote + ':destination/', expected=(destination, oracle))
        self.transfer('pull_raw', remote + ':destination/', str(pulled) + '/', expected=(pulled, oracle))
        unchanged = self.transfer('unchanged', str(source) + '/', remote + ':destination/', expected=(destination, oracle))
        if unchanged['literal_bytes'] != 0:
            raise QualificationError('unchanged', 'unexpected_literal_transfer')
        with (source / 'large-binary').open('r+b') as changed:
            changed.seek(min(1024, self.options.bytes - 1))
            changed.write(b'CHANGED')
        oracle = tree_digest(source)
        delta = self.transfer('delta_zlib', str(source) + '/', remote + ':destination/', compressed=True,
                      expected=(destination, oracle))
        if delta['matched_bytes'] == 0 or delta['literal_bytes'] >= (source / 'large-binary').stat().st_size // 2:
            raise QualificationError('delta_zlib', 'delta_not_observed')
        self.transfer('pull_rsync_z', remote + ':destination/', str(pulled) + '/', extra=('-z',),
                      expected=(pulled, oracle))
        (destination / 'owned-extra').write_bytes(b'delete only within disposable destination')
        self.transfer('delete', str(source) + '/', remote + ':destination/', extra=('--delete',),
                      expected=(destination, oracle))
        for root in (work, source, destination, pulled):
            if (root / 'SHOULD_NOT_EXIST').exists():
                raise QualificationError('literal_names', 'unexpected_shell_evaluation')
        # Capture unsupported bootstrap and missing executable independently of a
        # successful transfer; neither may silently fall back to a shell.
        for stage, category, argv in (
            ('reject_old_bootstrap', b'rsync requires secluded arguments', ('rsh', remote, 'rsync', '--server', '-logDtpre.iLsfxCIvu', '.', 'x')),
            ('missing_remote_exec', b'NotFound', ('rsh', remote, '/marmish-test-nonexistent/rsync', '--server', '-s')),
        ):
            result = self.run(stage, 'client', '--no-autostart', '--relay', self.options.relay, *argv)
            reached_open = re.search(rb'marmish stream: [0-9a-f]{32}', result.stderr) is not None
            if (result.returncode != 255 or result.stdout or category not in result.stderr
                    or reached_open != (stage == 'missing_remote_exec')):
                (self.root / (stage + '.stderr')).write_bytes(result.stderr)
                raise QualificationError(stage, 'unexpected_failure_category')
            self.report['results'].append({'case': stage, 'exit': result.returncode})
        self.cancel_rsync(remote)

    def cancel_rsync(self, remote):
        source = self.root / 'work' / 'cancel-source'
        source.mkdir()
        (source / 'large').write_bytes(os.urandom(max(self.options.bytes, 2 * 1024 * 1024)))
        destination = self.root / 'work' / 'cancel-destination'
        destination.mkdir()
        process = self.spawn('rsync', '-as', '--partial', '--bwlimit=64', '-e', self.shell(), '--',
                             str(source) + '/', remote + ':cancel-destination/')
        deadline = time.monotonic() + min(180, self.options.timeout_seconds)
        while not any(path.is_file() and path.stat().st_size >= 65536 for path in destination.iterdir()):
            if process.poll() is not None:
                raise QualificationError('rsync_cancel', 'ended_before_interrupt')
            if time.monotonic() >= deadline:
                raise QualificationError('rsync_cancel', 'transfer_start_timeout')
            time.sleep(0.1)
        before = time.monotonic()
        # One terminal-like group interrupt reaches rsync and the adapter; rsync
        # subsequently forwards USR1. All of these must mean one cancellation.
        os.killpg(process.pid, signal.SIGINT)
        try:
            _, stderr = process.communicate(timeout=min(90, self.options.timeout_seconds))
        except subprocess.TimeoutExpired as error:
            raise QualificationError('rsync_cancel', 'confirmation_timeout') from error
        if process.returncode == 0 or b'terminal state confirmed' not in stderr:
            (self.root / 'rsync_cancel.stderr').write_bytes(stderr)
            raise QualificationError('rsync_cancel', 'cancellation_unconfirmed')
        self.report['results'].append({'case': 'rsync_group_interrupt', 'exit': process.returncode,
                                       'confirmed': True, 'seconds': round(time.monotonic() - before, 6)})
        partial = destination / 'large'
        if not partial.is_file() or not 0 < partial.stat().st_size < (source / 'large').stat().st_size:
            raise QualificationError('rsync_cancel', 'partial_not_reusable')
        self.report['results'][-1]['partial_bytes'] = partial.stat().st_size
        # A new rsync process may reuse partial files but must create a new stream.
        restart = self.transfer('partial_restart', str(source) + '/', remote + ':cancel-destination/',
                      extra=('--partial',), expected=(destination, tree_digest(source)))
        if restart['matched_bytes'] == 0 or restart['literal_bytes'] >= (source / 'large').stat().st_size:
            raise QualificationError('partial_restart', 'partial_reuse_not_observed')


if __name__ == '__main__':
    raise SystemExit(main(RsyncQualification, lambda qualification, remote: qualification.rsync_cases(remote)))
