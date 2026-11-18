# Peer Offer receipt and Consent publication reviews

Claude Code used Sonnet 5.5 in auto permission mode. The detailed
[phase design](../phases/peer-invitation-integration.md) was frozen over
base `b6bbb684d35aba0f5f431431781e40559a6064e5`. The design reviewers
inspected source and the pinned MDK revision `8abc51b` locally. They used
no relay or production credentials.

| Round | Session | Verdict |
| --- | --- | --- |
| 1 | `d049c919-62e2-456c-a863-dad1c07534bf` | Changes requested |
| 2 | `acb39a57-8706-47f4-920f-f5be68edc323` | Changes requested |
| 3 | `aa9e87e2-83b8-46b3-8117-3814c4fd0124` | Approved with no blockers |

Round 1 required a sealed relay qualification boundary, exact durable
attempt and acknowledgement witnesses, actual profile lease ownership,
a single qualified wall, retry fairness, an OS-credentialed local action,
and an exact withheld native package recovery contract. Round 2 found
that the pinned MDK fresh preparation API can rotate a prior package.
The approved design uses its public signing adapter and maintenance
storage directly to recover an unsigned pending bundle. Fresh MDK
generation only runs on an approved, empty, bound stage.

Claude verified the approved design SHA-256
`42e01a53ffd770122b9a3f5e902011310abe53f34df6490bef66855fde4d38c2`.

## Completed code review

Claude independently verified the sealed origin and NIP-42 socket,
local peer credentials, native stage and private KeyPackage custody,
immutable outbox retry, and current-socket acknowledgement. The reviewer
ran the scoped core, opt-in WSS and independent cursor suites and strict
all-target native Clippy. It used only disposable loopback fixtures.

| Round | Session | Scope manifest SHA-256 | Verdict |
| --- | --- | --- | --- |
| 1 | `f318f37f-60df-4092-9270-2e2b1d8787fe` | `f887d7d9783c8a4ea9d9700e476401f2589ebcc7fc97ddc891a0f430bddb3286` | Approved with no blockers |
| 2 | `08c4a1ef-237d-48bd-890a-3abdd8e8175d` | `b202947a3554c35e6c294184a5ef1188a288fad558e7c06d1f542a3a7ced5525` | Approved with no blockers |

Round 1 identified a safe retry delay when an already retired generation
or elapsed deadline was known before reservation. The follow-up checks
socket readiness before opening native custody and once more before
committing a send attempt. The recovery test verifies that an immediately
usable or replacement socket can still publish at the original due time.
The fixture now uses the same signed carrier check across both suites.

Round 2 verified all 18 frozen scope hashes before and after review and
confirmed only the four retry and fixture files differed from round 1.
Claude ran 49 invitation and raw socket tests including the opt-in WSS
fixtures and strict all-target native Clippy. The exact review used the
original frozen dependency lock. The validation report records a separate
smoke run with the current upstream MDK dependency.

The reviewer identified follow-on checks before enabling a daemon owner
or public traffic. These include policy changes after queueing, explicit
native KeyPackage egress denial testing, key-copy lifetimes, interrupted
initialization recovery and durable quarantine of additional history
violations. They do not authorize deployment or affect the scoped tests.
