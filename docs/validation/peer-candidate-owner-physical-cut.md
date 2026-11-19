# Ordinary owner physical dispatcher cut validation

Source `a95f4e4453d0839171fc5270b2de30e2795a1dee` was checked locally on Linux
AMD64 with Rust 1.97.1 and mainline MDK locked to `bdf9833`. Cargo used offline
locked dependencies. The focused stock libtest build took 44.37 seconds. Its
SHA-256 was `009da3cf6930c647d2ee49743a18181e4103ba004897901ffb77c8f752d461d2`.
Candidate Clippy covered all targets of `marmish-native` and `marmish-engine`
with `--cfg marmish_peer_receiver_candidate` and `-D warnings` in 29.05 seconds.
Formatting and the Git diff whitespace check passed.

The stock libtest invocation for `mdk_adapter::physical_cut::tests` with
`--include-ignored --test-threads=1` passed all four cases in 26.82 seconds.

| Test | Observed result |
| --- | --- |
| `cached_cut_handles_survive_cancelled_observation_and_failed_join` | A cancelled observation preserved unfinished work. SDK and raw barriers completed while the forwarder stayed pending. A failed forwarder join was recorded once and the parked owner kept the lease. |
| `ordinary_owner_physical_cut_over_private_wss` | The authenticated loopback socket closed at the raw layer and the live child reached its release marker. All five barriers settled successfully. Terminal binding authority retired, the profile lease remained held, and the relay's authenticated request, close and publication counters stayed unchanged. Journal and MDK database images and sidecars were byte-for-byte unchanged. The ordinary graceful stop subsequently rewrote the same live-stream row and changed its Journal image in the disposable test. |
| `previously_closed_slots_remain_incomplete` | Closing the connector after constructing an owner yielded a persistent incomplete raw barrier. |
| `rejects_without_cut_authority_and_preserves_the_owner` | Refused a missing runtime, missing or untripped fatal observer, a mismatched profile lease, an owner already stopping and a Command engine with a different trip. A non-direct connector refused fatal attachment. |

The stock libtest also passed
`command_engine::tests::fatal::full_queues_disconnect_before_join_and_keeper_retains_lease`
in 0.02 seconds and
`transport::tests::fatal_drains_each_exact_route_slot_and_blocks_new_connects`
in under 0.01 seconds.

The fixture used a synthetic dispatcher cut with a real ordinary native owner,
Command engine, runner and private WSS relay. The daemon mount, takeover of
custody already transferred to graceful shutdown and actual SQLite default
dispatcher displacement are separate qualification. Published Command behavior
and installed candidate startup stay unchanged.
