# Explicit Keychain authorization validation

Beads: `marmish-0or`. Base: `01e3608d75ad52d6c335ecbbdc0912fdff89ba64`.
Native package version: 0.2.2. This record covers the local Linux source checks;
it is not a release, macOS consent or user-recovery result.
Subsequent hosted and published results are bound separately in
[corrective-publication.md](corrective-publication.md).

## Local checks

`CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo test --locked --workspace`
completed with exit 0. The native library reported 358 passed (365.71 seconds),
and the vault library reported 12 passed. All other invoked workspace suites and
doc tests passed. Tests that explicitly require a real native provider remain
ignored unless separately invoked; this run is not provider qualification.

Focused native identity tests (22), authorization tests (7), CLI-filter tests
(25, including existing command CLI tests), and the reserved-alias test passed.
These are overlapping filters, not additional disjoint totals. Strict
`cargo clippy --locked -p marmish-native --all-targets -- -D warnings` and the
corresponding vault command passed. Workspace formatting and `git diff --check`
passed. Final shared-serializer changes were included in the full workspace
test run.

Tests cover exact existing references and helper order, no verification after
failed authorization, strict request/response schemas, status/payload agreement,
watchdog timeout classification, original deadlines, cancellation and reaping of
real owned child processes, oversize responses, no-write directory snapshots,
refusal of unsafe/in-use/unready/file profiles, explicit-global argument
rejection in both positions, and Linux/frontend restrictions. Finite native
status mapping and process-local UI restoration tests are macOS-only and were
not run on Linux.

## Quality findings

Ripwire `--quality-delta` against git HEAD returned exit 2 with 64 findings,
including 17 gating findings; this check is not reported as passing. Actual
profile-constructor duplication and bounded-response serialization duplication
were consolidated. Remaining gating findings are enum-to-string matches with
unrelated semantics, the mandatory child kill/reap `Drop` shape matching an
unrelated test daemon, a shell-fixture construction shape, distinct closed
ordinary/authorization response DTO matches, historical churn, and the existing
native error callback passed through `map_err` being classified as dead code.
No finding was suppressed or rebaselined. The controlled exchange retains the
existing bounded polling loop and adds cancellation/status handling; its measured
complexity requires human/code review rather than a claim of a clean metric.

## macOS acceptance still required

The reported 0.2.1 denial persists after the user successfully unlocks Keychain.
That observation does not establish its exact OSStatus or ACL cause. The new
foreground command requests consent for the existing profile's two items, then
checks access in a fresh noninteractive helper. It creates no identity or keys,
changes no backend and rewrites no ACL. A successful check proves current access
for that canonical helper, not database integrity or full daemon startup.

An interactive macOS session must still establish persistent consent, fresh
verification and subsequent ordinary daemon startup with the same identity.
Cancel, locked-Keychain and interruption paths must remain failures. Headless
CI cannot approve a consent dialog and does not establish recovery of the user's
failure. Hosted compilation and provider results, when available, must be bound
separately to the exact source/run/attempt.
