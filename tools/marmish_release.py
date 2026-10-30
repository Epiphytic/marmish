#!/usr/bin/env python3
"""Build complete experimental native Marmish release archives."""
import argparse
import gzip
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile

from marmish_installed import (DARWIN_NATIVE, NATIVE, TARGETS, VERSION_PATTERN,
                               file_digest, payload_mode, payload_paths, target,
                               validate_record)

REPO = Path(__file__).resolve().parent.parent
LIMITATIONS = {
    'x86_64-unknown-linux-gnu': ['Experimental noninteractive prototype; no interactive shell or multi-device support.'],
    'aarch64-unknown-linux-gnu': ['Runtime confinement not validated: native runner denies nested user namespaces.'],
    'aarch64-apple-darwin': ['Runtime confinement not validated: confined broker preflight aborts (SIGABRT).',
                             'Ad-hoc signed, not notarized; upgrades may require renewed Keychain access.'],
}


def environment():
    from marmish_ci import build_environment
    return build_environment()


def checked(command, **options):
    return subprocess.run([str(arg) for arg in command], check=True, env=environment(),
                          stdin=subprocess.DEVNULL, **options)


def output(command):
    return checked(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True).stdout


def source_commit():
    value = output(['git', '-C', REPO, 'rev-parse', 'HEAD']).strip()
    if not re.fullmatch('[0-9a-f]{40}', value):
        raise ValueError('invalid_source_commit')
    status = output(['git', '-C', REPO, 'status', '--porcelain=v1', '--untracked-files=all'])
    if any(line and not line[3:].startswith('.beads/') for line in status.splitlines()):
        raise ValueError('dirty_release_source')
    return value


def launcher(command):
    # The tools path is a positional argument, never interpolated Python source.
    code = ('import sys;sys.path.insert(0,sys.argv.pop(1));'
            'from marmish_app import guarded_main,' + command + ';'
            'raise SystemExit(guarded_main(' + command + '))')
    return ('#!/bin/sh\nset -eu\n'
            'marmish_bin=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)\n'
            'exec python3 -I -B -c \'' + code + '\' "$marmish_bin/../tools" "$@"\n')


def verify_staged(root):
    code = ('import sys;sys.path.insert(0,sys.argv[1]);'
            'from marmish_mdk_build import verify_manifest;'
            'verify_manifest(sys.argv[2],sys.argv[3],sys.argv[4])')
    wnd = root / 'libexec/marmish/wnd'
    checked([sys.executable, '-I', '-B', '-c', code, root / 'tools', wnd,
             file_digest(wnd), str(wnd) + '.manifest.json'])


def shared_dependencies(root, build_target):
    native = root / 'libexec/marmish'
    executables = [p for p in sorted(native.iterdir()) if not p.name.endswith('.json')]
    libraries = set()
    versions = set()
    for path in executables:
        if build_target.endswith('linux-gnu'):
            dynamic = output(['readelf', '-d', path])
            libraries.update(re.findall(r'\(NEEDED\).*\[([^\]\n]+)\]', dynamic))
            symbols = output(['readelf', '--version-info', path])
            versions.update(re.findall(r'\bGLIBC_([0-9]+(?:\.[0-9]+)+)\b', symbols))
        else:
            dynamic = output(['/usr/bin/otool', '-L', path])
            libraries.update(line.strip().split(' (', 1)[0] for line in dynamic.splitlines()[1:])
    glibc = max(versions, key=lambda value: tuple(map(int, value.split('.')))) if versions else None
    if build_target.endswith('linux-gnu') and glibc is None:
        raise ValueError('missing_glibc_version_evidence')
    if not libraries:
        raise ValueError('missing_shared_library_evidence')
    return {'python': '3.11', 'glibc': glibc, 'shared_libraries': sorted(libraries)}


def _copy(source, destination):
    if source.is_symlink() or not source.is_file():
        raise ValueError('unsafe_release_input')
    destination.parent.mkdir(parents=True, exist_ok=True)
    with source.open('rb') as reader, destination.open('xb') as writer:
        shutil.copyfileobj(reader, writer)


def stage(root, runtime, version, build_target, commit):
    root.mkdir(mode=0o755)
    root.chmod(0o755)
    for name in sorted(payload_paths(build_target)):
        destination = root / name
        if name.startswith('bin/'):
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(launcher('server_main' if name.endswith('marmishd') else 'client_main'))
        else:
            source = runtime / Path(name).name if name.startswith('libexec/') else REPO / name
            _copy(source, destination)
        destination.chmod(payload_mode(name))
    licenses = runtime / 'licenses'
    from marmish_release_licenses import validate as validate_licenses
    validate_licenses(licenses)
    for graph in ('marmish', 'mdk'):
        if not (licenses / graph / 'manifest.json').is_file():
            raise ValueError('missing_dependency_license_graph')
    for source in sorted(licenses.rglob('*')):
        if source.is_symlink():
            raise ValueError('unsafe_license_input')
        if source.is_file():
            _copy(source, root / 'licenses' / source.relative_to(licenses))
    for directory in root.rglob('*'):
        if directory.is_dir():
            directory.chmod(0o755)
        elif directory.relative_to(root).parts[0] == 'licenses':
            directory.chmod(0o644)
    verify_staged(root)
    record = {'format': 1, 'version': version, 'target': build_target, 'source_commit': commit,
              'files': {str(path.relative_to(root)): {'sha256': file_digest(path),
                        'mode': payload_mode(str(path.relative_to(root)))}
                        for path in sorted(root.rglob('*')) if path.is_file()},
              'prerequisites': shared_dependencies(root, build_target), 'experimental': True,
              'limitations': LIMITATIONS[build_target]}
    validate_record(record)
    (root / 'release.json').write_text(json.dumps(record, sort_keys=True, indent=2) + '\n')
    (root / 'release.json').chmod(0o644)
    return record


