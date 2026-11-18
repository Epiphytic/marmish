# Private Peer TLS and local qualification validation

The [phase design](../phases/peer-installed-relay-trust.md) was exercised on
Linux AMD64 at source `359d9d8821277ad7e2922d92e4073f294eab7d3c` with upstream
MDK `59b7ed3ff007494b25f8404c144c1d82809b3973`.

The tests generated disposable certificates and used a local loopback WSS
listener. A bound connector upgraded only the exact endpoint with its pinned
private CA, and rejected a wrong SAN and an unrelated CA. The SPKI fixture
used the Peer-specific verifier with generated trust roots. Reissuing the
same public leaf key succeeded, while rotating that key stopped the actual
TLS connection before the relay saw a WebSocket upgrade. The connector
declined an exact-origin mismatch and proxy before dialing. Replacing the
pinned CA file caused the next connection to fail before dialing. Dropping
the profile owner refused an attempt through a previously cloned connector.

The certificate checks also cover an expired leaf, expired and future roots,
a simulated anchor expiry after a real handshake, CA file links and modes,
and chain count and byte limits.

The local record tests verify strict scope, lease and file identity, mode and
link checks, an 8 KiB read limit, expiry and backward time, and replacement
across failures before and after rename. A failed pre-rename turn cleans
its own staging inode and preserves an unexpected replacement inode. An expired persisted observation
cannot be revived by rolling the wall clock back. Local readback reports
metadata, and the ordinary runtime has no caller for the bound owner or
evidence writer.

| Exact frozen-source check | Linux AMD64 result |
| --- | --- |
| New TLS and record tests | 8 passed |
| Existing command and Peer config parser | 26 passed |
| Existing identity | 22 passed |
| Existing transport | 24 passed |
| Existing raw Peer core | 6 passed |
| Existing opt-in raw Peer loopback WSS | 9 passed in 1.06 s |
| Existing opt-in durable native PostJoin lost-ACK recovery, with same-build daemon | 1 passed in 58.47 s |
| Strict native all-target Clippy, including the `installed-checks` feature | Passed in 21.44 s |
| Formatting of the changed Rust files and Git whitespace check | Passed |

An additional broad default suite started on the superseded initial code
and was stopped once the review required a new source revision. The WSS
durations are harness runtimes under local load. This phase provides
no latency or throughput measurement for a supported relay, production
qualification result, release-platform evidence, or installed Peer traffic.
