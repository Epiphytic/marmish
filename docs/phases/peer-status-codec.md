# Private Peer heartbeat and permission codec

This phase implements the wire codec and deterministic receive decisions from
[Private peer status and access history](peer-heartbeats.md). An isolated
native integration-test entry qualifies the Rust module before daemon
activation. No configured daemon gains new subscriptions, fields, or
publication paths.

## Wire boundary

The caller has verified the active two-member Peer authority, application
author, native sender leaf, exact room identifier, current local room
generation, and server-to-client direction. The receiver takes the exact
active local binding and authenticated application creation time alongside the
encoded record. It independently compares the active binding with its own
pinned binding and the decoded room with that binding. Replacing the room or
local generation requires deliberate construction of a new receiver. No wire
field chooses the local generation, remote role, or principal. The codec makes
no admission decision. Scheduled heartbeats and permission pages remain
separate from the existing invitation-acceptance record.

Encode version 1 JSON as an object with `version`, a 32-byte room identifier
in lower hexadecimal, and a tagged payload. Containers reject unknown keys,
duplicate keys, extra documents and version/room mismatch. Check the raw 8 KiB
ceiling before decoding the shared envelope, then recheck heartbeats against
their 4 KiB ceiling once dispatched by tag. This is the app-record JSON byte
cap; MLS and relay framing have their own budgets. The version, page and page
total use u8. Interval, load and too-large length use u32. Wall times,
sequence, permission ordinal, uptime and disk amounts use u64. Binding
contains the verified local client and pinned server, exact room and nonzero
local generation. Require nonzero sequence and permission ordinal. Stable
authenticated application creation seconds equal the declared sample time or
permission issue time. Future skew beyond 120 seconds is refused. Permission
refresh requests, transport authorization, carrier retention and journal
writes belong to future integration.

Heartbeat body fields are `sequence`, `sampled_at`, `interval`, `host`, and an
optional `permission` generation object with `ordinal` and `digest`. Missing
permission means there is no advertised description. Within `host`, fields are
optional and ordered as `username`, `uptime`, `load`, `disks`, `ips`. Disk
fields are `label`, `used`, `available`. All three load values appear together
in one fixed-length array. Maximum size is bounded above by 2,048 bytes even
when strings need escaping, asserted by a maximal canonical heartbeat test.
The sender and receiver use the same field and size validators. A heartbeat
carries optional username, host uptime in seconds, 1/5/15-minute load in
thousandths, labeled disk space, and selected local IPs. Use 1–64 UTF-8 bytes
for the username and 1–32 for each label. Exclude Unicode controls, Arabic
Letter Mark, LRM and RLM, line and paragraph separators, bidi
embeddings/overrides U+202A–U+202E and isolates U+2066–U+2069 from username
and labels. Disk labels carry short names without a slash. Missing or null
optional fields have equal meaning. The label list is capped at eight with
unique, ascending labels. Each disk carries bytes used and available to the
sampler, a checked nonzero sum, and a utilization ratio derived by the reader.
An included disk list is nonempty. Use one through eight unique IPs when
present in ascending Rust `IpAddr` order encoded in canonical textual form.
Accept every IP address class including loopback, unspecified and multicast
without a scope identifier. The sampler strips local scope IDs, deduplicates
by address, and sorts. Require the real sender to canonicalize configured
fields, and retain the existing Tor/address sharing gate before collecting or
constructing any record. Load caps match the approved host collector at
1,000,000,000 thousandths. Uptime is bounded to 31,557,600,000 seconds (1,000
Julian years) and never used in freshness. An absent field denotes either a
configured omission or unavailable sample.

The permission description uses the layers from the engine's existing
`CommandPolicyDescriptionV1` and its exported `Restrictions`. Preserve layer
and pattern order, absent versus empty allow lists, and the deny union
semantics. Include capabilities with three values (unsupported, denied and
granted) for interactive and shell-command use. Mirror the engine limits of
129 effective layers, 256 patterns in aggregate and 1,024 UTF-8 bytes per
pattern. Each decoded sequence has a fixed limit before application shape
validation. Accept every absolute control-free path and valid literal-star or
literal-backslash escape the engine accepts. Preserve engine-legal Unicode
marks in patterns, which future operator views must escape when displaying. A
layer with an absent allow list and no denies is ineffective and is refused
because the engine does not emit it. The description is advisory and never
goes into `check_command`.

