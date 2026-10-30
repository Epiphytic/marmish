#!/usr/bin/env python3
"""Aggregate exact native build outputs before creating an experimental release."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from marmish_installed import TARGETS, VERSION_PATTERN, strict_json, validate_record, file_digest
from marmish_release_acceptance import installer

REPOSITORY = 'Epiphytic/marmish'


def verify_archive(path, record):
    if path.is_symlink() or path.stat().st_size > 512 * 1024 * 1024:
        raise ValueError('invalid_release_archive')
    module = installer()
    # The publication gate parses the exact same bounded gzip and strict USTAR
    # format as installation, without installing or invoking platform probes.
    with tempfile.TemporaryDirectory(prefix='marmish-archive-validation-') as temporary:
        stage = Path(temporary)
        expanded = stage / 'archive.tar'
        try:
            module.inflate(path, expanded)
            root, payload = module.extract_ustar(expanded, stage / 'payload',
                                                record['version'], record['target'])
        except module.InstallError as error:
            raise ValueError('invalid_release_archive:' + str(error)) from error
        if payload != set(record['files']):
            raise ValueError('release_archive_incomplete')
        if strict_json((root / 'release.json').read_bytes()) != record:
            raise ValueError('release_sidecar_mismatch')
        for relative, info in record['files'].items():
            if (root / relative).stat().st_mode & 0o7777 != info['mode']:
                raise ValueError('release_payload_mode_mismatch')
            if file_digest(root / relative) != info['sha256']:
                raise ValueError('release_payload_digest_mismatch')


def aggregate(inputs, output, version, source):
    if not VERSION_PATTERN.fullmatch(version) or not re.fullmatch('[0-9a-f]{40}', source):
        raise ValueError('invalid_publication_identity')
    records, archives = {}, {}
    for target in sorted(TARGETS):
        base = 'marmish-' + version + '-' + target
        manifests = list(inputs.rglob(base + '.release.json'))
        bundles = list(inputs.rglob(base + '.tar.gz'))
        if len(manifests) != 1 or len(bundles) != 1 or manifests[0].stat().st_size > 4 * 1024 * 1024:
            raise ValueError('missing_or_duplicate_build')
        record = validate_record(strict_json(manifests[0].read_bytes()))
        if (record['target'], record['version'], record['source_commit']) != (target, version, source):
            raise ValueError('build_identity_mismatch')
        verify_archive(bundles[0], record)
        records[target], archives[target] = record, bundles[0]
    # Require actual installed command acceptance from the Linux x86_64 build.
    reports = list(inputs.rglob('installed-e2e.json'))
    if len(reports) != 1:
        raise ValueError('missing_installed_live_evidence')
    report = strict_json(reports[0].read_bytes())
    required = 'installed_launchers_and_bundled_defaults_preserve_special_character_argv'
    if (report.get('outcome') != 'passed' or required not in report.get('assertions', [])
            or report.get('distribution_source_commit') != source
            or report.get('distribution_archive_sha256') != file_digest(archives['x86_64-unknown-linux-gnu'])
            or report.get('wnd_sha256') != records['x86_64-unknown-linux-gnu']['files']['libexec/marmish/wnd']['sha256']
            or report.get('local_cleanup') != 'complete'):
        raise ValueError('invalid_installed_live_evidence')
    output.mkdir(exist_ok=False)
    for target, path in archives.items():
        shutil.copyfile(path, output / path.name)
        name = path.name.removesuffix('.tar.gz') + '.release.json'
        (output / name).write_text(json.dumps(records[target], sort_keys=True, indent=2) + '\n')
    shutil.copyfile(reports[0], output / 'installed-e2e.json')
    assets = sorted(output.iterdir())
    (output / 'SHA256SUMS').write_text(''.join(file_digest(path) + '  ' + path.name + '\n' for path in assets))
    return records


def release_notes(records, source):
    run_id = os.environ.get('GITHUB_RUN_ID', '')
    if not re.fullmatch('[1-9][0-9]{0,19}', run_id):
        raise ValueError('invalid_publication_run')
    run_url = 'https://github.com/' + REPOSITORY + '/actions/runs/' + run_id
    lines = ['Experimental noninteractive Marmish prototype.', '', 'Source: `' + source + '`.', '',
             'Native GitHub-hosted builds. Originating workflow run: ' + run_url, '',
             'Linux AMD64: installed client/server commands executed through Marmot, including literal shell characters.',
             'Linux ARM64 and macOS ARM64: native builds, provenance and installation checks only; confined execution remains unvalidated.', '',
             'Python 3.11+ and platform confinement prerequisites are required. Run `marmish --doctor`.',
             'No Intel macOS artifact, Developer ID signature or notarization is included.', '',
             'Install from Radicle/Iris:', '',
             '```sh', 'curl -fsSL https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh | sh', '```', '',
             'GitHub: https://github.com/Epiphytic/marmish',
             'Radicle: https://radicle.network/nodes/iris.radicle.network/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg', '',
             'Checksums provide integrity from the release channel, not independent signatures.', '']
    for target, record in sorted(records.items()):
        lines += ['- ' + target + ': ' + ' '.join(record['limitations'])]
        if record['prerequisites']['glibc']:
            lines += ['  Requires glibc >= ' + record['prerequisites']['glibc'] + '.']
    return '\n'.join(lines) + '\n'


def verify_remote_tag(tag, source, *, required):
    ref = 'refs/tags/' + tag
    result = subprocess.run(['git', 'ls-remote', '--exit-code', '--tags',
                             'https://github.com/' + REPOSITORY + '.git',
                             ref, ref + '^{}'], capture_output=True, text=True)
    if result.returncode == 2 and not required:
        return
    if result.returncode != 0:
        raise ValueError('release_tag_unavailable')
    refs = dict((name, oid) for oid, name in
                (line.split() for line in result.stdout.splitlines()))
    if refs.get(ref + '^{}', refs.get(ref)) != source:
        raise ValueError('release_tag_source_mismatch')


def refuse_existing_release(tag):
    # Listing with the publication credential includes leftover draft releases.
    # A missing tag ref alone cannot establish that a draft does not exist.
    for page in range(1, 11):
        result = subprocess.run(['gh', 'api', 'repos/' + REPOSITORY +
            '/releases?per_page=100&page=' + str(page)],
            capture_output=True, text=True, timeout=30)
        if result.returncode != 0 or len(result.stdout) > 4 * 1024 * 1024:
            raise ValueError('release_inventory_unavailable')
        records = strict_json(result.stdout.encode())
        if (not isinstance(records, list) or len(records) > 100 or
                any(not isinstance(item, dict) or not isinstance(item.get('tag_name'), str)
                    for item in records)):
            raise ValueError('invalid_release_inventory')
        if any(item['tag_name'] == tag for item in records):
            raise ValueError('release_already_exists')
        if len(records) < 100:
            return
    raise ValueError('release_inventory_too_large')


def verify_master_source(source):
    result = subprocess.run(['git', 'ls-remote', '--exit-code',
        'https://github.com/' + REPOSITORY + '.git', 'refs/heads/master'],
        capture_output=True, text=True, timeout=30)
    if result.returncode != 0:
        raise ValueError('release_master_unavailable')
    if result.stdout.split() != [source, 'refs/heads/master']:
        raise ValueError('release_master_source_mismatch')


def publish(output, version, source, records):
    tag = 'v' + version
    verify_master_source(source)
    refuse_existing_release(tag)
    verify_remote_tag(tag, source, required=False)
    notes = output.parent / 'release-notes.md'
    notes.write_text(release_notes(records, source))
    command = ['gh', 'release', 'create', tag, '--repo', REPOSITORY, '--target', source,
               '--title', 'Marmish ' + version, '--prerelease', '--draft',
               '--notes-file', str(notes)]
    subprocess.run(command, check=True)
    subprocess.run(['gh', 'release', 'upload', tag, '--repo', REPOSITORY,
                    *map(str, sorted(output.iterdir()))], check=True)
    # Read back exact uploaded bytes before changing draft visibility.
    with tempfile.TemporaryDirectory(prefix='marmish-release-readback-') as temporary:
        subprocess.run(['gh', 'release', 'download', tag, '--repo', REPOSITORY,
                        '--dir', temporary], check=True)
        readback = Path(temporary)
        if {p.name for p in readback.iterdir()} != {p.name for p in output.iterdir()}:
            raise ValueError('published_asset_set_mismatch')
        for asset in output.iterdir():
            if file_digest(asset) != file_digest(readback / asset.name):
                raise ValueError('published_asset_digest_mismatch')
    # GitHub may defer creating a new tag until the draft is published.
    verify_remote_tag(tag, source, required=False)
    verify_master_source(source)
    subprocess.run(['gh', 'release', 'edit', tag, '--repo', REPOSITORY,
                    '--draft=false', '--prerelease'], check=True)
    verify_remote_tag(tag, source, required=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version', required=True)
    parser.add_argument('--source', required=True)
    parser.add_argument('--inputs', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    records = aggregate(args.inputs, args.output, args.version, args.source)
    if args.publish:
        publish(args.output, args.version, args.source, records)


if __name__ == '__main__':
    main()
