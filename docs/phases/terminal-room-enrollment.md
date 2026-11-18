# Terminal room enrollment over the authenticated bootstrap

This phase connects the existing signed Terminal bootstrap to real native
membership and ordered application delivery. It uses the volatile account,
withheld KeyPackage and SDK route selected in
[the transport proof](terminal-ephemeral-route.md). The private test-gated endpoint owner
implemented in this phase takes an authenticated Command binding and a live
grant callback. The later daemon/CLI phase supplies the binding from its
local ledger and connects the owner to attachment IPC. Use the actual
production-style room owner and sealed records in the loopback verification.
The proof never uses a test bypass for signatures, permission or membership.

## Inputs and ownership

Call the client and server roles in the established Command binding C and
S. Each owner lives under one exact ordered canonical pair, Command group
identifier, normalized explicit relay and generation. This phase qualifies
one explicitly selected direct WSS relay. The later owner chooses from the
profile relay policy, including the normal default resolution, and explicitly
handles multi-relay and Tor. Never open a transport on an address received
from a bootstrap payload. No outbound capabilities are public.

C and S complete Probe, Challenge, Confirm and Ready. Each checks local
authority again when using the confirmed transcript. Take the existing
confirmed random role keys and exact authenticated Ready rumor ID as the
capability. Use an owning take on the confirmed Exchange, available at most once.
Retain immutable transcript identity and deadline in the bootstrap lane. The bootstrap
owner can retransmit exact already-prepared records while live, but it cannot
issue two room allocations or two valid offers for the same Ready. Local
denial or generation retirement consumes the capability. Use an ordinary
30-second bootstrap deadline for Probe through Ready and a fresh maximum
30-second enrollment budget capped from first confirmation at each owner.
There is no restart recovery and no republishing on a new generation.

S owns the random 32-byte MLS group ID and the distinct random 32-byte native
route. C is the invitee. C pre-arms an exact #p Welcome subscription on its
fresh random account on the chosen relay. C opens one fresh SQLCipher memory
store, one slot and one pinned native runtime. Prepare the withheld
kind-30443 artifact, retaining its actual attempt-zero lifecycle in the
runtime. S may prepare its fresh store after producing Ready, in parallel with C.
Use a dedicated mutable single-room TransportRoutingPolicy, separate from
Command ExplicitRouting and its journal. Until registration it supplies
the exact configured Welcome inbox and no group route. At the server,
register the exact prepared room from the verified protected group state,
then sync the native #h subscription successfully before Welcome publish.
At the client, first verify the Plan against a volatile collision index of
current local Command, Peer and Terminal group and transport identifiers.
After native join and state verification, register only that group's
NOSTR_ROUTING_COMPONENT, which must decode to the exact Plan route and
relay list [configured relay]. Sync the native #h subscription successfully
before sending the first receipt. Require the completed subscription handle
and its exact issued ID in live delivery. Exercise the actual REQ and
subscription activation ordering over WSS.

Accept at most one pre-Plan Welcome of at most 48 KiB. Check the exact
TransportDelivery.source plane, endpoint and issued subscription ID after
adapter.receive, the signed outer kind and recipient, and the verified
ephemeral sender and withheld event source using
NostrMlsPeeler::peel_welcome_with_provenance. An unauthenticated envelope
cannot occupy the hold. Copy the one authenticated signed envelope to the
bounded hold, acknowledge its replay pin by exact signed outer ID and never
ack before securing the local copy or reaching a final rejection. The hold
has the enrollment deadline and is never persisted. Act on it only after
the Plan verifies. Conflicting valid responses, timeout, or ambiguity retire
the whole owner.

## Enrollment wire and native admission

Use the bootstrap aliases and the same exact NIP-59 outer and canonical
seal verification in one private v1 enrollment lane. Add a distinct
application kind 24863 with exact `package` and `plan` schema type values,
a closed schema, exact author and recipient, empty rumor tags, recomputed ID, exact role and
phase, and independent monotonic deadlines. The distinct kind has no inner
p tag. Outer recipient and verified canonical seal establish the capability.
The existing kind 24861 retains its exact alias tag. Refactor only common
outer and seal verification into a parameterized verifier. Keep the original bootstrap
kind, accepted signatures, size ceiling and replay behavior unchanged.
Enable larger parsing caps only after Ready. Check signed created time
within five seconds in the future and strictly less than 30 seconds old.
Before canonical decoding, cap the envelope at 64 KiB, outer content at
63 KiB, serialized seal at 46 KiB, decoded rumor JSON at 24 KiB, rumor
content at 20 KiB and decoded public KeyPackage at 12 KiB. Confirm the genuine package fits with
measured sizes. The owner queue admits at most four routed frames per
transport lane, with independent byte caps of four times each lane event
cap, one current challenge and one held Welcome. The pinned SDK adapter
also has its own bounded transport-notification frontier. The owner drains
that frontier continuously and retires on a reported gap. Ignore the SDK's
derived Event notification and admit only the raw EVENT with its actual
subscription ID so the same bytes cannot take a second ingress path. Excess work is discarded or retires the generation where it
would make current state ambiguous. Reject unknown fields,
wrong phase, conflicting duplicate, reversed role, over-limit age or size,
and replay after teardown. Within one live generation the same signed-event
or rumor ID is a no-op apart from an explicitly allowed exact retry.

1. C sends Package after receiving Ready and preparing its local slot.
   Package contains the exact Ready ID, both random role public keys,
   withheld signed event ID, MLS reference and canonical Base64 MLS public
   bytes. Retain the actual signed kind-30443 artifact locally. Revalidate
   the local pending lifecycle, package reference and authenticated native
   credential before sending. S parses with the pinned KeyPackage API,
   verifies its current profile and natural MLS signature through group
   creation, and independently verifies recomputed reference and credential
   against the client ephemeral public key. Set KeyPackageSource to the
   authenticated withheld event ID. No network publication, discovery or
   withdrawal of Terminal packages is admitted.
