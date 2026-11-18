# Peer pending activation design review

Claude Code used Sonnet 5.5 in auto permission mode and reviewed the
[amendment](../phases/peer-pending-activation.md) against locally resolved
upstream MDK `0ad3c61088e1acac58e0fcf1a9f1142bae5d997a`. The design runs
used no relay or production credentials.

| Round | Claude session | Exact design SHA-256 | Verdict |
| --- | --- | --- | --- |
| 1 | `79eb8598-750a-4352-8ddd-7a0f3077e164` | `418f07bac0b017817f4511454ca2c559c4c35f7208ef81622f26a76582ba898e` | Changes requested |
| 2 | `85d21ab3-dfed-47b4-be9c-7e3ee6e3e3d1` | `9528c6b84f80855110832444abd51b4ab15bdd263ee1664050a52d64ee04ec5e` | Changes requested |
| 3 | `f5851bdc-7e02-423d-9509-f073e6c7eeed` | `49f831dade9bda99085ea5e67750e44b5920e60fdaf0bb5f11ca393dec67bf7f` | Approved with no blockers |

Round 1 required explicit defaults denying native egress while pending,
reconciliation of native PostJoin and KeyPackage obligations, convergence
after the decline fence with a permanent drop-only application sink, and
trusted-only occupancy and start capacity. Round 2 confirmed those
changes and found that the proposed `AccountDeviceSession` boundary was
only available with `test-support`. The approved design uses the public
account runtime, with bounded capture that cannot authorize network
publication and a production qualification gate for exact SelfRemove
identification and retry. Application acceptance remains independent of
native join and departure.

Round 3 confirmed the production boundary and deployment fence. Its
qualification advisories ask for fail-before-exposure error semantics,
an older denied same-room effect in the retry test, coverage of adapter
probes and specialized publication methods, and no mutator that rewrites
frozen native work. The reviewer verified that `PublishWork::Proposal`
takes the modern `OutboundFanout` path for a group envelope. This path
freezes the exact transport message and endpoint snapshot before the
adapter call. Design approval still leaves the actual scoped re-drive
and SelfRemove provenance to the production qualification.

## Native Leave and fanout retry capability

Claude Code Sonnet 5.5 ran in Auto mode against an immutable six-file
scope. R1 session `3e88919f-c3c8-470d-9e66-69004b2f6d65` verified the
probe's behavior and asked for explicit disclosure that exact local
SelfRemove authentication remained open. The source and validation
record now state that limit and link the separately scoped proof.

R2 session `318c17d7-4477-4bac-ada7-e8ae686d7db2` approved the
test-only increment with no mandatory findings. Scope manifest SHA-256
`902634f1fbda220fae106590303c73c220c96aef65b627dd93a374edfc765f5c`
covers the resolved lock, both test sources, pending design, follow-on
proof design and validation record. Claude re-ran the exact test from
the frozen scope and observed one pass in 13.45 seconds. All six file
hashes matched before and after. The existing default fixture opening
continues to use the same routing. The test remains an explicit
ignored-test target with no network access.

Remaining gates include authenticated exact local-leaf SelfRemove,
mixed retained work, recovery before a native fanout exists, actual
decline convergence, relay qualification and daemon integration.
