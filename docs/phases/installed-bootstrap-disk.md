# Bound installed-workflow compiler disk usage

The native macOS job failed copying Rust's standard library with ENOSPC. The
verified official Darwin Rust 1.97.1 archive is 204,675,456 compressed bytes and
1,849,029,532 expanded bytes. Its five already-requested components total
588,861,566 expanded bytes including root metadata. The current `extractall` writes unrequested docs,
LLVM tools and analyzer files before installing only those five components.
Its approximate archive/extraction/installation peak is 2.64 GB. These figures
identify avoidable peak usage; logs do not establish historical job leaks or the
runner's initial free space. Retained fail-closed runtime state is never deleted
by this change.

## Preserve the generic bootstrap contract

Keep `bootstrap(destination)`, its five selected components, version, pinned
manifest and output `.marmish-toolchain.json` schema compatible with existing
release/native workflows. Parse the authenticated umbrella tar in memory, as
`download` already returns bounded bytes; do not write a compressed archive to
disk. Extract only the exact package root directory, flat regular root metadata,
and the existing selected component trees (`rustc`, `cargo`, target `rust-std`,
`clippy-preview`, `rustfmt-preview`). Reject malformed/absolute/traversal names,
selected links and unexpected selected member types. Use `tarfile`'s data filter.
Require each selected component and its `manifest.in`, plus installer metadata.
Do not unpack unselected directories. Run the unchanged official installer with
its explicit five-component list. TemporaryDirectory continues to remove only
the work directory this invocation created, including extraction/install errors.
The resulting approximate disk peak is 1.18 GB, saving about 1.46 GB on Darwin.

All path comparisons use canonical_state_path, including Darwin /tmp aliases.

Log finite JSON counts (free/total/required and selected unpacked bytes, no paths
or environment) before extraction and installation. Require available space for
block-rounded selected extraction plus selected installation plus a 128 MiB reserve before
extraction; require installation plus reserve afterward. A finite
`insufficient_bootstrap_space` error replaces an avoidable mid-copy ENOSPC.
Use statvfs f_bavail*f_frsize on the actual destination filesystem; /tmp may be
a limited Linux tmpfs. Round each member up to filesystem blocks and include
extra installer metadata within the reserve. Two streaming xz passes validate/
budget first and extract in archive order second, avoiding repeated seeks.
These are conservative estimates, not a promise against concurrent disk users.

## Current-job ownership and consumption

Only installed-interop changes its source path to the already-owned private
`$MARMISH_INSTALLED_ROOT/bootstrap/toolchain`. Add `bootstrap-owned` and
`cleanup-bootstrap` CLI operations in a new CI-only marmish_installed_bootstrap.py, leaving generic bootstrap
behavior intact. The owned operation requires destination basename `toolchain`,
parent basename `bootstrap`, an existing owned 0700 harness parent, and an absent
bootstrap directory and sidecar. Exclusively create bootstrap 0700, then an
immutable 0600 `bootstrap-owner.json` in the harness parent containing exact
format/kind/context/path and bootstrap device/inode/uid. Validate context using
the installed-release contract. Only then invoke ordinary bootstrap. If it
fails, attempt scoped cleanup; cleanup failure retains the ownership evidence.
A crash in the mkdir/record gap fails closed, retaining the tiny unrecorded root.

`cleanup_bootstrap(destination, ctx)` accepts only that exact recorded path and
context and validates no-follow file/directory ownership, mode and identity
through existing installed-state helpers. It holds the private harness parent
open while verifying and deleting. Require Python's fd-resistant rmtree support;
The recorded path is the toolchain destination; the identity belongs to its
bootstrap parent, and that whole parent subtree is deleted.
Remove only the recorded bootstrap subtree, prove its absence, then remove only
the verified sidecar. No matching ownership record means a preexisting directory
is refused. Missing record plus missing directory is `absent`. A valid record
with an absent bootstrap root removes only that verified sidecar and reports
`already-removed`. This proves
owned temporary-file removal, not native process absence; bootstrap runs before
keys/runtime identities and retains its existing workflow time/process boundary.

Extend prepare CLI with `--consume-toolchain` requiring `--toolchain-source`.
`prepare_dependencies(..., consume_source=False)` preserves existing callers.
For the installed workflow's true case, require source exactly
`state_file.parent/bootstrap/toolchain`, and validate its ownership record before
copying. After the existing complete verified private copy, revalidate the
ownership record and delete only that current-job source through cleanup_bootstrap
before version/fetch/warmup. Fail closed if deletion cannot be proved; the verified
private copy remains owned by normal state cleanup. This removes the second
installed toolchain (~589 MB) before dependency preparation. Final workflow
`always()` cleanup independently invokes cleanup-bootstrap, regardless of prepare
or live-cleanup outcome, before report upload; it writes a sanitized
bootstrap-cleanup.json report, covering preparation failure
before source consumption or bootstrap failure. No historical directories are
searched, reclaimed or deleted. Existing temporary Python and rootless-bwrap
ownership stays unchanged.

## Validation and publication

Synthetic authenticated archive tests include selected/unselected trees,
metadata, traversal, links and missing components; assert generic manifest shape
and component list remain stable. Disk-budget tests exercise exact boundaries
and finite logs. Ownership tests cover wrong context/path/inode, missing or
symlinked records/root, existing unowned directories, not-started and successful
consumption plus always-cleanup idempotency. Consume-source tests must prove
private copy verification precedes source deletion and that a failed copy never
removes its source. Run a real pinned Linux bootstrap with the new extraction,
then real private-copy/dependency preparation and exact owned cleanup. Run the
native macOS workflow and retain its disk metrics; lack of runner space remains
an honest blocker if conservative preflight fails. Obtain actual Claude design
approval before code, and actual Claude auto code review afterward. Update the
installed phase evidence and do not claim historical leaks or native success
from synthetic tests.
