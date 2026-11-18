# Offline Peer control codec reviews

Claude Code in automatic permission mode reviewed this increment with
Claude Sonnet 5.5. Scope includes the closed Peer flight schema, independent
clock and cover rules, exact nested artifact boundaries and offline tests.

## Design

| Review | Claude session | Disposition |
| --- | --- | --- |
| R1 | `d392270b-8226-4f47-8c39-e3313b452973` | Requested changes to strict event projections, the exact KeyPackage tag shape and slot, native versus injected time, and signature and recipient validation for the nested Welcome. |
| R2 | `8a5871c5-931a-4f76-9dd7-629a9e9ecb50` | Approved the offline design after those changes. The distinct logical room ID and native transport group ID are both signed in the Offer. |
| R3 | `0e579917-b344-4370-9153-7b454d757935` | Approved the narrow correction to require the last-resort marker emitted by pinned MDK's staged package API. The result remains bound to the exact Offer, with isolated storage, consumption evidence, replay fencing and withdrawal carried forward to integration. |

The first implementation run found that the earlier last-resort requirement
rejected the exact pinned MDK artifact. The R3 review confirmed that
`AccountDeviceRuntime::prepare_fresh_key_package` reaches
`build_key_package(storage, true)`. The test now asserts the marker on a
freshly generated native artifact. It also checks that Consent issued for one
Offer fails under a different Offer. The codec does not independently test
admission of an otherwise valid single-use sibling package.

## Completed code

Claude session `5a5f691f-414c-4065-87a9-21baf43f818c` approved the exact offline
implementation and found no materially actionable issues. The reviewer checked
all four new `peer_control_wire*.rs` source files and the integration entry,
against the approved design. It independently ran the offline core target and
observed eight passes. It also confirmed the use of an engine-generated native
KeyPackage for the offline fixture. The parent plan retains the staged runtime
artifact and relay qualification gates.

The frozen review source was based on `90ecba97b3ad970a90bb0f94f64a981620ffe847`.
The [validation report](../validation/peer-control-codec.md) records local
strict Clippy results and release benchmarks. Those two checks were run by the
implementer.

The complementary live-lane phase design was approved in round seven by
Claude Sonnet 5.5 (session `d6775d31-9d40-4a76-9d17-f572a632bd9c`). This
approval covers bounded raw WSS ownership and deferred page recovery.
The two nonblocking notes were incorporated, clarifying socket ownership
and the qualified relay's 64 KiB event admission cap. The codec review
above evaluates the offline implementation.
