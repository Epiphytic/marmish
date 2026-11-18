# Installed v2 capped Pending bridge design review

Claude Code independently reviewed the test-only
[bridge plan](../phases/peer-installed-capped-pending-bridge.md) in
Auto permission mode against base
`e29270e57df1d88706849c608a7861a009f741b1`.

| Round | Exact design commit | Model and session | Disposition |
| --- | --- | --- | --- |
| R1 | `cf5e1d94a0ac25f05d851cb8e0e473473ec632ec` | Opus 5.5, `57b0dca1-2983-4dbb-aab5-2f3104295396` | Changes requested for token ownership, cold v2 negotiation and missing effect/cut evidence |
| R2 | `a3385d0128f005fb870fd7e9c13d8df51206a3ad` | Sonnet 5.5, `d280e3d0-3da5-4c95-8666-0e70b420ec8c` | Approved, all three blockers resolved |
| R3 | `a7a19f0a0969e31cd662a18b7c2a621b2e83809a` | Sonnet 5.5, `bdb102aa-a947-4820-bd8f-611700524958` | Approved incremental clarifications, no blockers |

The final design's SHA-256 is
`e655719f1842ebee76a37af454dee38dffad7167c33918cfd119573f18f757f4`.
The R3 result text with trailing newline has SHA-256
`5d11a8f4cab718de7ff5ea7830ddea02251b0a36df4343c4b7ab9dab171eb38b`.

R2 checked the exact lease lifetime and registration order, custody
CAS, signed v2 negotiation and native-proof code. R3 checked exact
receiver instance identity, file-turn locking, the fixed receiver
snapshot path, custody deadlines and the expected cut outcomes. Reviews
read source and design. No source edits, compilation or relay tests were
part of this design evidence.

Use a real borrow for prepared evidence bound to the live scope and
exact quota instance. Preserve charge and its fence when a Pending
deadline has passed. Record zero committed joins only when a pre-effect
refusal or independent evidence proves zero. An uncertain possible
effect remains explicitly unknown, charged and fenced. Count both
directions of relay authentication separately from Peer application
egress. These are R3 nonblocking implementation notes.

Exact managed receiver recovery and settled signed v2 Consent plus
admitted v2 Delivery still need to converge in one tested base. The
private proof can then be implemented and reviewed independently.

## Completed code

Claude Code Sonnet 5.5 independently reviewed the completed source in Auto
permission mode. Reviews inspected the exact bridge source and recorded author
results. The WSS resource slot was reserved for the author's isolated run.

| Round | Exact source commit | Session | Verdict |
| --- | --- | --- | --- |
| R1 | `37e82e7ad28f213d36bdcf124f44bae978ddb452` | `a4257c7a-0546-4cdc-a21f-5fc9027d2bb2` | Approved subject to exact live evidence. Raised after-close sampling and fresh retained evidence issues and identified untested refusal cases. |
| R2 | `4689d29c79be65cdf4b5b4b3d35fa26e05397c6b` | `e02c2725-485e-4715-a756-3d2a7f14e106` | Approved the bounded private proof. Verified exact WSS success, pre-effect exit and journal refusal. Requested the legacy regression, exact binary and Clippy commands, and explicit validation limits. |

R2 result text SHA-256 with one trailing newline is
`2f945c1755faa86f26fa80a2a85b2f50e4cb2f870c19636f158152fd41dc420a`.
R2 confirmed that close checks run before metrics, native ingest reloads the
saved Delivery and durable JoinStarted row, and widening the test harness
window leaves the native and signed deadlines unchanged. It identified
uninstrumented SQL statement counts, generic cold refusal assertions and
partial direct negative tests. The broader refusal matrix and separate
uncertain-effect injection remain open. The earlier 36.91-second author test
covered two scenarios. The exact R2 source passed all three scenarios in
57.50 seconds. Approval conditions and commands are recorded in the
[author validation](../validation/peer-installed-capped-pending-bridge.md).

## Composed accounting follow-up

Claude Code Sonnet 5.5 in Auto permission mode requested changes for the
composition at `d9ea132`, session
`e3c8b15e`. The review identified literal effect counts, missing time
assertions, incomplete replay row comparison, unlinked current rollback
evidence and a required refusal matrix.

The follow-up session `509581fc-f216-448f-b739-828a7e690da3` inspected
exact accounting code `7d725d01d59e8be5fe940fd2186bb2d83375a068`. It
confirmed the source changes, including preserved ordinary custody
transaction semantics, and requested two evidence corrections. The custom
counter is scoped to installed Start and Qualify. The saved Pending row
comparison independently substantiates replay. The earlier rollback
qualification preceded the helper split in `e942577`. The author and
reviewer both passed the rollback test from the exact accounting binary
after that split, in 5.72 and 7.03 seconds respectively. Full author WSS
results and the separate rollback evidence appear in
[composed validation](../validation/peer-installed-capped-pending-bridge.md).

The scoped follow-up result text SHA-256 with one trailing newline is
`99648a9c0259efbc11d05e47b52e8073cba90c6191ef5921f8f4ed3c579dad19`.
The separate refusal matrix remains outside this accounting follow-up.
