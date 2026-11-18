# Peer native Leave C2 qualification

Linux AMD64 source `b794a47ffb8dbaa7763322aa97617f3c29af5e6e` uses
upstream MDK master `b3cd2cb1`, account schema 214, and Cargo.lock SHA-256
`c12c61906e8be867e36ecee22bd09ba4feabf73b855572f405d37e162b6f9ea4`.
The [C2 design](../phases/peer-native-leave-c2.md) applies to this opt-in local
proof.

The native fixture creates genuine founder and recipient state with new keys.
It verifies signed Offer, Consent and Delivery, plus the exact native
Welcome, before recording Pending and a durable C1 hold. A separate process
gets a fresh profile lease and the one-use epoch-6 reservation. It opens the
original SQLCipher stage under the managed DELETE-journal dispatcher and
runs native Leave in a single poll. The closed adapter and scheduler
remain unused. The owner requires one local failure ID and no success
report, fanout or queued intent. The proposal recovered from disk must
match that ID when the result is available.

The disposable child exits without running destructors after committed
reservation, after the native transaction and close, and after committed
capture. Each cold
process recovers using private durable state only. The first cut leaves
the original image byte-for-byte intact and returns indeterminate without
another native dispatch. After the second cut the owner authenticates the
native proposal and its separate SHA-256 marker before sealing. The
third cut preserves one immutable record and the same event ID on repeated
readback. A different inode is refused. A separate child exits during
the original managed scope after a synced DELETE journal and a successful
database page write. The cold owner refuses both normal and verifier open
of the hot original. The test compares the original digest and journal bytes before
and after refusal. The instrumentation identifies the scoped SQLite
write boundary and does not isolate hydration from later native work.

A pre-existing snapshot and an occupied logical extent consuming the
profile budget each block original dispatch. The test verifies that
preflight preserves the original image and both obstructions.

The fast SQL tests verify exact epoch inventory, refusal of incomplete
schema or direct writes, explicit transaction rollback of the write
aperture and effect, durable wall equality, duplicate reservation and
capture, exact deadline refusal for both apertures, and preservation of
the Fenced and charged stage. The clock test covers stop, wall and suspend-inclusive tick
regression, and refusal at and beyond expiry. Additional process modes
check a different generation and stop on either side of native dispatch.
The actual recovered candidate uses the established SelfRemove verifier
for account, signer and MLS leaf, group, canonical epoch, outer signed
event, and route.

## Reproduction

All commands use the exact Linux AMD64 source and lock above. The native
commands use bundled SQLCipher.

- Native C2 with real subprocess cuts. `cargo test --offline --locked -j 2
  -p marmish-native --lib peer_terminal_leave -- --include-ignored --nocapture`
  passed all four tests in 122.23 s.
- Standalone SQL C2. `cargo test --offline --locked -j 2 -p marmish-native
  --test peer_cursor_store peer_terminal_leave_effect` passed both tests.
- Existing quota dispatcher regressions. `quota_ --ignored --nocapture`
  passed all seven entries in 29.57 s using the same native test executable.
- Strict native lint. `cargo clippy --offline --locked -j 2 -p marmish-native
  --all-targets -- -D warnings` passed. Scoped Rust formatting and
  `git diff --check` passed.

The preceding implementation commit `65c55ea` also passed all nine
`epoch_five` regression tests, the existing opt-in
`peer_pending_unrouted_leave_recovers_exact_event` and
`exact_postjoin_commit_is_recovered_and_delivered_once` native regressions,
and the full standalone `peer_cursor_store` suite (54 passed, four opt-in
ignored). The follow-up changes only the two C2 test files.

## Scope

C2 demonstrates one bounded local native Leave dispatch and recovery of
its authenticated proposal from the closed SQLCipher stage. The test
mounts no relay socket and does not enable installed Peer or publish a
Leave. Founder convergence, recipient removal, physical slot release and
native hot-journal repair remain separate phases. The stage stays Fenced
and charged.

The native tests use one known fresh group. Cold Captured readback uses
two exact indexed IDs and rechecks authority. Armed and Absent inspection
also check complete matching-or-later Sent inventory. The C2 process
suite does not independently mutate genuine rows for every account,
founder, leaf, route, record-count, epoch, format or size refusal. The
shared SelfRemove verifier retains its existing signed event, authority
and MLS checks. macOS ARM64 remains unverified for C2.

## Integrated Linux rerun

Composed source `1a973f0` passed the two fast `peer_terminal_leave_effect_`
unit tests and all nine `epoch_five` regressions. The opt-in
`peer_terminal_leave_completes_in_one_managed_poll` passed with the real
native runtime and independent cold subprocess cuts in 119.65 s. Strict
all-target Clippy passed in 32.97 s. The C2 source and approved design
paths were byte-identical to the author revision after composition.
