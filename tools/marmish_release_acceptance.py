#!/usr/bin/env python3
"""Validate the real embedded installer and installed runtime in native CI."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import types

REPO = Path(__file__).resolve().parent.parent


def installer():
    text = (REPO / 'scripts/install.sh').read_text()
    code = text.split('# BEGIN MARMISH INSTALLER PYTHON\n', 1)[1].split(
        '# END MARMISH INSTALLER PYTHON', 1)[0]
    module = types.ModuleType('marmish_distribution_installer')
    exec(compile(code, 'scripts/install.sh', 'exec'), module.__dict__)
    return module


def live_acceptance(command, manifest, archive_sha256, evidence):
    binding = {'distribution_source_commit': manifest['source_commit'],
               'distribution_archive_sha256': archive_sha256,
               'target': manifest['target']}

    def failed(category):
        result = {'outcome': 'failed', 'error': category, **binding}
        evidence.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
        raise ValueError('installed_live_acceptance_failed') from None

    try:
        completed = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   text=True, timeout=960)
    except subprocess.TimeoutExpired:
        failed('installed_live_timeout')
    except (OSError, UnicodeError):
        failed('installed_live_execution_failed')
    try:
        result = json.loads(completed.stdout)
    except (ValueError, TypeError):
        failed('installed_live_invalid_json')
    if not isinstance(result, dict):
        failed('installed_live_invalid_result')
    if completed.returncode != 0 or result.get('outcome') != 'passed':
        failed('installed_live_command_failed')
    result.update(binding)
    evidence.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
    return result


def acceptance(args):
    with args.archive.open('rb') as source:
        archive_sha256 = hashlib.file_digest(source, 'sha256').hexdigest()
    module = installer()
    prefix = args.prefix.resolve()
    launchers = prefix / 'bin'
    manifest = module.install_archive(args.archive, prefix, launchers, args.version,
                                      module.native_target())
    root = prefix / 'versions' / ('marmish-' + args.version + '-' + manifest['target'])
    subprocess.run([sys.executable, str(REPO / 'tools/marmish_release.py'), 'smoke',
                    '--root', str(root)], check=True)
    # A second installation must preserve the immutable version and work again.
    module.install_archive(args.archive, prefix, launchers, args.version, manifest['target'])
    for name in ('marmish', 'marmishd'):
        subprocess.run([str(launchers / name), '--help'], check=True,
                       stdout=subprocess.DEVNULL)
    if args.live:
        if not args.relay:
            raise ValueError('explicit_live_relays_required')
        native = root / 'libexec/marmish'
        command = [sys.executable, str(REPO / 'tools/marmish_e2e.py'), '--live',
                   '--wnd', str(native / 'wnd'), '--wnd-sha256',
                   hashlib.sha256((native / 'wnd').read_bytes()).hexdigest(),
                   '--wnd-manifest', str(native / 'wnd.manifest.json'),
                   '--bridge', str(native / 'marmish-engine-bridge'),
                   '--worker', str(native / 'marmish-worker'),
                   '--installed-root', str(root)]
        for relay in args.relay:
            command += ['--relay', relay]
        live_acceptance(command, manifest, archive_sha256, args.prefix.parent / 'installed-e2e.json')
    module.uninstall(prefix, launchers)
    if any((launchers / name).exists() for name in ('marmish', 'marmishd')):
        raise ValueError('uninstall_left_launcher')
    if not (root / 'release.json').is_file():
        raise ValueError('uninstall_removed_version')
    print(json.dumps({'outcome': 'passed', 'target': manifest['target'],
                      'source_commit': manifest['source_commit'],
                      'distribution_archive_sha256': archive_sha256, 'installed_live': args.live,
                      'checks': ['real_installer', 'installed_help', 'staged_provenance',
                                 'idempotence', 'marker_owned_uninstall_retains_versions']}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--prefix', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--live', action='store_true')
    parser.add_argument('--relay', action='append')
    acceptance(parser.parse_args())


if __name__ == '__main__':
    main()
