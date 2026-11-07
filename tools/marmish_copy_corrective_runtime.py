#!/usr/bin/env python3
"""Rerun selected copy cases with a pinned baseline and explicit deadline margin.

Test tooling only. A PASS covers selected cases, never the complete copy gate.
Only completed baseline rows are eligible; timed-out or missing rows are rejected.
"""
from __future__ import annotations

import hashlib
import json
import math
import os
from pathlib import Path
import re
import stat
import time
from urllib.parse import urlsplit

from marmish_copy_runtime import CopyQualification
from marmish_stream_runtime import QualificationError, arguments, main

CORE_CASES = ('put_raw', 'get_raw', 'put_zlib', 'get_zlib',
              'compressible_put_zlib', 'compressible_get_zlib', 'empty_put', 'empty_get',
              'put_missing_source', 'put_existing_destination',
              'get_missing_source', 'get_existing_destination')
SELECTABLE = tuple(case for case in CORE_CASES
                   if case in {'put_raw', 'get_raw', 'put_zlib', 'get_zlib',
                               'put_existing_destination', 'get_existing_destination'})
HEX64 = re.compile(r'[0-9a-f]{64}')
MTIME = 1_700_000_000_123456789
NAME = "-space 'quote\";$(touch SHOULD_NOT_EXIST)*?[x]\\tab\tline\n🌲"


