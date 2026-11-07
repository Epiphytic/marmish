#!/usr/bin/env python3
"""Bounded native copy/Marmot qualification using disposable owned identities.

Python is test tooling only. Never publish private fixture paths or raw stderr.
"""
from __future__ import annotations

import hashlib
import os
import re
import time

from marmish_stream_runtime import Qualification, QualificationError, main


class CopyQualification(Qualification):
    def __init__(self, options):
        super().__init__(options)
        self.report.pop('latency_samples', None)
        self.oracles = {}
        self.report['payload_bytes'] = options.bytes
        self.report['payload_scope'] = ('default_window_crossing' if options.bytes >= 2 * 1024 * 1024 + 8192
                                        else 'small_smoke')
        self.report.update(suite='native_copy_source', scope='same_host_native_marmot',
                           not_covered=['cross-platform installer', 'Tor', 'daemon restart',
                                        'live interrupted durable resume', 'live copy cancellation',
                                        'retention garbage collection'])

    def transfer(self, stage, remote, direction, source, destination, *, compressed=False,
                 extra=(), expected_error=None):
        expected = None if expected_error is not None else self.oracles.setdefault(source, source.read_bytes())
        before = time.monotonic()
        result = self.run(stage, 'client', '--no-autostart', '--relay', self.options.relay,
                          direction, *(['--compress'] if compressed else []), *extra,
                          '--', remote, str(source), str(destination))
        elapsed = time.monotonic() - before
        # Retain diagnostics privately even for an exit-0 assertion failure.
        (self.root / (stage + '.stderr')).write_bytes(result.stderr)
        if expected_error is not None:
            if (result.returncode != 1 or expected_error not in result.stderr or result.stdout
                    or b'committed and verified' in result.stderr):
                (self.root / (stage + '.stderr')).write_bytes(result.stderr)
                raise QualificationError(stage, 'unexpected_failure_category')
            self.report['results'].append({'case': stage, 'exit': result.returncode,
                                           'expected_error': expected_error.decode('ascii'),
                                           'seconds': round(elapsed, 6)})
            return
        self.require_success(result, stage)
        negotiated = b'zlib' if compressed else b'none'
        if b'marmish copy: transport codec ' + negotiated + b'\n' not in result.stderr:
            raise QualificationError(stage, 'negotiated_codec_missing')
        if result.stdout or not re.search(rb'committed and verified transfer [0-9a-f]{64}', result.stderr):
            raise QualificationError(stage, 'commit_confirmation_missing')
        if not destination.is_file():
            raise QualificationError(stage, 'destination_missing')
        original = source.read_bytes()
        actual = destination.read_bytes()
        if actual != expected or original != expected:
            raise QualificationError(stage, 'content_mismatch')
        self.oracles[destination] = expected
        self.report['results'].append({'case': stage, 'exit': result.returncode,
                                       'compression': compressed, 'payload_bytes': len(actual),
                                       'negotiated_codec': negotiated.decode('ascii'),
                                       'sha256': hashlib.sha256(actual).hexdigest(),
                                       'seconds': round(elapsed, 6),
                                       'payload_bytes_per_second': round(len(actual) / elapsed, 1)})

    def copy_cases(self, remote):
        work = self.root / 'work'
        # Every operand is an argv element; shell metacharacters are literal.
        name = "-space 'quote\";$(touch SHOULD_NOT_EXIST)*?[x]\\tab\tline\n🌲"
        source = work / ('source' + name)
        source.write_bytes(os.urandom(self.options.bytes))
        source.chmod(0o640)
        os.utime(source, ns=(1_700_000_000_123456789,) * 2)
        for compressed in (False, True):
            put = work / ('put' + str(compressed) + name)
            get = work / ('get' + str(compressed) + name)
            for direction, original, destination in (('put', source, put), ('get', put, get)):
                stage = direction + ('_zlib' if compressed else '_raw')
                self.transfer(stage, remote, direction, original, destination, compressed=compressed,
                              extra=('--preserve-mode', '--preserve-mtime'))
                if (destination.stat().st_mode & 0o777) != 0o640:
                    raise QualificationError(stage, 'mode_mismatch')
                if (destination.stat().st_mtime_ns != 1_700_000_000_123456789
                        or source.stat().st_mtime_ns != 1_700_000_000_123456789):
                    raise QualificationError(stage, 'mtime_mismatch')
        compressible = work / 'compressible-source'
        compressible.write_bytes(b'marmish compressible bytes\x00\n' * 2048)
        compressed_target = work / 'compressible-put'
        self.transfer('compressible_put_zlib', remote, 'put', compressible,
                      compressed_target, compressed=True)
        self.transfer('compressible_get_zlib', remote, 'get', compressed_target,
                      work / 'compressible-get', compressed=True)
        empty = work / 'empty-source'
        empty.write_bytes(b'')
        self.transfer('empty_put', remote, 'put', empty, work / 'empty-put')
        self.transfer('empty_get', remote, 'get', empty, work / 'empty-get')
        for direction in ('put', 'get'):
            self.transfer(direction + '_missing_source', remote, direction,
                          work / 'missing-source', work / ('missing-' + direction),
                          expected_error=b'file_not_found')
            if (work / ('missing-' + direction)).exists():
                raise QualificationError(direction + '_missing_source', 'destination_created')
            self.transfer(direction + '_existing_destination', remote, direction, source, empty,
                          expected_error=b'transfer_conflict')
            if empty.read_bytes() != b'':
                raise QualificationError(direction + '_existing_destination', 'destination_changed')
        if (work / 'SHOULD_NOT_EXIST').exists():
            raise QualificationError('literal_paths', 'shell_evaluation')


if __name__ == '__main__':
    raise SystemExit(main(CopyQualification, lambda q, remote: q.copy_cases(remote),
                          default_bytes=2 * 1024 * 1024 + 8192))