Canonical V1 JSON orders fields as version, layers, interactive,
shell_command. A layer contains allow_exec followed by deny_exec. SHA-256
covers the exact bytes of `b"marmish/peer/permission-description/v1\0"`
followed by the canonical description bytes. Reconstructed descriptions must
byte-match a fresh serialization before exposure. The envelope fields are
`version`, `room`, and `payload`. The two payload variants contain `kind` and
`body`. The exact kind strings are `heartbeat` and `permission`. Permission
body fields are `issued_at`, `generation`, and `content`. Generation fields
are `ordinal` and `digest`. Content is an internally tagged `kind` with either
`chunk` and `page`, `total`, `encoded`, or `too_large` and `description_len`.
Split canonical bytes into fixed 5,760-byte chunks. Encode each canonically as
standard padded base64. Every non-final chunk is 5,760 decoded bytes, and the
final chunk holds 1–5,760 bytes. A maximal page envelope is 8,024 bytes. Bind
each page to the same room, issue time, nonzero generation ordinal and digest,
and require consistent totals (1–64). If a valid description exceeds 368,640
bytes, advertise too-large for the same generation and digest using a
canonical length greater than 368,640 and at most `256 * (2 * 1024 + 3) + 129
* 64 + 128`. The latter allows for every pattern byte doubled as JSON escapes
and all fixed JSON layout. A pending generation may reserve at most 368,640
bytes. The receiver copies into a single bounded contiguous buffer and tracks
receipt using a bitmap. Peak verification memory includes that buffer, a
bounded parsed description, and one canonical reserialization. Decoder
sequences cap their elements before validation or retention, and only one
bounded temporary decode can coexist with the one pending transfer for a
synchronous receive. The profile owner enforces the eight simultaneous
reservations and 4 MiB staging budget in the parent design using the
receiver's reservation status and its explicit allocation permission argument.

## Deterministic receive decisions

Keep a per-room heartbeat high watermark and the latest sample that arrived
fresh. Inputs include caller-supplied wall seconds and monotonic `Duration`.
Accept interval 300 through 86,400 seconds, with jitter `min(floor(interval /
20), 300)`. Reject signed age at or beyond `2 * interval + jitter + 120`.
Compare timestamps with checked unsigned arithmetic. A signed time within the
allowed future skew has age zero. An otherwise valid sample already older than
interval plus jitter plus skew advances the heartbeat watermark only. It does
not replace sample content or change permission state. Equal and lower
sequences are ignored, including conflicting payloads at an equal sequence.
When displayed, both signed age and monotonic receipt age must be strictly
below interval plus jitter plus skew. Time is unknown when the query's wall or
monotonic clock is earlier than the displayed sample's first receipt. A
watermark-only event keeps that displayed sample's own receipt. Retain the
last fresh arrival and its signed sample time, first receipt and current age
even after freshness ends. The display states are:

| State | Display |
| --- | --- |
| Fresh | Last authenticated sample values, signed age, and first receipt. |
| Stale | Last authenticated sample values and timing through retention. A stale-only watermark has no values to display. |
| Pruned | Stale with the replay watermark only. |
| Unknown | An unseen peer, or a seen peer without a comparable clock. Suppress age and values for this view. |

Seven-day retention drops values if either reliably comparable clock has
advanced that far since first receipt; watermarks remain.

A freshly arrived heartbeat or permission page at a higher ordinal fences the
current ordinal and frees its pending transfer. An older permission
advertisement does not affect the fresh host sample. A different digest at the
current ordinal poisons it, drops its pending transfer and evicts its
completed description. Older verified content may be retained internally, but
permission display is unavailable until a higher generation is authenticated.
A TooLarge control for the active ordinal displays the signed length without
rules and retains only any earlier verified content internally. A valid
description conflicts with TooLarge for its generation. A stage allocation
denial can still fence obsolete pages and leave the latest prior complete
snapshot cached. Any refused input leaves state unchanged, including malformed
framing, binding mismatch, timestamp mismatch, excessive skew and signed
expiry. Accepted stale-on-arrival heartbeats are the sole watermark-only path.
Authenticated in-generation equivocation and an invalid fully reassembled
description are explicit poison decisions with state changes.

