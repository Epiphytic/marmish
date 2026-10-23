#!/usr/bin/env python3
"""Attest the reviewed complete marmot-app Rust source, including all factories."""
import hashlib
import json
import os
from pathlib import Path
import stat
import sys

from marmish_mdk_build import PATCH, REVISION, ROOT, digest, strict_object

PIN = ROOT / 'patches/mdk/source-guard.json'


def source_digest(root):
    source = Path(root) / 'crates/marmot-app/src'
    if not stat.S_ISDIR(source.lstat().st_mode) or source.is_symlink():
        raise ValueError('mdk_source_type')
    files = []
    for directory, directories, names in os.walk(source, followlinks=False):
        for name in directories + names:
            path = Path(directory) / name
            meta = path.lstat()
            if stat.S_ISLNK(meta.st_mode) or not (stat.S_ISDIR(meta.st_mode) or stat.S_ISREG(meta.st_mode)):
                raise ValueError('mdk_source_type')
            if stat.S_ISREG(meta.st_mode) and path.suffix == '.rs':
                files.append(path)
    result = hashlib.sha256()
    for path in sorted(files):
        name = path.relative_to(source).as_posix().encode('utf-8')
        raw = path.read_bytes()
        result.update(len(name).to_bytes(8, 'big') + name + len(raw).to_bytes(8, 'big') + raw)
    return result.hexdigest()


def check(root, *, expected=None):
    # Synthetic trusted baseline injection is a unit-test API, never a CLI flag.
    if expected is None:
        value = json.loads(PIN.read_bytes(), object_pairs_hook=strict_object)
        if (set(value) != {'version', 'revision', 'patch_sha256', 'source_sha256'}
                or type(value['version']) is not int or value['version'] != 1
                or value['revision'] != REVISION or value['patch_sha256'] != digest(PATCH)):
            raise ValueError('mdk_source_pin')
        expected = value['source_sha256']
    if source_digest(root) != expected:
        raise ValueError('mdk_sdk_factory_guard')


if __name__ == '__main__':
    check(Path(sys.argv[1]))
