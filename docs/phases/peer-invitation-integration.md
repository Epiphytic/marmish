# Native Peer Offer receipt and Consent publication

This increment connects the [raw WSS owner](peer-raw-socket.md) and
[durable consent checkpoint](peer-consent-checkpoint.md). A private
fixture sends a signed Offer over WSS to the production client owner.
The client persists the exact pending or approved revision, uses a
separate native MDK stage to prepare a private KeyPackage, commits Consent,
then sends the exact committed envelope on the same configured origin.
Only current-socket, exact-event positive acknowledgement is recorded.
The fixture server opens that Consent with the existing codec. Downstream
Delivery, Welcome application, room activation, server Offer scheduling
and public daemon/CLI integration are separate increments. This produces
an internal runtime seam before any config can enable traffic.

## Entry points and ownership

Mount one private production Peer invitation module. Move the reviewed
cursor/consent and raw socket primitives into that scope with minimal
visibility changes, leaving existing fixture mounts in test builds.
Expose only the operations needed by a future daemon owner. Each owner is
serialized through exclusive mutable access. Its trusted local caller
provides canonical configuration, an existing
`ProfileLease` and matching `IdentityMaterial`. The owner borrows the
lease for its lifetime, checks the account and derives purpose-separated
keys from the profile SQLCipher root and profile identity. The caller
selects an existing local stage slot from the profile's bounded range. The private module receives a `QualifiedOrigin` whose fields and constructor
are private. In this increment its only constructor is test-only and can
mint a witness only for the scripted loopback TLS peer with fixture roots,
a singleton exact WSS URL, and direct route mode Off. The witness owns
that connector and is consumed on opening the raw lane. There is no
production minting path until a separate reviewed deployment proves the
parent plan's relay AUTH, reader/writer authorization, caps and retention
on a concrete operator-approved origin and defines Tor/direct admission
for that origin. This phase can compile its owner in production but
cannot dial an advertised URL, arbitrary private endpoints, or White Noise.
No form of unqualified origin can reach the invitation open path.
The module has no command authority or startup side effects. If not yet called in non-test builds,
any temporary dead-code allowance is scoped to this internal module.
The raw fixture connector remains test-only.

For each admitted origin, connect one dedicated singleton `RelayConnector`
with the exact configured URL and client account. Retain the existing
NIP-42 challenge, exact OK and live EOSE readiness sequence and per-operation
deadline. Untrusted Offer URLs cannot construct a socket. The wrapper
derives the checkpoint source only from the bound current socket owner
or an exact deferred row retrieved by that same qualified owner. It accepts
no caller-supplied raw record, source URL or generation for network intake.
The socket authenticates the TLS endpoint and local subscription ownership,
not the server identity in the Offer. The latter still requires inner Nostr
authentication and exact singleton route, audience, time and policy.
Keep memberships, subscriptions and state outside the ordinary Command
runtime and SDK seen tracker.

One explicit history step obtains cursor and reserved page, reads the
four-record bounded REQ, verifies the current generation, translates its
raw parser-owned bytes and commits only exact matching receipts. Production
code obtains the live record directly from that owner and synchronously
checks generation before disposal. The first successful scan selection on a fresh socket is Recovery.
Later selections on that same socket use Periodic, which resumes any
unfinished cursor and enforces the persisted cadence for a new pass.
A serialized step processes at most one page, one live record or one
permitted deferred retry. The authenticated owner's retry turn selects
one exact `due` snapshot from that origin, re-verifies it through
`Checkpoint::admit` at the currently qualified wall, then calls
`retry_again` on that same retained snapshot if it still defers. A
terminal disposition uses the existing exact deletion. Tests show a
perpetually deferred first row backs off so the next one is served.
The cursor's per-turn scheduling counters are ephemeral. The caller
applies the shared activation plan's aggregate and per-origin rate limits
and restart cadence before running turns. For this component, there are
explicit callable steps and no autonomous scheduling loop or relay
qualification assumption. Recognized fatal socket contract violations
retire the owner and cannot commit unaccounted page progress. Crash after
a row commit and before cursor progress repeats an idempotent effect.
Expected history resume uses a fresh socket with exact readiness.
A live record popped before a local error is recovered by catch-up. The
owner retires the connection when raw qualification fails, and all turns
check the cursor's durable quarantine before admitting traffic. A future
scheduler must prove requalification before clearing quarantine and
persist raw-side violations not detected by cursor receipts.
One qualified local Unix wall supplies all decisions. Sample it after
important awaits and before durable mutations. Reject any new mutation
with a sample less than the persisted eligible cursor or checkpoint wall.
Tests use a private disposable injected wall shared with native maintenance.