Pending accepts pages in any order and commits only after canonical parse and
exact digest. Both original and subsequent pages are checked against signed
issue age strictly under 900 seconds. The first accepted page fixes a
900-second monotonic reservation. Duplicates and later pages never extend it.
Conflicting chunks or metadata within one issue and an invalid completed
digest or canonical description poison the generation. Keep the greatest
completed or expired issue time for the current ordinal even after pruning. A
new ordinal starts its own issue-time fence regardless of the preceding
ordinal's timestamp. Replays at or below it may neither allocate a new stage
nor renew display. A strictly newer issue can restage the same generation
after expiry, or reissue it while complete using separate staging. A competing
issue cannot replace an unexpired transfer. If the monotonic clock is earlier
than a stage's first receipt, keep its reservation until the clock is
comparable or a higher authenticated generation fences it. Expose reserved
bytes and explicit sweep to the owner.

An exact fresh heartbeat reaffirms an already completed policy at its own
signed sample and receipt times, without initiating or extending a transfer.
It cannot recover incomplete content. A later same-generation reissue
reaffirms when every original canonical page has been independently received
and verified again. The original issue time may never bump freshness.
Permission-only freshness uses the default one-hour interval plus skew and
jitter. A matching heartbeat uses its own announced interval. When multiple
confirmation sources exist, use any fresh confirmation. Cached permission
display is allowed when current confirmation time is uncertain. A pending
newer generation can display older complete rules as cached. Poison and
TooLarge take precedence over cached content. A valid current-ordinal control
which contradicts the signed representation can poison even when its issue
time is already fenced, including a replayed chunk for an established TooLarge
generation. TooLarge goes through the same strict 900-second signed issue age
check and per-ordinal issue-time fence. A strictly later issue may reconfirm
the same length and set a new receipt, while retries of the same issue cannot
renew it. Display the length through seven-day retention based on first
receipt of the latest such valid issue. Display is unavailable while that
receipt is incomparable. A heartbeat cannot extend retention. Once pruned the
active ordinal still suppresses earlier rules behind an unavailable view, and
retains the conflict classification and fence. Seven-day retention removes
verified policy values based on their actual completed transfer receipt, while
an independently verified reissue is a new completion. Fresh heartbeat
confirmation alone does not extend value retention.

The receiver is memory-only in this increment. A later owner must commit its
high watermarks, current ordinal, conflicts, and projection durably before
acknowledging the native event, maintain profile-level admission and
conservative restart clock handling, and clear derived views when
authenticated authority is retired. This phase adds no cross-restart claim and
confers no authority to use the data for command execution.

## Validation

Add filtered deterministic coverage for exact round trips and a fixed digest,
real engine descriptions with triple-star, an escaped backslash, a bidi mark
and empty/absent allow, strict record shapes, overflow and extreme integers,
authenticated time and binding mismatches, and bounds for host fields and
maximum heartbeat. Prove the heartbeat interval edges 299/300 and
86,400/86,401, floor jitter, the future-skew 120/121 boundary, and the exact
signed freshness and acceptance limits. Validate heartbeat duplicate, reorder
and stale-watermark-only behavior, timing rollback, and seven-day retention.
Exercise unseen peers, null/missing optional fields, empty usernames/labels,
IP loopback/unspecified/multicast, duplicate addresses, and both raw size
gates. Reject ineffective layers. Validate out-of-order or incomplete
permissions, duplicate/conflicting chunks, digest mismatch, exact 900-second
expiry, higher ordinals, no stage allocation, poison and TooLarge visibility,
fresh matching heartbeat and complete reissue reaffirmation, and retention
from actual transfer/control receipt. Prove a higher ordinal starts with a
clean issue-time fence even after a wall clock step. TooLarge accepts a
strictly later confirmation but neither its original replay nor a heartbeat
extends retention. Its fence survives pruning. Test maximal 8,024-byte page
framing, canonical base64, 368,640/368,641 length and the explicit 533,440
TooLarge ceiling. Permission values may become visible only after the complete
canonical digest-checked description.

Keep the local benchmark ignored and use synthetic data. Report serialized
bytes, sample count, p50/p95/p99 and operations per second separately for
heartbeat decode and receive, representative ordered and shuffled permission
staging, and a near-capacity description. No fixture uses live relay or
personal host data. GitHub's small deterministic selector is an integration
decision.
