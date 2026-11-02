"""Installed-CI-only ownership and exact cleanup of a public bootstrap source."""
import argparse
import json
import os
from pathlib import Path
import shutil
import sys

from marmish_ci import bootstrap
from marmish_rpc import strict_json
from marmish_rendezvous import context


def _owned_bootstrap_paths(destination):
    from marmish_installed_state import _directory_fd
    from marmish_platform import canonical_state_path
    raw = Path(destination).absolute()
    if raw.is_symlink() or raw.parent.is_symlink() or raw.parent.parent.is_symlink():
        raise RuntimeError('toolchain_source_unowned')
    destination = canonical_state_path(raw)
    if destination.name != 'toolchain' or destination.parent.name != 'bootstrap':
        raise RuntimeError('toolchain_source_unowned')
    root, harness = destination.parent, destination.parent.parent
    parent = _directory_fd(harness, private=True)
    return destination, root, harness / 'bootstrap-owner.json', parent


def _bootstrap_owner(destination, ctx):
    from marmish_installed_state import read_private, _directory_fd, _root_identity
    from marmish_installed_release import validate_context
    validate_context(ctx)
    destination, root, marker, parent = _owned_bootstrap_paths(destination)
    try:
        if not os.path.lexists(marker):
            if os.path.lexists(root):
                raise RuntimeError('toolchain_source_unowned')
            return None
        record = strict_json(read_private(marker))
        if (not isinstance(record, dict) or set(record) !=
                {'format', 'kind', 'context', 'destination', 'root_identity'} or
                type(record['format']) is not int or record['format'] != 1 or
                record['kind'] != 'owned-bootstrap' or record['context'] != ctx or
                record['destination'] != str(destination) or
                not isinstance(record['root_identity'], dict) or
                set(record['root_identity']) != {'dev', 'ino', 'uid'} or
                any(type(value) is not int or value < 0 for value in record['root_identity'].values())):
            raise RuntimeError('toolchain_source_unowned')
        if os.path.lexists(root):
            descriptor = _directory_fd(root, private=True)
            try:
                if _root_identity(os.fstat(descriptor)) != record['root_identity']:
                    raise RuntimeError('toolchain_source_unowned')
            finally:
                os.close(descriptor)
        return record
    finally:
        os.close(parent)


def cleanup_bootstrap(destination, ctx):
    """Delete only a recorded current-job bootstrap tree, never historical state."""
    from marmish_installed_state import read_private
    record = _bootstrap_owner(destination, ctx)
    destination, root, marker, parent = _owned_bootstrap_paths(destination)
    try:
        if record is None:
            return {'kind': 'bootstrap-cleanup', 'outcome': 'absent', 'error': None}
        raw = read_private(marker)
        existed = os.path.lexists(root)
        if existed:
            if not shutil.rmtree.avoids_symlink_attacks:
                raise RuntimeError('toolchain_source_cleanup_failed')
            # Revalidate under the private held harness parent immediately before removal.
            if _bootstrap_owner(destination, ctx) != record:
                raise RuntimeError('toolchain_source_unowned')
            shutil.rmtree(root.name, dir_fd=parent)
        if os.path.lexists(root) or read_private(marker) != raw:
            raise RuntimeError('toolchain_source_cleanup_failed')
        os.unlink(marker.name, dir_fd=parent)
        os.fsync(parent)
        return {'kind': 'bootstrap-cleanup',
                'outcome': 'complete' if existed else 'already-removed', 'error': None}
    finally:
        os.close(parent)


def bootstrap_owned(destination, ctx):
    from marmish_installed_release import validate_context
    from marmish_installed_state import _directory_fd, _root_identity, write_private
    validate_context(ctx)
    destination, root, marker, parent = _owned_bootstrap_paths(destination)
    try:
        if os.path.lexists(root) or os.path.lexists(marker):
            raise RuntimeError('toolchain_source_unowned')
        os.mkdir(root.name, mode=0o700, dir_fd=parent)
        descriptor = _directory_fd(root, private=True)
        try:
            identity = _root_identity(os.fstat(descriptor))
        finally:
            os.close(descriptor)
        write_private(marker, {'format': 1, 'kind': 'owned-bootstrap', 'context': ctx,
                              'destination': str(destination), 'root_identity': identity})
    finally:
        os.close(parent)
    try:
        bootstrap(destination)
    except BaseException:
        try:
            cleanup_bootstrap(destination, ctx)
        except Exception:
            print(json.dumps({'kind': 'bootstrap-cleanup', 'outcome': 'failed',
                              'error': 'toolchain_source_cleanup_failed'}),
                  file=sys.stderr, flush=True)
            # Retain the marker when scoped removal could not be proved.
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('bootstrap-owned', 'cleanup-bootstrap'))
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    from marmish_installed_state import write_private
    try:
        if args.operation == 'bootstrap-owned':
            bootstrap_owned(args.destination, context())
            result = {'kind': 'bootstrap-preparation', 'outcome': 'complete', 'error': None}
        else:
            result = cleanup_bootstrap(args.destination, context())
    except Exception as failure:
        allowed = {'insufficient_bootstrap_space', 'invalid_bootstrap_archive',
                   'invalid_bootstrap_filesystem', 'toolchain_source_unowned',
                   'toolchain_source_cleanup_failed'}
        category = str(failure) if str(failure) in allowed else 'bootstrap_failed'
        result = {'kind': 'bootstrap-cleanup' if args.operation == 'cleanup-bootstrap'
                  else 'bootstrap-preparation', 'outcome': 'failed', 'error': category}
    print(json.dumps(result, sort_keys=True), flush=True)
    if args.report is not None:
        from marmish_installed_state import public_directory
        from marmish_platform import canonical_state_path
        destination = canonical_state_path(args.destination)
        expected = public_directory(destination.parent.parent / 'state') / 'bootstrap-cleanup.json'
        if args.operation != 'cleanup-bootstrap' or canonical_state_path(args.report) != expected:
            raise RuntimeError('toolchain_source_unowned')
        write_private(expected, result)
    if result['outcome'] == 'failed':
        raise SystemExit(1)


if __name__ == '__main__':
    main()
