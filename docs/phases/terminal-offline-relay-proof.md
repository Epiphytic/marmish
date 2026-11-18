# Offline terminal relay and receive proof

Continue the [staged ephemeral route proof](terminal-ephemeral-route.md) after
the [in-vitro proof](../validation/terminal-memory-proof.md). This increment
qualifies a disposable loopback relay, native receive dispatch and an initial
fixture resource ceiling. Bootstrap aliases, capability negotiation, Terminal
wire effects and PTY work have separate implementation gates. Keep production
Command discovery, early-Welcome fallback, routing, authentication and retention
intact.

## Implementation split

1. Extract the necessary endpoint, pending-package and signed-artifact setup
   into a private test-only `terminal_room_fixture.rs`, keeping the existing
   test entry point and its two founder orientations. Share bounded, real
   native event captures by ID across later fixtures. Add an observed
   KeyPackage publish-attempt counter before the existing panic guard. Check
   the attempt counter at setup, join, reconnect and teardown, and require
   zero Terminal-originated kind-30443 writes observed by the fixture relay.
2. Add a test-only `terminal_relay_fixture.rs` with a single ephemeral
   loopback WSS listener, bounded events and transcript, and explicit
   per-connection authentication and subscriptions. Use the pinned
   `nostr` and `nostr-sdk` revision `48801bf6f2f1cd55bb1729e94159946c8832a786`,
   MDK and its SDK-enabled adapter at `8abc51befbf71a1ae3bdbb25eb0860c731e75db2`,
   and existing `tokio-tungstenite` 0.28, `tokio-rustls` 0.26, `rustls` 0.23
   and test-only `rcgen` 0.14.8. In `transport.rs`, add a private
   `#[cfg(test)]` connector constructor, fixture-root field and branch in
   `RelayConnector::finish_socket` to call `tls_stream_with_roots`. Bind
   the listener only to `127.0.0.1` and the private trust configuration to
   its exact `wss://127.0.0.1:<port>/` URL. Give the generated leaf an IP
   SAN for `127.0.0.1` and keep the fixture CA key in memory. A connector
   without fixture configuration uses the standard public roots and rejects
   the fixture certificate. An explicitly configured fixture connector rejects
   any other URL, including the same address on a different port. Assert
   those failures with the native SDK client and compile the ordinary
   connector without tests using `cargo check --locked --offline -p marmish-native --lib --bins`.
   The fixture constructor, field and branch compile out of that build.
   A source-revision comparison can separately use the same test-only trust
   seam. This increment makes no claim about old releases or installed
   binary interoperability.
3. Add `terminal_route_proof_tests.rs` for real SDK delivery through the
   pinned adapter, an independent direct adapter check with captured wire
   context and actual durable Command positive controls. In
   `MdkOwner::receive_one`, add only a bounded per-instance
   `#[cfg(test)]` observer immediately before the call to
   `runtime.ingest_delivery`. Enumerate all native `ingest_delivery` call
   sites during implementation review. The current production path calls
   it once from `receive_one`. The direct calls today are in test fixtures.
   Require every durable route under test to pass the observer before
   interpreting its zero count. Keep observer and capture state out of
   runtime artifacts.
4. Measure the fixture resource ceiling in an opt-in subprocess. Reuse
   established rooms where fault ordering permits. Test actual cold
   restarts for the critical route cases. Use a deterministic clock for idle
   expirations and an external deadline for SQLCipher and process setup.
   Report the measured ceiling with sample counts and limits.

## Relay authorization and pre-ingress checks

Issue a unique NIP-42 challenge per connection. Check the AUTH event kind,
signature, challenge, relay URL and freshness before associating its signed
account with the connection. Require each experimental kind-1059 filter to
contain exactly one `#p` matching that account. Fail unauthenticated,
wrong-recipient and broad reads explicitly. Require a scoped group route for
kind-445 reads. Verify valid outer signatures for writes. In the qualifying
mode, accept previously unknown experimental AUTH accounts and valid
kind-1059 and kind-445 outer authors distinct from the connection signer.
Use a priori ordinary Command accounts for its positive controls. In
separate negative modes, deny unknown AUTH accounts and require write
authors to equal the AUTH signer on experimental connections. Each mode
must leave ordinary Command traffic available. Keep events in fixture
memory, including traffic held for misrouting and replay.

