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

## Runner uncertainty amendment

Exact source `b071e45fec9efbe57721afdf385f677efd202f7d` was retested
on Linux AMD64. Its stock libtest build took 41.84 seconds, with SHA-256
`65eb26c6caceb35e3ec3bc603514881702ce2a6ea44e49707f8203b910fefc43`.
Stock Clippy passed both packages and all targets with `-D warnings` in
28.10 seconds. Candidate all-target Clippy passed those same packages with the
Linux AMD64 candidate cfg and `-D warnings` in 28.05 seconds. Formatting and the
Git diff whitespace check passed.

The exact stock archive passed these selectors:

| Selector | Passed | Time |
| --- | ---: | ---: |
| `mdk_adapter::physical_cut::tests` with `--include-ignored --test-threads=1` | 6 | 19.29 s |
| `stream_runner::tests` with `--include-ignored --test-threads=1` | 27 | 14.42 s |
| `command_lifecycle::tests` with `--include-ignored --test-threads=1` | 6 | 3.97 s |
| `command_engine::tests::fatal::full_queues_disconnect_before_join_and_keeper_retains_lease` | 1 | 0.02 s |
| `transport::tests::fatal_drains_each_exact_route_slot_and_blocks_new_connects` | 1 | <0.01 s |

The real thread catch regression gated the runner before child spawn and
observed Pending while the exact profile lease prevented reacquisition. After
the injected panic, the real shared catch path published both the finished and
uncertain flags. The cut observed PossiblyLive with the other four barriers
Complete. Repeated finish calls returned the cut with the same report and the
flock still held. The private WSS control used a real live child, observed
finished without uncertainty, and completed the cut. A cancelled outer
forwarder join and already-closed raw slots stayed PossiblyLive. A separate
bounded task test distinguished the keeper returning a known-ended failure
from the keeper handle ending unsuccessfully.

The injected panic creates no OS child. The regression proves that uncertain
runner evidence blocks custody transfer at this leaf. Keeping the actual daemon
process alive after a real SQLite dispatcher displacement still requires the
daemon composition qualification.

## Exact composed source

On Linux AMD64, stock and candidate artifacts for
`f1c5a00c669162f12702ab2a98c676be4aa033e5` passed strict all-target Clippy
in 31.24 and 32.74 seconds respectively. Each passed 33 engine tests, six
physical tests and six lifecycle tests. One engine and one physical test
per build were skipped as opt-in. The physical suite included a real
private WSS owner, reaped child, closed transport and unchanged stored
state after the last admitted effect.

The engine explicitly records successful worker joins. Returned failure
or missing join evidence keeps the lease held. A focused fixture
separately proves that a reported failure before any child is started
permits a failed disposition once all other barriers have completed.
The registered daemon still needs an installed end-to-end stop test.