def file_sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        for block in iter(lambda: source.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate_json_key')
        result[key] = value
    return result


def read_baseline(path):
    # O_NONBLOCK also closes the stat/open race for a substituted FIFO.
    descriptor = os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW)
    with os.fdopen(descriptor, 'rb') as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise ValueError('baseline_not_regular')
        raw = source.read(1024 * 1024 + 1)
    if len(raw) > 1024 * 1024:
        raise ValueError('baseline_too_large')
    value = json.loads(raw, object_pairs_hook=unique_object,
                       parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite')))
    return value, hashlib.sha256(raw).hexdigest()


def baseline_row(case, row, payload_bytes):
    refusal = case.endswith('_existing_destination')
    common = {'case', 'exit', 'seconds'}
    required = common | ({'expected_error'} if refusal else
                         {'compression', 'payload_bytes', 'negotiated_codec', 'sha256',
                          'payload_bytes_per_second'})
    allowed = required | ({'payload_bytes', 'compression', 'negotiated_codec'} if refusal else set())
    if not isinstance(row, dict) or not required <= row.keys() <= allowed:
        raise ValueError('baseline_case_schema')
    if type(row['exit']) is not int or row['exit'] != (1 if refusal else 0):
        raise ValueError('baseline_case_exit')
    seconds = row['seconds']
    if type(seconds) is not float or not math.isfinite(seconds) or not 0 < seconds <= 86400:
        raise ValueError('baseline_case_seconds')
    compressed = case.endswith('_zlib')
    if 'payload_bytes' in row and (type(row['payload_bytes']) is not int
                                   or row['payload_bytes'] != payload_bytes):
        raise ValueError('baseline_case_bytes')
    if 'compression' in row and (type(row['compression']) is not bool
                                 or row['compression'] != compressed):
        raise ValueError('baseline_case_codec')
    if 'negotiated_codec' in row and row['negotiated_codec'] != ('zlib' if compressed else 'none'):
        raise ValueError('baseline_case_codec')
    if refusal:
        if row['expected_error'] != 'transfer_conflict':
            raise ValueError('baseline_case_refusal')
    elif not isinstance(row['sha256'], str) or HEX64.fullmatch(row['sha256']) is None:
        raise ValueError('baseline_case_hash')
    return {'seconds': seconds, 'expected_exit': row['exit'],
            'kind': 'expected_refusal' if refusal else 'successful_copy',
            'payload_binding': 'report_payload_bytes' if refusal else 'case_payload_bytes',
            'codec': 'zlib' if compressed else 'none'}


def validate_baseline(options):
    baseline, digest = read_baseline(options.baseline_report)
    if not isinstance(baseline, dict) or type(baseline.get('schema')) is not int or baseline['schema'] != 1:
        raise ValueError('baseline_schema')
    if baseline.get('suite') != 'native_copy_source' or type(baseline.get('passed')) is not bool:
        raise ValueError('baseline_suite')
    if type(baseline.get('payload_bytes')) is not int or baseline['payload_bytes'] != options.bytes:
        raise ValueError('baseline_payload_bytes')
    binary = baseline.get('binary')
    if (not isinstance(binary, dict) or not isinstance(binary.get('sha256'), str)
            or HEX64.fullmatch(binary['sha256']) is None
            or not isinstance(binary.get('version'), str)
            or re.fullmatch(r'[0-9A-Za-z.+_-]{1,100}', binary['version']) is None
            or binary['sha256'] != file_sha256(options.marmish)):
        raise ValueError('baseline_binary')
    revision = baseline.get('source_revision')
    if revision is not None and (not isinstance(revision, str)
                                 or re.fullmatch(r'[0-9a-f]{40}', revision) is None
                                 or revision != options.source_revision):
        raise ValueError('baseline_source_revision')
    relay = urlsplit(options.relay)
    if (baseline.get('relay') != relay.scheme + '://' + relay.netloc
            or type(baseline.get('relay_path_redacted')) is not bool
            or baseline['relay_path_redacted'] != bool(relay.path and relay.path != '/')
            or baseline.get('platform') != os.uname().sysname
            or baseline.get('architecture') != os.uname().machine):
        raise ValueError('baseline_context')
    results = baseline.get('results')
    if not isinstance(results, list) or len(results) > len(CORE_CASES):
        raise ValueError('baseline_results')
    selected = {}
    for case in options.case:
        rows = [row for row in results if isinstance(row, dict) and row.get('case') == case]
        if len(rows) != 1:
            raise ValueError('baseline_selected_case_missing_or_duplicate')
        selected[case] = baseline_row(case, rows[0], options.bytes)
    required = math.ceil(4 * max(row['seconds'] for row in selected.values()))
    if options.timeout_seconds < required:
        raise ValueError('baseline_requires_four_times_observed_timeout')
    return {'sha256': digest, 'overall_passed': baseline['passed'],
            'binary': {'sha256': binary['sha256'], 'version': binary['version']},
            'source_revision': revision, 'source_revision_verified': revision is not None,
            'relay_path_verified': False, 'selected': selected,
            'required_timeout_seconds': required}


def corrective_arguments(argv=None):
    parsers = []

    def configure(parser):
        parsers.append(parser)
        parser.add_argument('--case', action='append', required=True, choices=SELECTABLE)
        parser.add_argument('--baseline-report', required=True, type=Path)

    options = arguments(2 * 1024 * 1024 + 8192, argv=argv, configure_parser=configure)
    if len(set(options.case)) != len(options.case):
        parsers[0].error('duplicate --case')
    options.case = [case for case in SELECTABLE if case in options.case]
    # Resolve output aliasing without following baseline symlinks on open.
    if options.output == options.baseline_report.resolve():
        parsers[0].error('--output must differ from --baseline-report')
    try:
        options.baseline = validate_baseline(options)
    except (OSError, ValueError, TypeError, KeyError, UnicodeError, RecursionError):
        parsers[0].error('invalid baseline or insufficient corrective timeout')
    return options


class SelectedCopyQualification(CopyQualification):
    def __init__(self, options):
        super().__init__(options)
        self.report.update(suite='native_copy_selected_corrective', selected_only=True,
                           full_gate_claimed=False, selected_cases=options.case.copy(), completed_cases=[],
                           omitted_cases=[case for case in CORE_CASES if case not in options.case],
                           baseline=options.baseline,
                           operation_timeout_seconds=options.timeout_seconds,
                           elapsed_scope='inherited_transfer_including_local_validation',
                           locally_seeded_get_sources=[])
        self.report['not_covered'] += self.report['omitted_cases']
        self.report['fixture_scope'] = 'same_host_owned_paths_no_preparatory_remote_transfers'
        self.report['baseline_eligibility'] = 'completed_rows_only_no_timeout_or_missing_row_claim'
        self.report['negative_get_operand'] = 'locally_created_main_payload_no_preparatory_transfer'

    def identify_binary(self):
        if file_sha256(self.options.marmish) != self.options.baseline['binary']['sha256']:
            raise QualificationError('binary', 'baseline_binary_changed')
        super().identify_binary()
        if self.report['binary'] != self.options.baseline['binary']:
            raise QualificationError('binary', 'baseline_binary_changed')

    def measured_transfer(self, stage, remote, direction, source, destination, **kwargs):
        before = time.monotonic()
        self.transfer(stage, remote, direction, source, destination, **kwargs)
        elapsed = time.monotonic() - before
        row = self.report['results'][-1]
        row.update(kind=self.options.baseline['selected'][stage]['kind'],
                   baseline=self.options.baseline['selected'][stage],
                   operation_timeout_seconds=self.options.timeout_seconds,
                   deadline_headroom=self.options.timeout_seconds / max(elapsed, 1e-9),
                   observed_seconds_including_validation=elapsed)
        if row['deadline_headroom'] < 1.5:
            raise QualificationError(stage, 'corrective_headroom_insufficient')

    def copy_cases(self, remote):
        work = self.root / 'work'
        payload = os.urandom(self.options.bytes)
        source = work / ('source' + NAME)
        source.write_bytes(payload)
        source.chmod(0o640)
        os.utime(source, ns=(MTIME,) * 2)
        self.oracles[source] = payload
        for stage in self.options.case:
            direction = stage.split('_', 1)[0]
            compressed = stage.endswith('_zlib')
            if stage.endswith('_existing_destination'):
                destination = work / (stage + '-empty')
                destination.write_bytes(b'')
                before = destination.stat()
                self.measured_transfer(stage, remote, direction, source, destination,
                                       expected_error=b'transfer_conflict')
                after = destination.stat()
                if (not stat.S_ISREG(after.st_mode) or after.st_size != 0
                        or (after.st_dev, after.st_ino, after.st_mtime_ns) !=
                           (before.st_dev, before.st_ino, before.st_mtime_ns)
                        or source.read_bytes() != payload or source.stat().st_mode & 0o777 != 0o640
                        or source.stat().st_mtime_ns != MTIME):
                    raise QualificationError(stage, 'refusal_changed_operand')
            else:
                put = work / ('put' + str(compressed) + NAME)
                destination = put if direction == 'put' else work / ('get' + str(compressed) + NAME)
                original = source if direction == 'put' else put
                if direction == 'get' and not put.exists():
                    put.write_bytes(payload)
                    put.chmod(0o640)
                    os.utime(put, ns=(MTIME,) * 2)
                    self.oracles[put] = payload
                    self.report['locally_seeded_get_sources'].append(stage)
                self.measured_transfer(stage, remote, direction, original, destination,
                                       compressed=compressed, extra=('--preserve-mode', '--preserve-mtime'))
                if (destination.stat().st_mode & 0o777 != 0o640
                        or destination.stat().st_mtime_ns != MTIME or source.stat().st_mtime_ns != MTIME):
                    raise QualificationError(stage, 'metadata_mismatch')
            self.report['completed_cases'].append(stage)
        if (work / 'SHOULD_NOT_EXIST').exists():
            raise QualificationError('literal_paths', 'shell_evaluation')


def cli(argv=None):
    options = corrective_arguments(argv)
    return main(SelectedCopyQualification, lambda q, remote: q.copy_cases(remote), options=options)


if __name__ == '__main__':
    raise SystemExit(cli())
