# Peer provisional reservation review

Claude Code independently reviewed the
[storage design](../phases/peer-provisional-reservation.md) and frozen source
with Sonnet 5.5 in Auto permission mode. The reviews used an offline checkout
and disposable fixtures. No relay or application room was opened.

| Review | Claude session | Exact SHA-256 | Verdict |
| --- | --- | --- | --- |
| Design 1 | `591d7a72-6330-4b2a-bddc-a1602d55d119` | `593e7ac80438ee6886ddcec419ed3ead4592e95fbe16591932a194ae94feff84` | Changes required |
| Design 2 | `af4ad7ee-c778-4214-acd4-654cd912efb1` | `c064d96da23a141afc455614b54f11f7072534ffdc9d6118f0d7514f106de5bb` | Changes required |
| Design 3 | `d6f650fd-b46b-4ca9-9f17-e396d0aade51` | `988ac718e0614ecf53df7892cf66d2ca4472d466b52b97c8da12c281205a6a4a` | Approved |
| Code 1 scope manifest | `0b73229e-00a5-4c9c-af1a-bb1f06c312b3` | `6b8b95612be62edd28bc33bb397603c86ab44f3e054c2ef6b1a663d91fc85205` | Approved |
| Code 2 scope manifest | `c9a9128b-f4e2-4e8b-b323-5878d1783bde` | `db62928db919dcffb96d4f007d10e48a8dbd6ec6846cf80f0c2f556ca962d5b5` | Approved |

Design revisions introduced an exact local retirement capability, SQL ownership
constraints and a durable expiry observation before signed decoder errors. The
final design review found no blocking findings. Implementation began after its
approval.

The first code review independently ran the 29-test cursor suite and
verified all 12 scoped file hashes. Its useful advisories were incorporated.
The final source shares the guarded legacy invitation load, rejects identical
author and recipient when reopening signed Offers, and tests both integer
limits, simultaneous sweep of an unowned expired Offer and readback of
unchanged ciphertext from the earlier schema. The shared signature check
also enforces recipient inequality during legacy Consent preparation.

The final reviewer verified all 12 hashes before and after the offline
tests and found no blocking findings. The focused test run passed six
checks with one optional benchmark ignored. The complete cursor suite
passed 31 checks with four explicit ignores, and the reviewer ran the
reservation benchmark separately. Their p50 and p95 were 87.4 and 105.7 ms
for reserve and 85.9 and 101.1 ms for recover. The
[author validation](../validation/peer-provisional-reservation.md) records
a separate sample and a scoped Clippy pass. These timings reflect local
host conditions.

The test suite checks local retirement in default Manual mode as well
as with Off, block and quarantine. One direct-SQL test deliberately omits
the origin from a held selector. Schema text is intentionally exact and
needs an explicit migration when changed. Keep native stage gating,
bounded terminal release and closure of the unreserved legacy stage path
as deployment prerequisites.
