# Installed recipient Offer admission validation

Linux AMD64 tested source `42391b4bfd77482b825d377ed8851659e35596f2`
with the locked upstream MDK revision
`bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`. The four focused tests
passed in 5.24 s.

The loopback WSS fixture used a disposable recipient and founders, the native
TLS path with the fixture CA, and positive NIP-42 authentication. It upgraded
an empty capped SQLCipher cursor, received one authenticated Offer from
history and another live, and retained both as unreserved Unknown. The test
compared each authenticated inbound event with its authored signed event and
checked the exact bytes delivered by the port against durable bytes after
closing and reopening SQLCipher. A different socket could not present the
page, and route retirement prevented use of the observed live carrier.

Deterministic cases checked source, layout, account, origin, Manual policy,
clock floors, quarantine, and signed Offer policy. Wrong-recipient and
self-addressed outer envelopes were refused at indexing. The fixture also
checked exact replay, cooldown, same-founder backpressure, and rollback under
a forced SQL page cap. Legacy automatic mode and reserved pins left the
Manual Offer unreserved. No test path granted provisioning, native group
access, or cursor advancement from an unclassified record.

Early development runs caught an expired Offer still within the wire codec's
clock tolerance and a signed-event serializer emitting a different key order
from the socket. Admission now checks the actual expiry, and the cold test
compares saved bytes with the observed socket bytes. The first final review
caught the use of an unavailable signing method in the envelope test.

| Check | Result |
| --- | --- |
| `peer_installed_custody_offer` | 4 passed in 5.24 s |
| `peer_authenticated_route` | 4 passed in 21.00 s |
| `peer_installed_offer` | 6 passed in 1.73 s |
| `peer_installed_cursor` | 4 passed in 10.98 s |
| Final `installed-checks` `peer_installed_custody_offer` | 4 passed in 6.64 s |
| Standalone `peer_cursor_store` target | 72 passed, 4 ignored in 55.18 s |
| Strict all-target native Clippy with `installed-checks` | Passed |
| Rustfmt on changed paths and Git whitespace checks | Passed |

The Ripwire report flagged clone-shaped fixture helpers, churn in the shared
custody module, and length in the Offer admission and fault tests. The fixture
helpers assemble an empty installed successor and keep the negative checks on
the same capped database. The review checked bounded admission, rollback and
the exact portable layout in static inspection.

This is a test-only Linux AMD64 proof of Offer admission. Daemon mounting,
combined mixed-page cursor progress, Prepared Consent send, Delivery and native
Welcome processing remain separate.
