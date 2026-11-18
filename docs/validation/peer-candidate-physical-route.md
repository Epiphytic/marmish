# Candidate physical recipient route validation

Linux AMD64 source `9344290bed426356df27780627acdb5c2e2c2c96` combines
the sealed registered route handoff and independent private WSS fixtures.

| Check | Result |
| --- | --- |
| `peer_registered_route_private_wss_handoff --include-ignored` | Passed all six fresh subprocess cases in 28.37 s |
| `registration_scope_and_irreversible_dispatch` | Passed all seven fresh subprocess cases in 29.42 s, including actual default VFS displacement before and after the one-use route take |
| `peer_relay_trust::physical_route::tests --include-ignored` | 2 passed in 9.75 s, including cold WSS trust and clock fault regressions |
| Strict native all-target Clippy | Passed in 27.36 s stock and 26.99 s candidate |
| Formatting and diff checks | Passed |

The six WSS cases used disposable identities, real TLS and NIP-42, and
four qualification connections per child. Successful handoff observed
one live request and one bounded empty history page. A repeated take
left the original port usable. Replacing the config inode or dropping
the attached register owner revoked the live port. A foreign profile
lease, a different material object, a dropped unused admission, and
default displacement refused the handoff without recipient requests.
Each child observed zero control publication attempts. Ordinary
SQLite still created and reopened its data wherever the process
forwarding verifier stayed current.

The first global cut left the route in place and the token uncancelled
before owner drop. The second cut returned Indeterminate after the
route moved and verified that the route destructor cancelled its token.
The WSS material case tests pointer identity. The separate registration
case with a different SQLCipher root exercises the qualification issuer
check before the attached material borrow exists.

The exact candidate libtest executable compiled and linked in 40.36 s.
SHA-256 was
`aa80542f2b9644aaec2115f1e41659d7feaf1216fd1f323b59365a6ad21e5980`.
An isolated compile probe against source `5fd8638` received `E0451` when
it tried to construct the token outside the owning module. Another
candidate probe received `E0599` when it tried to use the test-only
fixture constructor. Neither probe is in the published source.

This increment qualifies the private handoff and typed port. The
installed daemon does not consume either capability.