Offer policy remains Manual, Automatic or Off. Current policy and exact
selector must be checked before generating or publishing. The local
manual boundary takes a genuine connected Unix stream, reads peer UID
from the OS and compares it to the profile owner EUID. An exposed IPC
request will neither specify a UID nor get raw storage access. An automatic
approval follows the authenticated intake transaction. The private owner calls an internal `approved_work` method whose inputs
contain the exact selector but no local authorization capability. It can
only operate on a durable already approved row. The existing fixture's
UID-based helper remains private to the checkpoint and its tests. The
integrated manual operations mint `Local` directly after
`UnixStream::peer_cred` passes the same EUID predicate as the normal
IPC endpoint. No caller-supplied UID field exists. The actor can inspect
bounded selector summaries through the local boundary.
Discard remains local-owner only and keeps the durable replay fence.

## Native package custody

The profile layout uses a protected `peer` child directory, a fixed
`cursor.db`, and at most 16 fixed stage filenames `stage-00.db` through
`stage-15.db`. The stage index is a trusted local allocator choice. It
cannot come from a relay record. Use separate HKDF contexts
`marmish/peer/cursor/2` and `marmish/peer/native-stage/1` from the profile
SQLCipher root, bound to profile identity and account. The stage key
also binds the typed index. The profile holder creates the directory
with 0700 and fixed files with `create_new`, 0600 and no symlink
following. Initial cursor creation and empty stage allocation are
explicit local operations. Missing files on recovery or publication
fail closed. Reject symlink, hard link, wrong owner/mode and same
device/inode as the cursor. Check the peer directory before path use,
require a regular file with EUID owner, 0600 and a single link, and
require the same ownership/mode/link rules for any MDK `-wal` and
`-shm` and cursor `-journal` sidecars. The borrow of the exclusive
profile lease excludes concurrent Marmish opens or path mutation.
The private directory excludes other UIDs, and same-profile-owner
malicious filesystem races are outside this security boundary. Pinned
MDK still opens by path and can create missing files or repair modes,
so validate before passing it the path and recheck file inode and
sidecar modes after open. Plaintext or a different profile key fails
closed. Stage retirement and replacement of expired selections remain
outside this increment. A later daemon custodian manages the 16-slot
allocator and cleanup before enabling Peer.

Keep an actual upstream `AccountDeviceRuntime` for
`prepare_fresh_key_package` and encrypted `SqliteAccountStorage` from
the staged Peer proof. Reuse the pinned Nostr identity ProofSigner and
publication signing code through a narrow child of `mdk_adapter`.
Implement adapters that deny account activation, subscription, publishing
and aggregate receipt. Set the native maintenance wall with
`with_maintenance_sources` to the same qualified wall sample provided to
the actor. Record that sample before opening/preparing native package.
Fresh native preparation maps `AccountError::KeyPackageAnnouncementPending`
and `AccountError::ClockSkewBlocked` to unavailable. Keep the stage
file and existing invitation without exposing usable package bytes. No network path
can prepare or publish native packages independently.

Before generation, persist the native stable slot, bound with
domain-separated HKDF to stage key, account, exact origin, and Offer
selector. Check it on reopen and reject a slot bound elsewhere. A fresh
empty and bound lifecycle can prepare exactly once while an approved
Offer is live. For an existing pending artifact require it still match
our recorded singleton target, native reference, native lifetime,
and untouched and unconsumed lifecycle. Recheck its exact signed bytes
and package contents against the native lifecycle and canonical account.
A signed valid pending artifact is opened directly without invoking
MDK replacement or asking if it would regenerate it after an upgrade.
This increment pins the wire/profile/implementation. If a later build
declares that old proof obsolete, it must fail closed rather than
replacing signed bytes. A pending but unsigned valid lifecycle is the
native crash window. Reconstruct the exact persisted native
`KeyPackagePublication` from the stage account, `pending.key_package`,
`lifecycle.stable_slot_id`, `pending.authored_created_at`, and the
persisted singleton endpoint. Call the Peer signing adapter's
`KeyPackagePublisher::prepare_key_package` directly and write the
result to `pending.signed_event` using
`MaintenanceStorage::put_key_package_lifecycle` on that staged
`SqliteAccountStorage`. Only the fresh empty path calls
`AccountDeviceRuntime::prepare_fresh_key_package`, whose private
obsolete-pending detector can rotate a prior package.

