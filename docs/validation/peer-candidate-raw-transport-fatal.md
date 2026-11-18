# Candidate raw Peer transport fatal validation

Linux AMD64 source `5b319577a1cd5606eb236b5e304069a468907e5a`
qualifies the explicit receiver-candidate registered socket wrapper and
its drain. Stock compilation checks exercise the ordinary build. The
installed daemon has no signal issuer and does not use this socket in
this increment.

| Check | Result |
| --- | --- |
| `peer_registered_raw_fatal_private_wss --ignored` | Passed all 13 fresh subprocess cases in 56.97 s |
| `peer_registered_route_private_wss_handoff --ignored` | Passed all six fresh subprocess cases in 25.57 s |
| `peer_relay_trust::physical_route::tests` | 2 passed, including cold WSS, in 7.87 s |
| `candidate_registration_scope_and_irreversible_dispatch` | Passed all seven fresh subprocess cases in 25.05 s |
| `dispatcher_fatal` | 2 passed in both author and independent reviewer runs |
| Strict native all-target Clippy | Passed in 29.06 s stock and 28.60 s candidate |
| Scoped Rust formatting and diff checks | Passed |

The candidate libtest executable compiled and linked in 44.29 s.
SHA-256 was
`268cf30fbc9dbf9eadf85145cf29c5b6755d3f894bcdf20c92127baeff39c6de`.
The candidate build used `--target x86_64-unknown-linux-gnu` with
`--cfg marmish_peer_receiver_candidate`. Cargo used locked, offline
inputs. The two Clippy runs used `--all-targets -- -D warnings` and
include checks of the non-test libraries.

The opt-in harness uses disposable identities, private loopback WSS
and actual SQLCipher registration. It verifies zero TCP attempts
before open, startup interrupted or dropped while the relay withholds
the exact AUTH acknowledgment, and cleanup on malformed live EOSE.
Fatal wakes an already-pending read. Fresh page and stop attempts
refuse. Retained page, live record and control acknowledgment callbacks
cannot run after fatal, and `wait_live` cannot return data from a
populated reserve. Counts distinguish successful pre-cut publication
and a healthy history subscription close from unauthorized post-cut
activity. The private relay corroborates raw EOF or reset on the exact
recipient connection independently of the locally reaped permit.

The split cases keep profile cancellation live, consume the actual
AUTH challenge, trip fatal and poll the TLS-backed WebSocket read and
write sides separately. Each side refuses and keeps the actual slot
until dropped. A deterministic stream destructor checks that the raw
I/O is destroyed while the slot is still held.
