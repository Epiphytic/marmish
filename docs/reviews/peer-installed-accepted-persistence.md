# Installed Peer v2 admission design review

Claude Code Sonnet 5.5 reviewed the design in Auto mode. The reviews read
source and design documents without builds or relay traffic.

| Round | Exact reviewed design | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `ee2359bb4588b3ff75b4ee4c74b2b06b28abffc6` | `8981fb4b-618a-4034-96dd-0e9df7153ed9` | Request changes |
| R2 | `8362ab67965882fdd1da2742a016076e7b126835` | `917713d8-aee6-44a4-9f6f-67ff6cb9323f` | Request changes |
| R3 | `e335ff168efbe1cc5bf418afbf2ef1c248e9757f` | `0afb94d6-478c-4c25-b817-c70d8e125af2` | Approve |

R1 identified the missing composed v2 code base, fixture-only activation
boundaries, incomplete durable schema and unpinned receipt completion and
expiry ordering. R2 rebased on published v2 and limited implementation to
recipient admission and restart material. R2 required exact handling of the
v1-only current fixture lookup, Automatic admission, legacy attach refusal,
existing replay and capacity and exact cursor cleanup. R3 resolved those
findings.

R3 recorded implementation tightenings. Reject a v2 layout during legacy
attach before legacy DDL. Freshness includes all legacy replay fences and
the marker table's existence identifies the installed layout even when
empty. Compilation outside tests must retain the unconditional Automatic
guard. Explicitly guard legacy approve, approved work, discard and
publication reservations. Preserve route and version failure classification
so local configuration, time, signer and stored-integrity failures leave
pending cursor work retryable. Select the correct storage layout in every
sweep entry point.

The approval applies to the [recipient v2 admission design](../phases/peer-installed-accepted-persistence.md).
The approved umbrella design is an implementation-base dependency.
Founder and recipient acceptance under a live owner require another
review and real native/private-relay validation. This evidence grants no
installed activation or runtime validation.

## Completed code review

Claude Code Sonnet 5.5 in Auto mode reviewed the implementation and tests
against the approved recipient scope.

| Round | Exact source | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `bf15d995929b59e84ac7c69852bfe8f3299ec7a7` | `f625310b-f28a-4052-828d-6e2dbdc22061` | Request changes |
| R3 | `077d569757aa7d6ed57005c3fb2a8ccd3e3b8276` | `c7a97871-d93a-4b28-9755-9ebe117be41f` | Request changes |
| R4 | `7b4c4d3fcded2e6f4377e868d32c1bfbc67f9ef4` | `abd709f0-8d5d-483a-a19e-50af857a8ffc` | Approve |

R1 found that a remote signed Offer with a malformed or mismatched relay
list could prevent the cursor from advancing. The correction records those
inputs as Invalid and preserves errors for unknown local source, stored
integrity, index and clock. R3 verified the production correction and
found that the new regression misinterpreted the continuation returned by
a full page. The exact approved test checks the persisted continuation and
then completes the short follow-up page. An intervening review of the
initial regression is superseded by the R3 and R4 results.

R4 checked the version marker and write guards, exact cursor transaction,
layout exclusion and expiry. It ran the exact Linux AMD64 debug test
executable offline and independently observed 6 passing installed Offer
checks and 52 passing consent-checkpoint checks with 3 ignored. The author
ran the other checks in the [validation record](../validation/peer-installed-accepted-persistence.md).

The author confirmed coverage for a v2 Offer at the legacy opener, sweep
rollback and v1 Delivery substitution. The last case is in the existing
pure v2 wire test, separate from the new reopen test. R4 recorded no open
blocking finding. Nonblocking limits include opening the sweep guard even
when no row is due, and reliance on the existing cursor test for exact
cleanup with competing carriers sharing an ID.
