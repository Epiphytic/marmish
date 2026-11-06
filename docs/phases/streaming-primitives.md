# Phase 1: bounded stream primitives

Parent: [streaming plan](streaming-transfer-plan.md), Beads `marmish-8ka`.
This phase adds a reusable Rust crate; it enables no network execution yet.

## Interfaces and ownership

New `crates/marmish-stream` owns only pure stream wire/state/compression. Its
public API is exercised by tests and subsequently used by native integration.
Root owns workspace registration, integration checks and documentation. An agent
owns codec/schema; another owns bounded receive/send windows with disjoint files.
No changes to the finite engine or native command wire in this phase.

A closed serde envelope contains protocol version, 16-byte timestamp-bound stream
ID, 32-byte client/server identities, 16-byte session ID, exchange sequence and a
request/response body. Parsing bounds input before allocation, rejects unknown
fields and noncanonical encodings; transport authentication remains the native
adapter's job and is never inferred from payload identities.

Request variants: Hello (supported compression), Open (argv and selected codec),
Exchange (optional stdin block, final stdin offset, stdout/stderr consumed
cursors), Cancel and Status. Cancellation responses explicitly distinguish an accepted
request from a confirmed terminal cancellation; retries are idempotent. Responses: capabilities, Opened, Exchange (input
accepted/delivered cursors, independent output blocks/EOF, terminal outcome), and
finite typed errors. The provisional scheduling is one outstanding data Exchange per stream: client
sends stdin and output consumed ACKs; server replies immediately with currently
available data/status, then client sends the next exchange. Retries preserve the
logical sequence and request digest. A separate control request can cancel while
a data exchange is outstanding. No push or implicit pipelining is assumed.
Rough one-channel goodput is 8 KiB / end-to-end exchange RTT; compression may
increase raw goodput. Phase 2a must approve this provisional schema before live
execution; future pipelining requires an explicit design.
Open is immutable and duplicate/conflicting Open must be distinguishable.

Limits: 8 KiB raw blocks, 32 KiB envelope, 256 KiB per channel window, 128 argv
items and 16 KiB argv aggregate. Checked u64 offsets are capped at i64::MAX.
Each compressed block carries offset, codec, decoded length and SHA-256 of raw
bytes. Encoded block <= 8 KiB (compression falls back to none if not smaller). A
response has at most two 8 KiB blocks, whose base64 is <=21,848 bytes total;
remaining bounded metadata fits below 32 KiB. Open base64 argv <=~22 KiB.
The MLS app-event wrapper stays <=36 KiB, matching the current adapter limits.
Require a worst-case serialized-fit test for every variant. The stream argv
limits are deliberately stricter than the engine (256 items/64 KiB); reject NUL
and empty argv[0], matching engine rules. No secret material is printable in diagnostics.

## Compression

`none` and independent zlib blocks. Encoder uses zlib only if selected and smaller
than raw. Decoder bounds both input and output, permits exactly one complete
frame, rejects trailing bytes/frames, truncation, length mismatch and bad raw hash.
Use exact-pinned flate2 with default features disabled and explicit rust_backend;
update native CI test package list and release license roots. Reject preset
dictionaries (FDICT). Raw decoded size is checked before allocation. No untrusted dictionary or shared
compression state. Both directions use the negotiated codec; none remains valid
for incompressible individual blocks. Protocol errors never return partial bytes.

## Windows and acknowledgments

A SendWindow queues raw bytes at monotonic offsets, stops accepting at its byte
capacity, exposes bounded blocks and only reclaims a prefix after validated
cumulative ACK. Reject ACK beyond produced or behind already acknowledged.
A ReceiveWindow accepts exactly the next offset, recognizes a matching retained
retry, rejects overlaps/gaps/conflicts/overflow, and delivers queued bytes once.
Accepted means queued by receiver; delivered means actually written to the child
pipe; consumed means successfully written to the local output destination, and
only that cursor authorizes cumulative output ACK. Receipt entries are evicted
oldest-first only after their full bytes are consumed; at most 256 queued chunks
are admitted even if the byte window has room. Consumption and acceptance are distinct. A 256-entry recent receipt cache supports
idempotency; retries older than that horizon fail explicitly rather than being
silently accepted. EOF fixes one final offset, only at accepted end; matching EOF
is idempotent. Output final offsets must be consistent with acknowledged bytes.

These pure structures do not claim durability or atomic delivery to pipes. Phase
2 owns durable launch/transport fencing and explicitly uncertain recovery.

Status and Cancel return the Exchange snapshot. Terminal variants distinguish
normal exit, signal, confirmed cancellation and finite protocol/transport failure.
Open digest covers immutable argv and codec independently of exchange sequence.

Stream IDs use the first 8 bytes as big-endian Unix seconds and 8 random bytes.
At new Open only, an injectable-clock validator admits at most 7-day age and 60-second future skew;
native replay floor and durable launch fences remain additional requirements.

## Relationship to finite engine

| Primitive | Streaming use | Existing finite API |
| --- | --- | --- |
| ReceiveWindow | Memory queue to separate live child stdin | write_stdin/EOF remain unchanged |
| SendWindow | Reclaimable live child output window, pipe backpressure | read_output/prefix spool remain unchanged |
| Authorization/launch | Opaque engine preparation + existing native launcher | authorize/resolve_exec/check_execute_request reused |

Phase 2a adds an explicit live-child API reusing launcher and safe process-group
ownership, not an alternate finite worker/spool mode. No ENGINE_FORMAT or finite
worker migration is needed. Existing detached finite workers retain old semantics.
Memory-backed streaming queues do not survive daemon restart; restart marks them
Lost and never relaunches. Native durable launch/publication fences still apply.

## Acceptance

Roundtrip random/NUL/Unicode bytes in both codecs; compressible and incompressible
payloads; truncation, trailing frame, bomb, bad hash/length, oversized envelope;
closed schema and canonical roundtrip; bounds and integer overflow; bidirectional
window exhaustion, partial consumption and prefix reclaim; reorder/duplicate/
conflict/old retry; EOF retries and writes after EOF; invalid ACK; many windows
of transfer while resident buffered bytes remain bounded. Run crate tests, fmt
and strict clippy, independent Claude review, then source publication.
