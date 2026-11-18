# Peer heartbeat publication provenance over private WSS

The [heartbeat activation design](peer-heartbeat-activation.md) requires proof
that native MDK records exact application provenance before it attempts to
publish. The acceptance receipt proof already performs a cold capture for
one application kind using a closed publisher. This increment uses the
accepted founder from the same private WSS enrollment to test the heartbeat
path. The accepted receipt continues to use its existing verifier. All new
network and authority constructors in this proof are test-only.

## Sender boundary

Use disposable founder and recipient accounts and the same authenticated
private TLS test relay. The founder has accepted the exact receipt from the
previously retained founder enrollment. The recipient must have reached the
matching stable active native group and exact roster. Assert both readbacks
independently after reopening. The stored founder acceptance provides the
scope for the test. A dedicated in-memory fixture authorizes an initial
heartbeat under one current execution grant and one selection, with durable
status reservation in the existing isolated send-fence store. The fixture
can turn off the grant, close the revision or replace the scope. These
fixture facts cannot construct installed Peer authority.

Encode the reviewed status codec into a canonical Marmot app event with
fixture kind 30104 and no optional tags. This is the only kind accepted by
the fixture. Before promoting it, review a common installed dispatcher
reservation. Use the qualified sampled time for created_at. Store the
sequence using the existing durable heartbeat reservation. Keep the
Prepared record in the fixture's memory. It binds the committed sequence,
app ID, encoded digest, frozen room, revision, native pin and expiry before
native send. This proof cannot establish durable Prepared recovery or
restart safety. Native send uses the exact stable pin as expected_epoch.

Extend the checked `PeerFounderReopen` fixture so its validated account
open constructs an adapter with a clone of the same
`SqliteAccountStorage` handle supplied to `SessionConfig::with_storage`.
The clone borrows the shared connection through supported public traits.
Only the runtime owns native writes. No second writable handle opens the
database. At the first typed publisher call, capture the exact request
message ID and use `outbound_fanout(&request.message.id)` for one exact
lookup. Obtain its associated native application message, then match the
signed event, target, room, audience, purpose, frozen app ID and digest,
retention and the prior Prepared binding. Use separate direct reads
without `with_read_snapshot` or a clone of the runtime. An earlier stable
native authority read pins source epoch and leaf outside the callback. The
callback returns a closed result regardless of the observation. It must
report one of observed, absent, storage failure or binding mismatch. The
reservation remains spent on every failure category.

Run the callback test in an owned subprocess so the parent can detect a
lock stall in the underlying synchronous SQLite read. Distinguish a
watchdog death at the in-callback stage from a missing record, storage
failure and mismatched scope. Use a fixed child trace with the stage emitted
and flushed before the first read. Give the whole positive test at most
180 seconds and the first callback read 5 seconds after that marker, with
500 ms reserved to terminate and reap the process. The proof uses an
isolated private directory and removes only owned test files. No
post-return capture substitutes for the in-callback observation.

A fixture may stage a second read after the closed initial callback for a
separate one-shot exact test send. That path uses the current, rechecked
authority and the original native signed bytes. Reuse the qualified scoped
Group socket and authenticate the live target. It records one physical
status write and a matching relay ACK. For each send record exactly one
initial `send` call, one matching native target attempt after the closed
callback, and the same signed event ID and digest in the resumed request.
No fresh message is generated from the same reservation. Close before the
first write under changed revision,
lost grant, changed pin/roster, deadline, wrong Group endpoint or mismatched
native/app event, and require the relay count to stay unchanged. Run these
negative turns through the same armed adapter with the saved native
request. MDK spends a native attempt before entering its callback, so use
its real resume only for the positive send. A negative adapter turn must
leave the saved native attempt count unchanged. An unexpected maintainer
callback cannot publish under this proof.

## Receiver and timing

Ingest through the recipient's exact accepted native Group lane. Require
the exact waiting account, route, author, epoch, retained active leaf and
canonical status payload from native application authority. Borrow the
existing receiver projection logic under a test-only accepted binding. A
matching already seen heartbeat stays idempotent. Replaying the same
signed bytes cannot advance peer observation, grant execution, or create
another room. A private relay must see a kind-445 Group event and no public
status metadata. The fixture records count, authenticated encoded bytes,
wire bytes and elapsed milliseconds from reservation to authenticated
projection. Do not emit metrics content or peer IDs.

Exercise stable accepted-state negatives before release, including status
before acceptance, current grant loss and revision changes. Each checks
zero WSS writes. A separate bounded private WSS test proves refusal of an unregistered
account, wrong transport group tag, extra filters and other kinds. Use a
fresh disposable third signer for the refused account.
Reject wrong author, epoch, leaf, application room, kind and marker,
nonempty tags, noncanonical bytes and expired or future creation time in
the receiver. Confirm receipt and heartbeat kinds cannot cross lanes.
Cover replay of the original signed bytes and exactly one projection. Use
the real native provider for the positive and replay; use bounded
direct validator inputs for malformed records where native authentication
already denies construction. Distinguish that coverage in the report.

The first proof sends one typical heartbeat and, in a separate operation,
one maximal-field heartbeat with eight disks, eight IPs and bounded
strings under the 4 KiB heartbeat cap. Each case allows one physical
status write. Use deterministic values, check the exact decoded fields and
measure the largest application envelope that completed. The 8 KiB
permission pages and their different issue reservations, staging and
projection form a separate increment under the original activation plan.
Run each application/relay step with a 5-second local bound, except
where the underlying integration requires the existing longer enrollment
deadline. Use one in-flight application event, a one-second send floor
and at most two positive records and sixteen negative cases in this
test. Keep the opt-in whole fixture out of ordinary CI.

Run the benchmark in a separate owned process with twelve fresh heartbeat
reservations over one new accepted native group. Use the normal MDK retry,
a one-second send floor and one in-flight operation. Give each full sample
50 seconds and the process ten minutes, including prior enrollment. MDK
122ad45's `FROZEN_FANOUT_AMBIGUOUS_RETRY_BASE_MS` in
`crates/marmot-session/src/runtime.rs` is 30 seconds for the initial closed
publish in this path. Fifty reservations would need over 1,500 seconds for
retry alone. The unchanged native delay dominates reservation-to-ACK and
reservation-to-projection timings. Those timings bound the closed-capture
path. Reservation-to-callback includes native setup and submit work. Report
in-callback lookup and validation time separately.

Report elapsed timings, writes, projections, total encoded and signed wire
bytes, and largest application size without reporting host or identity
contents. Use nearest-rank monotonic p50/p95 for each stage. With twelve
samples p95 is the maximum. Wall time only supplies created_at and freshness
checks. The already measured 200-sample [local planner and codec path](../validation/peer-heartbeat-planner.md) has separate results and exercises
none of the native retry or WSS path.

All new authority and transport proof code is under `#[cfg(test)]`. A
non-test compiler probe confirms that fixture constructors and adapters
are unavailable to production. This increment does not enable installed
send/receive, promote fixture authority, register a VFS or intercept
ordinary traffic. The installed phase needs durable publication and
restart, authorization, native platform and external qualification.