2. S creates a single group on its own fresh memory runtime, using protected
   name Marmish Terminal v1, protected description as a canonical ASCII
   versioned hexadecimal transcript digest, exact route and 60-second
   retention. The digest is SHA-256 of a fixed domain and length-delimited
   normalized command binding, canonical role keys, Ready ID, both fresh
   role keys, Package rumor ID, withheld event ID, MLS reference, SHA-256
   of public MLS bytes, chosen group ID, route and the normalized configured
   relay URL, Current native protocol profile and big-endian retention value
   60. Fixed-length values use raw canonical bytes with no delimiter
   ambiguity. Freeze a digest vector.
   S sends canonical-signed Plan containing exact Package ID, chosen group
   ID, route and digest. C verifies the digest and records the one exact
   expected room. A relay ACK for Plan, Welcome, or both never establishes
   application admission.
3. S publishes the real native Welcome over its separate Terminal adapter.
   C independently verifies the native NIP-59 decryption and verified
   ephemeral sender plus withheld package source before ingestion. Only
   the dedicated memory runtime may receive it. Match the used package
   reference and authenticate the exact chosen MLS group, protected name
   and digest, native route, the exact full app-component set, retention,
   initial canonical epoch 1, Current protocol profile, mandatory Current
   capabilities only, founder sole administrator, and exactly two approved MLS leaves with Unclaimed
   device state and roles as bound in Ready. Use the same structural native
   views as MdkOwner::check_group and require the retention component bytes
   via runtime.app_component to equal big-endian 60u64. The founder's
   local install/join epoch is 0 and the client installs epoch 1. The
   mandatory Current MLS capabilities are AppDataDictionary 6,
   AppDataUpdate 8 and AccountIdentityProof. An incorrect native result destroys the entire
   ephemeral runtime without passing any record to application delivery.
   Even a Welcome from the right canonical counterpart is unauthorized
   when the random account or package revision does not match.
4. After C joins, validates state and syncs its route, C sends JoinProof.
   S receives it and sends ServerReady. C receives ServerReady and sends
   ClientHello. All three use a dedicated native app kind 24864 and closed
   explicit types with exact transcript digest and verified role. Accept a
   receipt's native-authenticated creation time no more than five seconds
   in the future and strictly less than 30 seconds old. S may
   expose no PTY nor output until it authenticates ClientHello; C may start
   accepting application traffic only after sending ClientHello and in the
   same bounded enrollment window until ordinary native progress. One
   outbound receipt may be in flight in each role. Exact duplicates cause
   no effect. Send each native receipt once through runtime.send. A timeout
   or uncertain application receipt ends the owner, so nothing queues a
   duplicate native app message. Canonical alias retries preserve the
   original signed outer event and are bounded by the enrollment deadline.
   The frame lane uses [its own kind](terminal-frames.md). Check live grant
   and generation before emitting receipts, exposing the owner, or
   processing the first frame. ClientHello absence leaves S incapable of
   interacting with tmux. The pinned runtime may report a pending
   post-join rotation after it publishes the application record. Keep the
   initial canonical epoch and skip maintenance fetch, convergence and
   rotation during enrollment. A connection or inbox subscription attempt
   change retires the owner and never updates a live binding in place.

## Backpressure, expiry and verification

Keep the Command engine, profile storage and ordinary relay routing outside
this owner. Bootstrap aliases and each random account have independent
adapter, direct SDK client, subscriptions, bounded ingress and volatile
replay state. Check configured relay, actual subscription, event kind,
exact route/recipient and bytes before native peel. Recheck grants and
generation at each effect and write boundary. Match the native queued delivery
ID to the raw signed ID. An owned alias task continues verification after Plan
or Package acceptance, so concurrent room operations cannot interrupt an alias
placement before it is verified and acknowledged. Unknown envelopes, third-party
tags, unexpected native effects or state changes and stale owner handles
never reach frame delivery. No inbox fallback, extra leaves, account
enrollment, ordinary journaling, package maintenance or automatic subscription
expansion is allowed. Closing the owner drops the volatile account, routes,
pin ownership, pending records and SDK connection. Revocation cancels an
in-flight enrollment and fences its subsequent deliveries. No local durable
history, raw payload or key is emitted to diagnostics.

Run a small offline core selection covering the bound values, fresh generation,
invalid envelope and grant-cancellation checks. Keep native loopback WSS and
timing suites opt-in. Use real signed bootstrap, unopened kind-30443 candidate,
native Welcome/join and both directions of application frames end to end,
with existing verified owner/observer controls applied at the frame layer.
Exercise wrong recipient, group ID, route, relay, protected purpose and
digest, expired and replayed records, enrollment order, duplicates, Welcome
arriving before Plan and grant revoked both before join and before frame
delivery. Send a byte-exact 4 KiB sample including every byte value.
Reopen the ordinary SQLCipher Command profile and compare its command
ledger, native deliveries and package lifecycle with the expected baseline.
Assert exact protected members and route before first receipt, native
expiration, zero Terminal KeyPackage relay writes and separate subscription
teardown. Collect initialization and Ready-to-Group-Ready p50/p95 and
steady-state one-way frame timing, include first-run and warmed measurements,
and record sample counts, wire sizes and failure rates. Use actual public
relays only in a separately bounded interoperability exercise with disposable
identities. Claude reviews this design before implementation and the exact
integrated code before publishing.
