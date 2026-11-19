# Guarded Manual recipient ingress over private WSS

The Manual ingress keeps the existing encrypted cursor under the recovered
profile owner and uses one registered, NIP-42 authenticated recipient port
for both history and live events. A port can claim Recovery once. A checked
short page commits the exact cursor revision and completes the scan. The
cursor proof stays private. A caller stops scheduling history at that edge
and retires its port and owner on an ingress error.

The guarded classifier shares the installed Offer and Delivery paths. Its
turns check the original owner and port. Final transaction checks fence
the registered dispatcher lifetime, startup readiness, stop and current
native route horizon. A measured Delivery also checks exact dispatcher lifetime on its fresh route
before opening a native window. The cursor checks the stop fence after
its SQL mutation and immediately before commit.

## Exact private WSS and SQLCipher results

Linux AMD64 author source `15aa34c453e5011e4947ab01bf3780acee6cba16`
used upstream MDK `bdf9833e340909f29c628f4fc2f9a3d6873f1cf5` from its
lockfile. The native ignored selector
`manual_ingress_attributes_history_and_live_on_one_private_port` passed
in 28.75 s. It runs four isolated child processes with disposable keys,
real private TLS/NIP-42 authentication and a per-child 40-second watchdog.

| Case | Observed behavior |
| --- | --- |
| Exact history and live | Four expired Offer carriers fill one page. A terminal short page completes recovery. One fresh Offer is accepted on the same live port. Idle returns no status. Duplicate recovery, an alien dispatcher observer and a stop are refused. |
| Oversized history | A real oversized history response supplies no completion. Subsequent live ingress is refused. The durable page revision, retry count and custody count stay unchanged. |
| Founder Backlog | Multiple authentic live Offers from the same founder produce the existing typed Backlog. Subsequent live ingress is refused. The durable page revision, retry count and custody count stay unchanged. |
| Measured Delivery | A signed Delivery with actual native Welcome bytes takes the current package measurement path and records the exact carrier in the durable deferred retry queue. |

The failure scenarios check unchanged recovery page revision and unchanged
retry and custody row counts. They also check that the failed ingress refuses
a subsequent live turn. The fixture's relay counts only the founder's intended carrier writes.
The ingress emits zero Peer carrier publications. The alien-observer
test checks the exact comparator at the live port; inspection confirms
the same comparator runs before native measurement.

The native `final_scan_and_page_refusals_roll_back_their_sql_writes`
test passed and reopened the encrypted database to check refused new,
resumed, advancing and quarantine transaction commits. Stock strict
all-target Clippy and test link passed on the exact source. Candidate
strict all-target Clippy passed at production-identical `85ba358`.
The standalone `peer_cursor_store` suite passed 78 tests with 4 ignored
in 81.17 s at `85ba358`. The subsequent commit affects only two
native test files and adds the Backlog scenario.

This qualification covers the guarded ingress component. Installed
daemon startup and scheduling, the read view, guarded outbound Consent,
native retry expiry and full invitation acceptance remain separate from
these tests.

## Linux AMD64 composition with expired Delivery retirement

The composed source `5b423ba1121fba8ab5c3636fcfe032249c0c204a`
retains upstream MDK master locked at
`e4421e86a6c73f336b0154ee09c13826d8fafff5`. Strict all-target native
Clippy passed in stock mode (38.37 s) and candidate mode (33.24 s).
The stock libtest SHA-256 was
`dc3b2e18b1eb978a13661f648f40bfbd2949bffdfa421494d85dd98ce4f7e7be`.

| Composed stock libtest selector | Result |
| --- | --- |
| `manual_ingress_attributes_history_and_live_on_one_private_port` | Four private WSS child cases passed in 31.76 s. |
| `managed_expired_delivery_private_wss` | Two private WSS child cases passed in 44.78 s. Measured native retirement took 402.920 and 421.189 ms. |
| `final_scan_and_page_refusals_roll_back_their_sql_writes` | Passed in 0.10 s. |
| `expiry_refusals_preserve_corruption_and_reobserve_stale_rows` | Passed. |
| `peer_cursor` | 15 passed, 1 opt-in skipped in 16.69 s. |
| `managed_consent_single_port_private_wss` | Seven existing private WSS child cases passed in 118.25 s. |

Scoped Rust formatting and diff checks passed. Workspace-wide formatting
also reported unrelated existing differences inherited from the base. Each
live case ran on loopback with disposable profiles. The candidate build was
checked statically. The composed executable for this validation was the
stock test build, which includes the Linux AMD64 candidate-only test paths.