Record actual issued subscription IDs from SDK relay REQs. For the direct
adapter case the synthetic relay client records IDs from the actual
`NostrSubscription::subscription_id` returned at registration. First show
that an intact signed Terminal Welcome and group event each reach the
matching Terminal adapter and that independent, valid Command deliveries
reach the Command observer. Then deliver the exact Terminal events to the
Command adapter on active Command IDs and to every other room's separate
adapter on that room's active IDs. Exercise live routes, post-teardown
replay and receiver restart on either side. Send EOSE and independent
matching positive events through each relevant window before checking the
zero counters. Use separate fixture receive state as needed to prevent a
prior matching delivery from masking the direct adapter check.

The pin exposes `NostrRelayEvent` with relay endpoint, subscription ID and
signed event and publicly exposes `handle_relay_event` and
`handle_group_relay_copy`. If the SDK discards a deliberately mismatched
delivery, record that observation and additionally send the exact event
under the actual issued ID directly through the adapter entry point. Use
the group-copy entry point for the raw SDK path, too. Require zero
deliveries on the Command and cross-room adapters, including through the
direct adapter entry point. Require zero pre-ingest hits and zero matching
markers in the live Command store and again after reopening with the
disposable test SQLCipher key. Query the exact outer Welcome ID with
`MessageStorage::has_ingress_dedup_marker` and the outer group transport
ID with `has_processed_transport_id`. Record the row-count delta in
`cgka_ingress_dedup` and `cgka_processed_transport_ids` before and after
the negative batch. Use a fixture seed and batch below marker eviction
capacity and exclude concurrent unrelated deliveries. Test positive controls
separately to show the observer and expected durable records are visible.

Characterize the same-adapter case separately. The pinned `routes_for`
matches the signed recipient or group destination and relay endpoint. On
the generic `handle_relay_event` path an event correctly addressed to that
adapter can be delivered as `Untracked` on a foreign or unissued
subscription ID. Expect that result and record it. The raw group-copy
entry point additionally requires a live group window owned by the
issuing account. Enforcing the exact subscription on every same-account
delivery would require a separate reviewed local lane gate.

Verify early group delivery with an unknown route drops or retries within
the bound without pinning the room indefinitely. Verify an independent
Terminal sync and deactivation leave Command's existing acknowledgements
and pins available. Stop if the cross-account or cross-room check admits
an event addressed elsewhere, a Terminal event reaches durable Command
dispatch, routine Terminal work attempts a KeyPackage publication, or
isolation requires changing the MDK pin or Command trust or fallback.
Preserve the failing fixture and obtain review for an isolated Marmish
lane gate before attempting a local fix. This proof uses the unmodified pin
and the existing Command subscription and authentication policy.

## Bounds and acceptance

Run the NIP-42 positive case and each fixed negative mode with a successful
allowlisted native Command between changes. Compare exact exit status and
bytes, including NUL and shell metacharacters. Keep restriction, rotated
package and retained-bundle behavior coverage in the staged Command suite.
Record what the relay and adapter saw and the ingress and persistence
counters. Perform installed interoperability, the full durable-store scan
and full fault qualification separately wherever the fixture cannot provide
them.

In the fixture gate, allow one underway invite, up to four concurrent rooms
and at most two rooms per peer. Verify refusal of a second simultaneous
invite or a fifth room before fixture allocation or dial. Keep the Command
permit independent and count at most six fixture sockets for experimental
traffic per URL, over no more than 16 configured endpoints. These
fixture-side checks do not qualify production admission. Exercise reconnect
and release with injected Tor permit and route checks as appropriate. Live
Tor circuit use remains unqualified in the loopback test. In an isolated
subprocess collect incremental RSS, locked memory, `RLIMIT_MEMLOCK`, live
descriptor and socket counts, per-endpoint migration time and ordinary
Command p95 at zero, one, two and four rooms. Maintain the approved frame
and queue limits. Require a functioning fourth room and healthy Command
delivery with measured host headroom to report four-room qualification.
Report the observed ceiling and its limiting resource if the fourth fails.

The first stage observed about 2.6 seconds to start both endpoints of one
room in debug mode, or about 10 seconds for four rooms plus Command setup.
Give four-room initialization up to 60 seconds and each ready fixture
protocol wait up to five seconds. Use a 180-second external wall-clock
deadline per opt-in test subprocess, including synchronous initialization
and cleanup. Review a small fast dispatch test for routine CI only after
the offline proof is approved.
