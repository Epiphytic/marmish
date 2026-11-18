# Authenticated Peer recipient transport review

Claude Code Auto used Sonnet 5.5 for all rounds.

| Scope | Source | Session | Verdict |
| --- | --- | --- | --- |
| Design R1 | `45c3270` | `e5518774-b018-49e5-a123-1d2d36a72392` | Requested changes |
| Design R2 | `b7a44d2` | `e839db5c-1eb0-4fcc-8334-aa36f7e49329` | Approved |
| Timing amendment | Reviewed as `742c6bc`, same design at `78a2c05` | `7584ef9c-d228-46fa-bcd5-c2ccea0b8e38` | Approved |
| Implementation R1 | `ccc8189543776aa78e86e71087fb815def627ddc` | `1354ee3b-8298-4320-a6c8-e1de62724d99` | Requested changes |
| Implementation R2 | `c64ffad458fdc6e7d956a73c5a47f88c1a2c0f35` | `5b11d5b4-bd10-4204-acb4-3b99b6f664ca` | Approved after stated test and lint conditions passed |

The design revisions define how a recipient operation supplies its exact
durable authority, when a staged write can become exposed, and how the
AUTH and subscription actions share the physical time ceiling. The
approved scope ends at the reusable checked port and its closed test
adapter. It has no installed startup or autonomous publication entry point.

The first code review found that the authenticated author comparison
existed only in the test helper. It also found gaps in direct coverage of
the final send fence and observable WSS handshake/route retirement cuts.
R2 puts the independent author check in the native send port before any
write poll. The test now drives the actual checked port with counted SDK
sink cuts and exercises configuration inode replacement on the correlated
AUTH acknowledgement and live EOSE. Separate private WSS tests cover
buffered data after CA replacement and idle retirement at actual leaf
expiry.

Claude Code Opus 5.5 in Auto mode approved the exact composed source
`6829448` against `0f80c27` in session
`c2e6d16d-4cd5-45a4-a860-99aeda1bc281`. The reviewer traced the transport
and storage boundaries on the composed tree and read its test logs.
The real fixture covers private CA mode. SPKI live open and misaddressed
arrivals from a relay that permits them remain independent test cases.

The reviewer confirmed the final fence order, only-tightening route
ceiling, inclusive certificate end, disabled early data and session
resumption, signed carrier checks, and the four-test focused suite.
The remaining conditional regressions, strict Clippy and optimized
`installed-checks` build check passed on the same source after the review.
The reviewer also noted the short leaf lifetime and hardcoded fixture
connection counts may make the ignored test more sensitive to slow hosts
and fixture evolution. Mid-flush expiry is covered by the deterministic
sink test. The ping reply is bounded by the route check and deadline.
The unshared connector construction assertion relies on holding the only
reference at that line. No remaining finding blocked approval.

See the [phase plan](../phases/peer-authenticated-route.md) and
[validation evidence](../validation/peer-authenticated-route.md).