On reopen require exactly `KEY_PACKAGE_GENERATION_REVISION`, a
matching supported staged component profile from the pinned generation,
a valid native reference and package lifetime, the unattempted target
with no failure or consumption, and absence of current, withdrawn,
published, superseded or retained slot state. Stages are isolated from
external lifecycle mutations. Reject any expired, obsolete, non-current
profile or unexpected state before preparing or signing. Test that
unsigned recovery uses the same native package and that such rejection
leaves the lifecycle bytes unchanged.
Singleton native endpoints bind local provenance, as the signed package
itself omits its publication endpoints.

Cross-database order is fixed. The approved invitation is durable before
native generation. Pinned MDK persists its native pending lifecycle and
private material before exact signed package bytes are recovered. Only
then may the checkpoint transaction sign and commit immutable Consent.
The same commit stores the verified native event ID and SHA-256 of the
exact package bytes alongside Consent. Before any actual send, prove the
current bound native stage owns that same immutable artifact. Re-opening
a file with no pending package cannot authorize an existing outbox.
The stage returns no caller-constructible origin or selector metadata,
and no uncommitted native material or outer frame can be exposed.

## Publication and recovery

Use a version-2 consent schema, with v1 refused in this previously
test-only increment. The cursor schema and its version remain independent.
Bump the consent version check to 2 and reject a mismatched version or
account before mutations commit. Each outbox row has the existing exact
origin, selector, outer carrier and immutable frame plus verified native
package ID and bytes SHA-256, nonnegative attempt sequence, optional
retry-at and optional acknowledged-at. A newly queued row has attempt
zero and neither timestamp. Reserve an attempt only for the exact
stored selector, origin and carrier. Recheck authorized current policy,
durable approval, both immutable native bindings, unacknowledged state,
and `wall < expires` with qualified wall. An existing retry-at greater
than wall delays the next attempt. Increment the sequence and persist
`retry_at = wall + 60` in one transaction before releasing a send witness.
This fixed bounded attempt frequency is independent of socket reconnect.

Only that private witness can select the stored bytes for a WSS write.
A separate raw socket method accepts exact bytes, canonical recipient
and expected carrier directly, without recreating a `Prepared` or
decrypting the server-addressed message. The checkpoint's scoped adapter
consumes the witness on the active origin owner. The same raw socket
operation serializes predrain, full outer signature and ID validation,
send and matching OK. It returns an acknowledgement witness carrying
the private socket scope and exact carrier. The adapter compares origin
and current generation and pairs it with the reserved sequence. Wrong
OK retires the socket. Negative OK, timeout, network interruption,
cancellation or ambiguous send keeps the immutable outbox through
expiry and the already durable retry time.

A separate transaction records a positive exact acknowledgement only
with the same row, selector, origin, carrier, both native bindings and
attempt sequence, and with a newly sampled qualified wall before the
outbox expiry. A newer attempt, discarded row, expiry or regressed clock
cannot commit acknowledgement. Recording observed acceptance is policy
independent. A later Off or blocked policy suppresses all new attempts,
and cannot reverse an acknowledgement. Keeping the original outbox
through expiry leaves exact evidence for a future Delivery. If process
or SQL failure loses the acknowledgement before commit, retry the same
bytes after the durable delay. Reopen after durable acknowledgement
makes no further attempt. This models relay acceptance, and grants no
room membership or command authority.

## Acceptance

Use real disposable Nostr keys and real TLS/NIP-42 against loopback, with
no external credentials or relay work. Reuse the scripted raw proof
fixture. Cover Manual plus real local OS owner approval, Automatic,
Off/blocked, recovered history and live intake, wrong-origin signed Offer,
exact acknowledged Consent and native withheld lifecycle. Show no direct
KeyPackage publication. Exercise negative and wrong ACK, stale generation,
same-byte retry after timer and cursor/native reopen, and no resend after
durable acknowledgement. Cover native pending/reopen before outbox,
outbox commit before send, durable retry fairness, closed admission on an
unqualified URL, stage symlink/hard link/wrong mode/missing file/missing
directory refusal, corrupt or missing stage recovery, and positive
ACK followed by failed SQL update with exact replay. At least one test
uses the full signed native exchange from Offer through opened Consent.
Check that a pending row cannot progress using the actor-owned work
path, and that an actual kernel credential must pass for manual actions.
Use a monotonic I/O deadline and disposable injected wall for retry timing.
Keep an opt-in loopback turn latency measurement alongside existing offline
benchmarks. Run the targeted core and WSS suites and full strict native
Clippy. Document exact test scope and measured duration. No changes to CI
or enabled public features are part of this increment.
