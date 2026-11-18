# Authenticated Peer recipient transport validation

The reviewed source is `c64ffad458fdc6e7d956a73c5a47f88c1a2c0f35`.
The private fixture ran on Linux AMD64 with Rust 1.97.1 and locked upstream
MDK `bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`. It used disposable
file-provider credentials, a loopback WSS listener with a private CA, and
native SQLCipher. The native port and final write fence compile in ordinary
builds. The fixture supplies the otherwise unavailable sender and receiver
admission constructors.

## Installed state exercised in the fixture

The qualification used four sockets. After all four closed, the fixture
reacquired the Ready profile and used the same listener, certificate, and
CA. The candidate port authenticated by NIP-42 and received the live
subscription's exact EOSE. A v2 history Offer and a live Offer from a
second founder yielded two durable markers and the committed history cursor.
After closing and reopening the checkpoint, both markers were readable.
The misaddressed and v1 Offers had no installed marker. The v1 Offer
crossed the transport and was refused by the installed v2 checkpoint.
The misaddressed signed Offer was refused at the relay.

The exact checked positive send produced a single relay receipt. The fixture
compared its recipient, account, carrier ID and SHA-256 of the exact signed
bytes. A lost ACK produced one observed publication and an ambiguous
outcome. Wrong account, digest, generation, a withdrawn row and another
socket's scope produced zero further EVENT attempts. A stale acceptance or
foreign history page could not invoke an observation callback. The cold
checkpoint contained marker state only. No native KeyPackage, Welcome or
group custody was created.

A bad or missing AUTH acknowledgement and a mismatched live EOSE
prevented readiness. Same-byte configuration inode replacement just as the
relay's correlated AUTH ACK or live EOSE entered the real socket withdrew
the route. Replacing the CA inode also withheld an already buffered history
page. The original held files were restored for independent checks and did
not repair the retired sockets. An idle, authenticated socket retired at
the actual short-lived WebPKI leaf certificate boundary.

The checked port's injected SDK sink counted readiness, staging and flush.
At the exact permit-wall expiry it counted `0/0/0`. A durable row
withdrawal while readiness was pending counted `1/0/0` and returned no
send. Cancellation during a pending first flush counted `1/1/1` and
returned possible exposure. None reached the physical relay. The raw
socket regressions separately cover the staged-but-unflushed boundary. The
injected sink panics on close to catch implicit flushing during retirement.

## Observed timing and checks

The exact focused run took 20.71 seconds, including a deliberate wait
through the test certificate's 20-second lifetime. Qualification took
3,982 ms, checked AUTH and EOSE took 99 ms, and history/live ingestion
through cold SQLCipher readback took 227 ms. The subsequent send and fault
batch took 16,399 ms including the certificate wait. These measurements are
one local sample.

| Selector or command | Result |
| --- | --- |
| `peer_authenticated_route`, including its opt-in WSS test | 4 passed |
| `peer_raw_socket_core` and `peer_raw_socket_native_wss` | 6 and 9 passed |
| `peer_installed_offer` | 6 passed |
| `physical_route` | 2 passed |
| `peer_relay_trust` | 18 passed, one controlling-PTY CLI test skipped |
| `transport::tests` | 17 passed |
| `cargo clippy --locked -p marmish-native --all-targets -- -D warnings` | Passed |
| `cargo check --locked --release -p marmish-native --features installed-checks --bins` | Passed |
| `rustfmt --check` on changed Rust files and `git diff --check` | Passed |

The first private fixture attempts encountered the existing one-live-Offer
limit per founder and one-open-connection budget per connector. The final
fixture uses distinct disposable founders and independent connector
instances. Both bounds remain enforced in production.

Ripwire reported increased complexity in the fixture relay handler,
appearing alongside name-based structural matches and the expected
unmounted types. The relay's added fault choices are scoped to the
opt-in recipient fixture and the native socket regression selectors
exercise its established modes.

See the [phase design](../phases/peer-authenticated-route.md) and
[review evidence](../reviews/peer-authenticated-route.md). The installed
worker still needs paired durable recipient admission and a sealed v2
operation producer before it can start this path. Native installer
artifacts, other platforms and deployed relay operation were outside this run.
