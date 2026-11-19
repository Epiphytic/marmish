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
