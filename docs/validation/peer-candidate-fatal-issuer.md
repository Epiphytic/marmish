# Registered dispatcher signal and raw WSS validation

Linux AMD64. Exact integrated source
`16dc1b3e1bc9fa0cb044e232a22f26f3388c0819`. The candidate test
executable SHA-256 was
`a819d50f581a3e96cb3e14aeeb7619025274398d30b1c0ab8f6630f87ea70549`.

| Check | Result |
| --- | --- |
| Always-on dispatcher unit cases | 3 passed |
| Isolated actual SQLCipher registration and forwarding | 13 fresh processes passed in 47.40 s |
| Registered route handoff over private WSS | 6 fresh processes passed in 25.26 s |
| Raw socket cut and drain over private WSS | 14 fresh processes passed in 59.73 s |
| Stock handshake regressions | 7 passed |
| Native all-target strict Clippy | Stock 29.12 s and candidate 28.41 s |
| Native libtest link | Stock 44.92 s and candidate 43.60 s |

Displacement found by an exact registered observer synchronously
cuts the shared signal and wakes a pending raw read. The relay
sees physical disconnect and receives neither WebSocket Close
nor Nostr CLOSE. Cut cases also cover NIP-42 startup, both surviving
TLS split halves, and checked application use of previously
buffered authority. Healthy exact local scope refusal leaves the
ordinary verifier and SQLite forwarding live.

Registry tests force unwind, poison, and repeated notification
after releasing the mutex. One case delays the first notifier
and a second observation wakes the waiter. A custom waker checks
that the flag is set and the registry mutex is available when
the notification arrives. Restoring the prior SQLite default
keeps Indeterminate sticky.

Two separately compiled non-test candidate probes refused the
additional route latch argument (E0061), independent Trip
issuance (E0599), and direct mint construction (E0451). The
Command consumption probe belongs to the adjacent SQLCipher
composition. These tests run the private owning components.
The installed actor and actual daemon-wide physical stop have
their own qualification.
