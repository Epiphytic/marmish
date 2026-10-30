#!/usr/bin/env python3
"""Build pristine pinned MDK; macOS applies the reviewed restricted patch."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import tempfile

from marmish_mdk_build import SOURCE, REVISION, LOCK_SHA256, ROOT, PATCH, BOOTSTRAP_PATCH, FIXTURE, digest, _target


def run(args, **kwargs):
    subprocess.run(args, check=True, **kwargs)


TRANSPORT_TESTS = (
    'shared_canonical_relays',
    'initialize_config_and_ack_subprocess',
    'exact_url_before_network_and_total_socks_timeout',
)


def run_exact_tests(source, selector, environment, package, module, names):
    for suffix in names:
        name = module + '::tests::' + suffix
        result = subprocess.run(['cargo', *selector, 'test', '--locked', '-p', package,
                                 '--lib', name, '--', '--exact', '--test-threads=1'],
                                cwd=source, env=environment, check=True, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        print(result.stdout, end='')
        if ('running 1 test' not in result.stdout or
                'test ' + name + ' ... ok' not in result.stdout or
                'test result: ok. 1 passed; 0 failed;' not in result.stdout):
            raise RuntimeError('transport_test_not_executed')



def run_transport_tests(source, selector, environment):
    run_exact_tests(source, selector, environment, 'marmot-app', 'marmish_transport', TRANSPORT_TESTS)


BOOTSTRAP_TESTS = ('frame_and_immutable_store', 'metadata_and_setup_recovery', 'startup_handshake_subprocess')


def run_bootstrap_tests(source, selector, environment):
    run_exact_tests(source, selector, environment, 'wn-cli', 'marmish_bootstrap', BOOTSTRAP_TESTS)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--licenses-output', type=Path,
                        help='collect original notices from the exact locked MDK dependency graph')
    args = parser.parse_args()
    output = args.output.absolute()
    output.parent.mkdir(parents=True, exist_ok=True)
    host = platform.system().lower()
    if host not in ('darwin', 'linux'):
        parser.error('unsupported platform')
    with tempfile.TemporaryDirectory(prefix='marmish-mdk-build-') as directory:
        source = Path(directory) / 'source'
        run(['git', 'init', '-q', str(source)])
        run(['git', '-C', str(source), 'remote', 'add', 'origin', SOURCE])
        run(['git', '-C', str(source), 'fetch', '--depth', '1', 'origin', REVISION])
        run(['git', '-C', str(source), 'checkout', '--detach', 'FETCH_HEAD'])
        if subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip() != REVISION:
            raise RuntimeError('source_revision_mismatch')
        if subprocess.check_output(['git', '-C', str(source), 'status', '--porcelain']):
            raise RuntimeError('source_not_clean')
        lock_hash = digest(source / 'Cargo.lock')
        if lock_hash != LOCK_SHA256:
            raise RuntimeError('source_lock_mismatch')
        environment = dict(os.environ)
        target_dir = Path(environment.get('MARMISH_MDK_TARGET_DIR', str(source / 'target')))
        if not target_dir.is_absolute():
            raise RuntimeError('nonabsolute_build_target')
        environment['CARGO_TARGET_DIR'] = str(target_dir)
        selector = [] if os.environ.get('MARMISH_NATIVE_TOOLCHAIN') == '1' else ['+1.97.1']
        rustc = subprocess.check_output(['rustc', *selector, '--version'], text=True).strip()
        if not rustc.startswith('rustc 1.97.1 '):
            raise RuntimeError('toolchain_mismatch')
        run(['git', '-C', str(source), 'apply', '--check', str(BOOTSTRAP_PATCH)])
        run(['git', '-C', str(source), 'apply', str(BOOTSTRAP_PATCH)])
        if host == 'darwin':
            run(['git', '-C', str(source), 'apply', '--check', str(PATCH)])
            run(['git', '-C', str(source), 'apply', str(PATCH)])
            environment['MACOSX_DEPLOYMENT_TARGET'] = '15.0'
            for target in ('aarch64_apple_darwin', 'x86_64_apple_darwin'):
                environment['CFLAGS_' + target] = '-mmacosx-version-min=15.0'
                environment['CXXFLAGS_' + target] = '-mmacosx-version-min=15.0'
        run([shutil.which('python3') or 'python3', str(ROOT / 'tools/check_mdk_builders.py'), str(source)])
        run(['cargo', *selector, 'build', '--locked', '--release', '-p', 'wn-cli', '--bin', 'wnd'], cwd=source, env=environment)
        if host == 'darwin':
            # Release is already built from the guarded tree. Test input is external.
            descriptor = os.open(FIXTURE, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(descriptor, 'rb') as fixture:
                if not stat.S_ISREG(os.fstat(fixture.fileno()).st_mode):
                    raise RuntimeError('relay_fixture_type')
                with (source / 'crates/marmot-app/marmish-relays.json').open('xb') as target:
                    shutil.copyfileobj(fixture, target)
            run_transport_tests(source, selector, environment)
        run_bootstrap_tests(source, selector, environment)
        if digest(source / 'Cargo.lock') != lock_hash:
            raise RuntimeError('source_lock_changed')
        if args.licenses_output is not None:
            from marmish_release_licenses import collect
            collect(source, args.licenses_output.absolute(), 'mdk')
        with (target_dir / 'release/wnd').open('rb') as src, output.open('xb') as dst:
            shutil.copyfileobj(src, dst)
        output.chmod(0o755)
        helpers = {}
        if host == 'darwin':
            for name in ('marmish-lifetime', 'marmish-probe'):
                helper = output.parent / name
                if helper.exists() or helper.is_symlink():
                    raise RuntimeError('helper_output_exists')
                run(['clang', '-Wall', '-Wextra', '-Werror', '-O2', '-mmacosx-version-min=15.0',
                     str(ROOT / 'tools/native' / (name + '.c')), '-o', str(helper)])
                run(['codesign', '--force', '--sign', '-', str(helper)])
                helpers[name] = digest(helper)
            run(['codesign', '--force', '--sign', '-', str(output)])
            for artifact in [output, *(output.parent / name for name in helpers)]:
                run(['/usr/bin/codesign', '--verify', '--strict', str(artifact)])
        manifest = {'version': 2, 'source': SOURCE, 'revision': REVISION,
                    'clean_checkout': True, 'platform': host, 'architecture': platform.machine(),
                    'rust': '1.97.1', 'cargo_lock_sha256': lock_hash,
                    'rustc_version': rustc, 'target': _target(host),
                    'codesign': 'adhoc' if host == 'darwin' else None,
                    'transport_tests': 'marmish_transport::tests:passed' if host == 'darwin' else None,
                    'relay_fixture_sha256': digest(FIXTURE) if host == 'darwin' else None,
                    'patch_sha256': digest(PATCH) if host == 'darwin' else None,
                    'deployment_target': '15.0' if host == 'darwin' else None,
                    'restricted_protocol': 1 if host == 'darwin' else None,
                    'helpers': helpers, 'sha256': digest(output),
                    'bootstrap_protocol': 1, 'bootstrap_patch_sha256': digest(BOOTSTRAP_PATCH),
                    'bootstrap_tests': 'marmish_bootstrap::tests:passed',
                    'source_guard_sha256': __import__('check_mdk_builders').source_digest(source)}
        with Path(str(output) + '.manifest.json').open('x') as stream:
            stream.write(json.dumps(manifest, sort_keys=True) + '\n')
        print(json.dumps({'sha256': manifest['sha256'], 'platform': host}))


if __name__ == '__main__':
    main()