def archive(root, destination):
    # Fixed metadata and USTAR only: the installer rejects extension records.
    with destination.open('xb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.USTAR_FORMAT) as tar:
            for path in [root, *sorted(root.rglob('*'))]:
                info = tar.gettarinfo(str(path), arcname=str(path.relative_to(root.parent)))
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ''
                if path.is_file():
                    with path.open('rb') as data:
                        tar.addfile(info, data)
                else:
                    tar.addfile(info)


def build_runtime(destination):
    from marmish_release_licenses import collect
    collect(REPO, destination / 'licenses', 'marmish')
    checked(['cargo', 'build', '--locked', '--release', '-p', 'marmish-engine',
             '-p', 'marmish-prototype', '-p', 'marmish-vault'], cwd=REPO)
    cargo_target = Path(os.environ.get('CARGO_TARGET_DIR', str(REPO / 'target')))
    if not cargo_target.is_absolute():
        cargo_target = REPO / cargo_target
    for name in ('marmish-worker', 'marmish-engine-bridge', 'marmish-keychain'):
        _copy(cargo_target / 'release' / name, destination / name)
    checked([sys.executable, REPO / 'tools/build_mdk.py', '--output', destination / 'wnd',
             '--licenses-output', destination / 'licenses'], cwd=REPO)


def smoke(root):
    root = root.resolve(strict=True)
    from marmish_installed import strict_json
    record = validate_record(strict_json((root / 'release.json').read_bytes()))
    if record['target'] != target():
        raise ValueError('smoke_target_mismatch')
    actual = {str(path.relative_to(root)) for path in root.rglob('*') if path.is_file()}
    if actual != set(record['files']) | {'release.json'} or any(path.is_symlink() for path in root.rglob('*')):
        raise ValueError('smoke_payload_mismatch')
    for name, entry in record['files'].items():
        path = root / name
        if file_digest(path) != entry['sha256'] or path.stat().st_mode & 0o7777 != entry['mode']:
            raise ValueError('smoke_digest_or_mode_mismatch')
    verify_staged(root)
    for name in ('marmish', 'marmishd'):
        checked([root / 'bin' / name, '--help'], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    # Doctor may report missing runtime prerequisites on build-only hosts.
    result = subprocess.run([root / 'bin/marmish', '--doctor'], env=environment(),
                            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True, timeout=15)
    diagnostic = json.loads(result.stdout)
    if result.returncode not in (0, 1) or not isinstance(diagnostic.get('ok'), bool):
        raise ValueError('invalid_installed_doctor')
    if sys.platform == 'darwin':
        for name in ('wnd', *DARWIN_NATIVE):
            checked(['/usr/bin/codesign', '--verify', '--strict', root / 'libexec/marmish' / name])
    print(json.dumps({'smoke': 'passed', 'target': record['target'], 'doctor': diagnostic}, sort_keys=True))


def build(args):
    if len(args.version) > 64 or not VERSION_PATTERN.fullmatch(args.version):
        raise ValueError('invalid_release_version')
    build_target = target()
    commit = source_commit()
    if not (REPO / 'LICENSE').is_file():
        raise ValueError('missing_project_license')
    args.output.mkdir(parents=True, exist_ok=True)
    name = 'marmish-' + args.version + '-' + build_target
    destination = args.output / (name + '.tar.gz')
    sidecar = args.output / (name + '.release.json')
    if destination.exists() or sidecar.exists():
        raise ValueError('release_destination_exists')
    with tempfile.TemporaryDirectory(prefix='.marmish-release-', dir=args.output) as temporary:
        work = Path(temporary)
        runtime = args.runtime_dir
        if runtime is None:
            runtime = work / 'runtime'
            runtime.mkdir()
            build_runtime(runtime)
        else:
            if runtime.is_symlink() or not runtime.is_dir():
                raise ValueError('unsafe_runtime_directory')
            value = json.loads((runtime / 'build-source.json').read_text())
            if value != {'source_commit': commit, 'target': build_target}:
                raise ValueError('runtime_source_mismatch')
        root = work / name
        record = stage(root, runtime, args.version, build_target, commit)
        smoke(root)
        staged_archive = work / (name + '.tar.gz')
        archive(root, staged_archive)
        # Outputs only appear after every build/provenance/smoke check succeeds.
        staged_sidecar = work / 'sidecar.json'
        staged_sidecar.write_text(json.dumps(record, sort_keys=True, indent=2) + '\n')
        os.link(staged_archive, destination)
        try:
            os.link(staged_sidecar, sidecar)
        except BaseException:
            destination.unlink()
            raise
    print(json.dumps({'archive': str(destination), 'sha256': file_digest(destination),
                      'manifest': str(sidecar), 'target': build_target}, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    builder = commands.add_parser('build')
    builder.add_argument('--version', required=True)
    builder.add_argument('--output', required=True, type=Path)
    builder.add_argument('--runtime-dir', type=Path,
                         help='offline packaging of provenance-recorded prebuilt native inputs')
    checker = commands.add_parser('smoke')
    checker.add_argument('--root', required=True, type=Path)
    args = parser.parse_args()
    try:
        if args.command == 'build':
            build(args)
        else:
            smoke(args.root)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        parser.exit(1, 'marmish-release: build_or_validation_failed\n')


if __name__ == '__main__':
    main()
