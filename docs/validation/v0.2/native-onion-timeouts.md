# Route-aware connection timing: offline validation

This is a reviewed transport correction, not hosted Phase 1 acceptance or
remote-command support. The failed exact-source hosted run and its finite
diagnostics remain preserved in [the hosted evidence](hosted-foundation.md).

The [approved design](../../phases/v0.2-phase1-onion-timeouts.md) separates the
Tor stream budget from the TLS/WebSocket budget, preserves direct timing and
all original activation/check deadlines, and lets SDK-owned recovery dialing
survive short actor turns. Actual per-relay SDK options now receive both the
route timeout and the existing message limits. Recovery also clears stale
subscription state after a connection drops and preserves healthy forwarders.

Root's full offline run passed **117 native tests, 8 vault tests and 3 helper
tests**. The native suite took 199.98 seconds. The separately gated native
keychain integration test was intentionally ignored; this transport change
does not repeat or extend the prior provider qualification.

The [actual Claude completed-code review](../../reviews/v0.2-native-onion-timeouts-code-r1.md)
approved the integrated source and independently passed all **27 focused tests**
(15 transport, 9 startup, 3 recovery), **42 adapter tests**, formatting and
workspace production Clippy. Its 20 repeated recovery-suite runs all passed.
These tests use the production phase wrapper and the actual pinned SDK Relay
API with inert delayed transports. They do not establish Tor reachability.

Both root and reviewer found the same test-only `cloned_ref_to_slice_refs`
lint. Replacing the cloned one-element slice with `std::slice::from_ref` fixes
it without changing transport behavior. Root's 15 transport tests, native/vault
all-target Clippy with warnings denied and formatting checks pass afterward. Cargo.lock and
dependency revisions are unchanged.
The [final Claude confirmation](../../reviews/v0.2-native-onion-timeouts-code-r2.md)
approves the lint correction and evidence, independently passes strict all-target
native Clippy, and retains the requirement for fresh hosted acceptance.

Final source hashes:

- `transport.rs`: `8f7eb479d8a4b28d874f11b91c04b36964b0da6203954beb652a8eaa433e83a8`.
- `mdk_adapter.rs`: `b0c84e1e2034364e0886e48e79b247f5fb648295b1caa241235b4c60c12b7265`.
- Cargo.lock: `0d937b2ab2a1e04afa4b0cbf965fba92ca67c63437c77a39be7609aefd9c4a25`.

The design reviewed at SHA256
`05e8731377d70d3ff8a34a6484b5bd036e30b095e5c5c874ecd57d0ced19c6cf`
now hashes to
`c0bcc406b9c5a38f10ba0618f5a2e2784f1fde6018050d3179761ba5d0c2c919`.
Changes are its approval/status header and the review's useful clarifications:
explicit AddRelay size limits, the private recovery test seam, pending's existing
retryable result, and the owner publication-fence assertion. The route budgets,
global deadlines, route authority and phase exit criteria are unchanged.

The scoped quality delta returned **exit 2**, not a pass: it counts the existing
startup test module growing from 290 to 361 lines as a major verbosity finding.
One route-budget argument expands existing test call sites; the new test proves
the shared original deadline after bootstrap across multiple endpoints. Minor
production findings are recovery complexity 20→21 and activation length 59→63.
It also labels new test functions/trait implementation methods as dead code,
although the tests compile and pass. These tradeoffs are recorded without a
baseline or suppression. Formatting, actionlint and whitespace checks passed.

A fresh full four-job hosted run must still prove actual encrypted Tor exchange,
binary payloads, receipts, restart and cleanup on all client platforms. A fixed
deadline failure remains a failure; no budget expansion or fallback is implied.
