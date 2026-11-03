#!/usr/bin/env python3
"""CI-only adapter of the audited collector; never installed or required at runtime."""
from pathlib import Path
import argparse
import json
import hashlib
import re
import subprocess
import marmish_release_licenses as licenses


def rust_notices(sysroot: Path, compiler: bytes) -> bytes:
    """Preserve notices shipped with the exact selected compiler/standard library."""
    match = re.search(rb'^commit-hash: ([0-9a-f]{40})$', compiler, re.M)
    if not match or len(compiler) > 16384:
        raise RuntimeError('invalid_rust_toolchain_provenance')
    docs = sysroot / 'share/doc/rust'
    directory = docs / 'licenses'
    paths = sorted(directory.iterdir())
    if not 1 <= len(paths) <= 128 or any(not p.is_file() or p.is_symlink() for p in paths):
        raise RuntimeError('invalid_rust_toolchain_notices')
    if not {'MIT.txt', 'Apache-2.0.txt'} <= {p.name for p in paths}:
        raise RuntimeError('missing_rust_toolchain_notices')
    parts = [b'\nRust standard library/toolchain notices\n', compiler,
             b'Original source: https://github.com/rust-lang/rust/tree/' + match[1] + b'\n']
    for path in [docs / 'COPYRIGHT-library.html', *paths]:
        raw = licenses._read(path)
        parts += [b'\n===== rust/' + path.name.encode('utf-8') + b' sha256=' +
                  hashlib.sha256(raw).hexdigest().encode('ascii') + b' =====\n', raw, b'\n']
    return b''.join(parts)


def assemble(directory: Path, output: Path, toolchain: bytes = b'') -> None:
    licenses.validate_graph(directory)
    manifest = (directory / 'manifest.json').read_bytes()
    parts = [b'Marmish native dependency license inventory\n\n',
             (directory / 'SOURCES.txt').read_bytes(), b'\nLocked graph manifest:\n', manifest]
    records = json.loads(manifest)
    referenced = sorted({notice['text'] for package in records['packages']
                         for notice in package['notices']})
    for name in referenced:
        parts += [b'\n===== ' + name.encode('ascii') + b' =====\n',
                  (directory / name).read_bytes(), b'\n']
    parts.append(toolchain)
    content = b''.join(parts)
    if not 0 < len(content) <= 16 * 1024 * 1024:
        raise RuntimeError('native_license_inventory_oversized')
    with output.open('xb') as stream:
        stream.write(content)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    licenses.collect(args.root, args.work_dir, 'native')
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], timeout=30, text=True).strip())
    compiler = subprocess.check_output(['rustc', '-vV'], timeout=30)
    assemble(args.work_dir / 'native', args.output, rust_notices(sysroot, compiler))


if __name__ == '__main__':
    main()
