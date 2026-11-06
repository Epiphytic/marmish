# Streaming primitives validation

Phase 1 of `marmish-8ka`, Linux AMD64 local source verification. No daemon or CLI
stream capability is enabled by this increment; no live-relay or installed
cross-platform result is claimed. Native CI now includes this crate in its
existing three-platform test matrix; those remote results are not yet observed.

## Local checks

- `cargo test --locked -p marmish-stream`: 16 passed, zero failed.
- `cargo clippy --locked -p marmish-stream --all-targets -- -D warnings`:
  passed on the final corrected implementation.
- `python3 -m unittest discover -s tools -p 'test_marmish*licenses.py'`:
  19 passed after reverting the unnecessary native license-root change.

Tests cover all wire variants at maximum payload, canonical JSON/base64/hex,
unknown fields, invalid offsets/EOF/ACK, new-Open age boundaries, cancellation
snapshot consistency, random/NUL bytes, incompressible fallback, corruption,
dictionary/truncation/trailing-frame rejection, bounded decompression and
nonbeneficial compressed representation rejection. An 8 MiB transfer traverses
256 KiB windows and reclaims only consumed prefixes; tiny chunks and retry
receipt metadata remain capped at 256 records.

The independent code reviewer also reports scratch randomized checks: 300,000
mutated requests with canonical re-encoding, 200,000 random compressed frames,
and 2,000 receive-window model rounds. These are reviewer-run experiments, not
committed reproducible test fixtures. See the
[actual review and disposition](../reviews/streaming-primitives-code.md).

`cargo fmt --all -- --check` and `git diff --check` pass. Final independent
Claude code-review verdict: **APPROVE**, after correcting the new regression
test’s unnecessary `vec!` allocation flagged by strict clippy.

## Structural review

`ripwire . --quality-delta` was run against git HEAD. It reports 26 gating rows,
not a clean gate. The report conflates unqualified names (`hex16`, `offset`,
`encode`, `decode`, `validate_request`) with existing definitions; for example
it reports increased nesting in unchanged `engine/admission.rs`. The exact diff
contains no engine, native command, or prototype implementation change.
Serde macro-generated hex modules and test/serialization entry points appear
unused in its static graph. Similar argv serialization and closed typed envelope
wrappers intentionally remain independent of the native crate, avoiding an
inverted dependency from pure stream primitives into the daemon. The actual
shared canonical-base64 decoding duplication was removed. No unrelated code was
refactored to alter the metric. `ripwire . --pr-context` was also inspected; its
packaging impact led to the license regression check and reverted root edit.

Private keys, profile state, databases, journals, target output, temporary files
and logs remain excluded by the existing `.gitignore`. Only source and sanitized
review/validation documents are staged. Native production dependencies and
finite command behavior remain unchanged in this phase.
