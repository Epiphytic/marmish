# Installed fixture compiler boundary refinement

This revision incorporates the actual independent Claude review in
`/tmp/marmish-compiler-design-review.raw.json` (six blockers) and
`/tmp/marmish-compiler-design-review-2.raw.json` (registry and budget blockers). The tested Rust
fixture and pinned Cargo.lock remain unchanged. Compiler processes receive only
public constants and a scrubbed environment. Runtime authentication is unchanged.

## Toolchain and dependencies before identities

Keep the existing digest-pinned `marmish_ci.bootstrap` workflow step, with its
step timeout, before `prepare`. Do not add that toolchain to GITHUB_PATH. Pass its
explicit fresh managed destination to prepare; the workflow refuses an existing
destination. Its provenance is the digest-pinned bootstrap earlier in the same
job; the generated manifest proves copy consistency, not independent origin.
Missing or invalid input fails prepare;
no network download, shell installer or toolchain replacement runs inside state.

`prepare_dependencies(state, *, deadline, toolchain_source)` copies the managed
Rust 1.97.1 toolchain in pure Python into `state.root/compiler`. Traverse source
using lstat, reject symlinks, devices, hardlinked files, unsafe file types or
unexpected modes. Copy regular files into new owned directories beneath the 0700
state root, preserve only safe read/exec bits and verify every destination hash
against the source's managed manifest; re-fence source file stat and directory
identities around copying. Validate the destination managed manifest and full
payload, recording at most 64 executable paths and hashes only in a dedicated
compiler provenance record. Never call register_executable for toolchain files;
compiler provenance is not consulted by runtime executable authentication.
No global toolchain permissions change. Run copied
rustc/cargo --version with ordinary run_bounded before using them; require the
pinned 1.97.1 version explicitly.

Create an empty private writable `state.root/cargo-home`. Do not copy any host
Cargo config, credentials or registry/src. Before compilation, check for
`.cargo/config` and `.cargo/config.toml` in SOURCE and every ancestor, plus either
config file in private CARGO_HOME; any such config fails closed. Use a fixed
worker stage to run `cargo fetch --locked --manifest-path <fixture manifest>`
under the compile boundary below. Only the public crates.io registry from the
pinned lock/manifest is accepted; Cargo's own lock checksums authenticate fetched
crate archives. Warm dependencies with `cargo build --locked --offline` against
that same private Cargo home and `state.root/fixture-target`. Reuse this directory
for the three fixture builds after the identity barrier. Do not describe Cargo
home as read-only: Cargo writes its locks/cache and extracts authenticated crates.

The worker receives only absolute copied cargo/rustc/rustdoc paths, private Cargo
home/target, target triple, public fixture constants and a fixed stage selector.
It builds an explicit environment from the scrubbed base: set RUSTC/RUSTDOC to
those absolute paths, RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER to empty strings,
CARGO_TARGET_<TRIPLE>_LINKER to the absolute system cc, and PATH to private
compiler/bin:/usr/bin:/bin. Omit RUSTUP_HOME and all ambient Cargo/rust flags,
wrappers, compiler settings and credential variables. The worker does not fork
for rustc --version: separate bounded calls obtained that version beforehand.
It only validates input, changes to the fixed fixture source and execve's cargo.
System linker/tool versions and hashes may be recorded as provenance; Linux
requires root-owned non-writable tool paths, while macOS records the selected
Apple tools without imposing a Linux ownership policy on Xcode installations.

Preparation's original 900-second absolute deadline reserves 600 seconds for two
identity initializations, 20 seconds for compiler cleanup and 5 seconds for report
publication. All dependency stages stop at preparation_deadline minus 625 seconds
(or fail before starting if that cutoff already passed); copy/version, fetch and
warmup share this remaining budget. The workflow prepare step allows 17 minutes,
including bootstrap-independent installation and cleanup margin. A slow install
may consume the dependency allocation; dependency work then fails before starting
and preserves the identity reserve, rather than extending the deadline. Identity creation and identity-ready publication happen
only after this succeeds. Fixture rebuilds remain within the original 300-second
server readiness deadline; no download occurs there. Remove post-barrier ambient
CARGO_HOME/GITHUB_PATH lines.

## Narrow compile-only session proof

Extend `run_bounded` with `build_scope=False`, accepted only for runtime=False and
the fixed fixture compiler worker. The unique marker and ordinary child registry
remain unchanged. Write a bounded scope record before spawn, binding that exact
marker, fixed worker/source+lock hashes, deadline and stage. Finalize it with the
exact tracked leader identity immediately after spawn, before normal polling.
The worker blocks reading stdin; run_bounded delivers that input only after the
ownership record is finalized, so no additional handshake is needed.
The source/lock hashes are checked again before execution and after completion.

On Linux, `_linux_scan` has one explicit compile branch. For a process with this
exact scope marker (including global marker=None scans), use this branch
exclusively, with no runtime _verify_process fallback. Accept it only while its `/proc/PID/stat` process-group and
session IDs both equal the tracked leader PID. Re-fence the leader's PID/start/
boot identity before and after this check, retaining it unreaped. Re-fence the
candidate identity around reading stat. No generated-build-script path rules,
hardlink exceptions, external-executable authorization or ancestor walking are
needed. Fork-before-exec children satisfy the same session proof. Any marked
process outside that group/session fails closed and is never signalled. No
native runtime call can request this branch. A group member that changes pgrp
inside the reserved session is also refused. The existing runtime executable,
configuration and namespace authentication path stays unchanged.

Cleanup first uses the existing reserved-leader TERM/KILL/group-absence proof,
then performs two marker scans that signal nothing and must both return empty.
A surviving marker, including a setsid escape, retains private state and fails
cleanup; do not call the runtime `_clean_linux` signaling scanner for compiler
scopes. A global scan finding any remaining compile marker fails closed without
signaling because that later process does not own the unreaped leader.
Preserve a scope's cleanup allowance for at least the named existing
termination constants plus scan margin (20 seconds), within fixture cleanup
grace. Expiry blocks new work, never prevents finishing exact owned cleanup.
Use one bounded scope record, not unbounded per-process observations. macOS keeps
its existing unreaped leader/group proof and the same fixed credential-free
compiler policy; this refinement does not alter native broker/shim controls.
macOS has no Linux-style marker escape scan; that existing limitation remains.

## Acceptance

Tests cover runtime scopes refused; exact marker and leader identity required;
wrong pgrp/session or reused leader rejected; generated build scripts and
fork-before-exec accepted solely through the reserved group; a marked setsid child
is refused without being signalled (test explicitly cleans its own captured
identity); final cleanup encountering an escaped compiler marker refuses it
without signaling or removing state; timeout/group termination bounded; two post-cleanup scans required;
poisoned source/Cargo configs and ambient wrapper variables refused; symlinks,
hardlinks and changed payloads in managed toolchain input rejected. Confirm real
managed 1.97.1 installations have no symlinks/hardlinks on all three targets; a
contrary observation fails preparation and requires a reviewed adjustment.
Assert the identity/cleanup/report deadline reserves. Run actual
locked fetch, offline warmup and three incremental fixture builds through this
boundary, native fixture byte/receipt tests and installed negative bridge policy
checks. Then run the installed nine-pair workflow. Compilation is preparation,
never transport latency or throughput. Integrated actual Claude code review is
required before publication; unavailable bootstrap or network produces finite
preparation failure with final cleanup evidence.
