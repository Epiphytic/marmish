# Installed private Manual listener handoff

The exact Rust source was `7943903e2753e6e62be9d28a9734069b456d8696`
with the upstream MDK `master` lock at `e4421e86`. On Linux AMD64,
the private example ran against a clean native installer layout using
a digest-pinned manifest. The daemon and example embed the full source
commit. The immutable native test runner came from the same source and
target, and was placed in the installed version's test-only `deps`.

| Artifact | SHA-256 |
| --- | --- |
| Candidate manifest | `f4a3a599939bf9841b3d1ba6e1cd103eceef7794864ad78ab8c3df04ef1bb8fa` |
| Candidate installed daemon | `8e975def2c735a08113e0b184138f1e72a82dfade12dfae7ff22595334fc0601` |
| Candidate private example | `2b42e4ab9c6b0638df6379d11825f82e80f766f177894ac65e034683cf6e7e8c` |
| Candidate test runner | `45619c32b0ee89934041683a68419d661ebc059c978d56010f8365b0728189bc` |
| Separate stock manifest | `f90fa4d430c661ac0c3b2513122028b42207940eff358036ffea9509af167bee` |
| Separate stock installed daemon | `9758493eec6e67f62f0eab374e79db182fad085c22bfe9ee643b33fbf7509789` |

Candidate and stock test builds passed. All-target native Clippy with
`-D warnings` took 40.96 s for candidate and 30.79 s for stock.
Both stock launchers and both public candidate launchers refused the
private `--manual-recipient` argument at parsing. Only the private
example accepted it at the documented position.

The actor lifecycle controller passed twice, in 52.54 s and 56.04 s.
It exercised child-process startup gates, open refusal, stop and panic
while holding the view, exact raw route drain, and unfinished join
while reap remained parked. The actual Manual and Passive listener
pairing cases passed in 8.10 s and 5.94 s. Manual admitted the actual
view's Reader while the actor was live and refused the clone before the
same listener drained. Passive held no view. The foreign Fatal check,
three private launch tests and 25 Peer read regressions passed.
[Claude Auto independently approved the exact source](../reviews/peer-candidate-manual-handoff.md)
and reproduced both listener checks.

Both installed controllers used private loopback WSS, disposable identities
and two independent Command and Peer relay origins. They observed
provisional readiness, a deliberate hold with no Peer connection, exact
startup Commit, and a single authenticated recipient subscription.
Same-UID Peer reads returned `peer_unavailable` while the Manual actor
held the live port. Binary-safe ordinary Commands worked before and
after an exact Peer disconnection. The controllers proved zero Peer
publications, preserved the unrelated foreground descriptor, observed
raw disconnect on SIGTERM, and checked that the cold SQLCipher profile
could reopen through the ordinary binding. The reopen uses the controller's
stock-style library path. The separately installed stock daemon was
qualified for packaging and private-option refusal in this increment.

| Single-run measurement | Passive | Idle Manual |
| --- | ---: | ---: |
| Provisional ready | 6,241 ms | 6,104 ms |
| Deliberate pre-ACK hold | 2,000 ms | 2,001 ms |
| ACK to authenticated Peer connection | 230 ms | 223 ms |
| Binary-safe Command with Peer connected | 8,156 ms | 7,327 ms |
| Binary-safe Command after Peer disconnect | 2,683 ms | 2,501 ms |
| Foreground stop | 78 ms | 78 ms |
| Full selector | 39.20 s | 38.42 s |

These results qualify one private Linux AMD64 listener and its inert
Manual actor. Actual invitation intake, positive Pending and Healthy
Empty views, Auto, heartbeat, macOS, ARM64 and external relay performance
remain outside this run. Pair-counter overflow and OS thread resource
exhaustion were not forced.
