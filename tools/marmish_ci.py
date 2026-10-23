"""User-local, digest-pinned native CI toolchain and public build bundles."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request

from marmish_rpc import strict_json, RpcError
from marmish_rendezvous import context

RUST = '1.97.1'
MANIFEST_SHA256 = '03569b1886ceb5c05276b50c8431ab111de944cd6140fe1fa7d821dd8e0f29cf'
REPO = Path(__file__).resolve().parent.parent


def capabilities():
    if sys.version_info < (3, 11) or not hasattr(tarfile, 'data_filter'):
        raise RuntimeError('python_capability_required')


def refuse_api_tokens():
    if any(os.environ.get(name) for name in ('GITHUB_TOKEN', 'GH_TOKEN',
            'GITHUB_ENTERPRISE_TOKEN', 'GH_ENTERPRISE_TOKEN', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN')):
        raise RuntimeError('unexpected_api_token')


def build_environment():
    # Actions runtime services remain available to artifact steps, but neither
    # those credentials nor API credentials enter native build processes.
    names = {'PATH', 'HOME', 'TMPDIR', 'LANG', 'LC_ALL', 'CARGO_HOME', 'CARGO_TARGET_DIR',
             'MACOSX_DEPLOYMENT_TARGET', 'CC', 'CXX', 'SDKROOT', 'RUSTFLAGS',
             'MARMISH_MDK_TARGET_DIR', 'MARMISH_NATIVE_TOOLCHAIN', 'EPIPHYTIC_EXECUTION_ID',
             'RUNNER_TRACKING_ID'}
    return {name: value for name, value in os.environ.items() if name in names}


def checked(command, **kwargs):
    kwargs.setdefault('env', build_environment())
    subprocess.run(command, check=True, **kwargs)


def file_hashes(root):
    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(root.rglob('*')) if path.is_file()
            and path.name != '.marmish-toolchain.json'}


def valid_toolchain(destination):
    try:
        record = strict_json((destination / '.marmish-toolchain.json').read_bytes())
        return (record['managed'] is True and record['rust'] == RUST and
                record['manifest'] == MANIFEST_SHA256 and record['hashes'] == file_hashes(destination)
                and all((destination / 'bin' / name).is_file()
                        for name in ('rustc', 'cargo', 'cargo-clippy', 'rustfmt')))
    except (OSError, ValueError, KeyError, TypeError, RpcError):
        return False


def download(url, expected):
    if not url.startswith('https://static.rust-lang.org/'):
        raise RuntimeError('invalid_bootstrap_origin')
    with urllib.request.urlopen(url, timeout=60) as response:
        body = response.read(400 * 1024 * 1024 + 1)
    if len(body) > 400 * 1024 * 1024 or hashlib.sha256(body).hexdigest() != expected:
        raise RuntimeError('bootstrap_digest_mismatch')
    return body


def bootstrap(destination):
    capabilities()
    refuse_api_tokens()
    target = {('Linux', 'x86_64'): 'x86_64-unknown-linux-gnu',
              ('Linux', 'aarch64'): 'aarch64-unknown-linux-gnu',
              ('Darwin', 'arm64'): 'aarch64-apple-darwin'}[(platform.system(), platform.machine())]
    if destination.exists():
        if valid_toolchain(destination):
            return
        marker = destination / '.marmish-toolchain.json'
        # Only this bootstrap's managed installation may be replaced. An
        # arbitrary existing directory is never removed or installed over.
        if destination.is_symlink() or not marker.is_file():
            raise RuntimeError('unmanaged_toolchain_destination')
        record = strict_json(marker.read_bytes())
        if record.get('managed') is not True:
            raise RuntimeError('unmanaged_toolchain_destination')
    manifest = tomllib.loads(download('https://static.rust-lang.org/dist/channel-rust-' + RUST + '.toml',
                                     MANIFEST_SHA256).decode())
    package = manifest['pkg']['rust']['target'][target]
    payload = download(package['xz_url'], package['xz_hash'])
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='.marmish-rust-', dir=destination.parent) as temporary:
        work = Path(temporary)
        archive = work / 'rust.tar.xz'
        archive.write_bytes(payload)
        with tarfile.open(archive) as source:
            source.extractall(work, filter='data')
        source_root = work / ('rust-' + RUST + '-' + target)
        installed = work / 'installed'
        checked(['sh', str(source_root / 'install.sh'), '--prefix=' + str(installed),
                 '--disable-ldconfig', '--components=rustc,cargo,rust-std-' + target +
                 ',clippy-preview,rustfmt-preview'])
        version = subprocess.check_output([installed / 'bin/rustc', '--version'],
                                           text=True, env=build_environment())
        if not version.startswith('rustc ' + RUST + ' '):
            raise RuntimeError('installed_toolchain_mismatch')
        record = {'managed': True, 'rust': RUST, 'manifest': MANIFEST_SHA256,
                  'hashes': file_hashes(installed)}
        (installed / '.marmish-toolchain.json').write_text(json.dumps(record, sort_keys=True))
        if not valid_toolchain(installed):
            raise RuntimeError('incomplete_toolchain_install')
        if destination.exists():
            shutil.rmtree(destination)
        installed.rename(destination)


def tool_versions():
    return {'python': sys.version.split()[0],
            'rustc': subprocess.check_output(['rustc', '--version'], text=True,
                                             env=build_environment()).strip()[:256],
            'cc': subprocess.check_output(['cc', '--version'], text=True,
                                          env=build_environment()).splitlines()[0][:256]}


def bundle_record(hashes):
    return {'version': 1, 'context': context(), 'platform': sys.platform,
            'architecture': platform.machine(), 'hashes': hashes, 'tools': tool_versions()}


def validate_bundle(record, expected_context=None):
    required = {'wnd', 'fixture', 'marmish-worker', 'marmish-engine-bridge', 'wnd.manifest.json'}
    if sys.platform == 'darwin':
        required |= {'marmish-lifetime', 'marmish-probe'}
    if (not isinstance(record, dict) or set(record) != {'version', 'context', 'platform',
            'architecture', 'hashes', 'tools'} or type(record['version']) is not int or
            record['version'] != 1 or record['context'] != (expected_context or context()) or
            record['platform'] != sys.platform or record['architecture'] != platform.machine() or
            not isinstance(record['hashes'], dict) or set(record['hashes']) != required or
            not all(isinstance(value, str) and re.fullmatch('[0-9a-f]{64}', value)
                    for value in record['hashes'].values()) or not isinstance(record['tools'], dict) or
            set(record['tools']) != {'python', 'rustc', 'cc'} or
            not all(isinstance(x, str) and 0 < len(x) <= 256 for x in record['tools'].values())):
        raise RuntimeError('invalid_native_bundle')
    return record


def _build_bundle(destination):
    checked(['cargo', 'build', '--locked', '--release', '-p', 'marmish-engine', '-p', 'marmish-prototype'])
    for name in ('marmish-worker', 'marmish-engine-bridge'):
        shutil.copyfile(REPO / 'target/release' / name, destination / name)
        (destination / name).chmod(0o755)
    checked(['rustc', '--edition=2024', '-O', str(REPO / 'tools/marmish_fixture.rs'),
             '-o', str(destination / 'fixture')])
    environment = dict(build_environment(), MARMISH_NATIVE_TOOLCHAIN='1')
    checked([sys.executable, str(REPO / 'tools/build_mdk.py'), '--output', str(destination / 'wnd')],
            env=environment)
    hashes = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
              for path in sorted(destination.iterdir()) if path.is_file()}
    (destination / 'bundle.json').write_text(json.dumps(bundle_record(hashes), sort_keys=True))
    # Artifact services discard modes; keep native executables in one tar archive.
    with tarfile.open(str(destination) + '.tar', 'w') as archive:
        for path in sorted(destination.iterdir()):
            archive.add(path, arcname=path.name, recursive=False)


def bundle(destination):
    capabilities()
    refuse_api_tokens()
    archive = Path(str(destination) + '.tar')
    if destination.exists() or destination.is_symlink() or archive.exists() or archive.is_symlink():
        raise RuntimeError('native_bundle_destination_exists')
    destination.mkdir(parents=True)
    try:
        _build_bundle(destination)
    except BaseException:
        shutil.rmtree(destination)
        archive.unlink(missing_ok=True)
        raise


def unpack(archive, destination):
    capabilities()
    if destination.exists() or destination.is_symlink():
        raise RuntimeError('native_bundle_destination_exists')
    if archive.stat().st_size > 600 * 1024 * 1024:
        raise RuntimeError('native_bundle_oversized')
    allowed = {'wnd', 'fixture', 'marmish-worker', 'marmish-engine-bridge',
               'wnd.manifest.json', 'bundle.json', 'marmish-lifetime', 'marmish-probe'}
    destination.mkdir(parents=True)
    try:
        with tarfile.open(archive, mode='r:') as source:
            members = source.getmembers()
            if (len({x.name for x in members}) != len(members) or len(members) > 8 or
                    not any(x.name == 'bundle.json' for x in members) or
                    sum(x.size for x in members) > 512 * 1024 * 1024 or any(
                    not x.isfile() or x.name not in allowed or x.size > 256 * 1024 * 1024
                    for x in members)):
                raise RuntimeError('invalid_native_bundle')
            source.extractall(destination, filter='data')
        raw = (destination / 'bundle.json').read_bytes()
        if len(raw) > 8192 or raw.startswith(b'\xef\xbb\xbf'):
            raise RuntimeError('invalid_native_bundle')
        record = validate_bundle(strict_json(raw))
        hashes = record['hashes']
        if set(hashes) != {x.name for x in destination.iterdir()} - {'bundle.json'}:
            raise RuntimeError('invalid_native_bundle')
        for name, expected in hashes.items():
            if hashlib.sha256((destination / name).read_bytes()).hexdigest() != expected:
                raise RuntimeError('native_bundle_digest_mismatch')
    except BaseException:
        # This directory was exclusively created above; never remove a caller's
        # preexisting destination when extraction or validation fails.
        shutil.rmtree(destination)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('bootstrap', 'bundle', 'unpack'))
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--archive', type=Path)
    args = parser.parse_args()
    if args.operation == 'bootstrap':
        bootstrap(args.destination.resolve())
    elif args.operation == 'bundle':
        bundle(args.destination.resolve())
    else:
        unpack(args.archive, args.destination.resolve())


if __name__ == '__main__':
    main()
