# Authenticate exact native SelfRemove using upstream public interfaces

This qualification increment follows the
[pending native invitation design](peer-pending-activation.md). It
evaluates the missing independent SelfRemove identity check against
upstream MDK `0ad3c61088e1acac58e0fcf1a9f1142bae5d997a` and its existing
OpenMLS resolution. The test adapter owns no network. Runtime enablement,
relay qualification, local decision persistence and convergence remain
separate gates.

## Candidate and authentication boundary

The eventual owner supplies the terminal decline generation, local
account, exact native group, qualified route and one expected
acknowledgement threshold. A sibling candidate, remote Offer or outer
event can supply none of these independently. The owner serializes native
operations and private-stage access. Bind the runtime and SQL reads to
the same native connection and private stage. Capture bounded outgoing
native requests with a deny-all physical adapter. A candidate might come
from a captured send or frozen fanout on reopen. A captured or retained
candidate carries no permit.

Acquire the owner operation before reading
`AccountDeviceRuntime::{self_id, own_signature_key, epoch_pin}`. Keep
ownership throughout both authority snapshots, the async peel, and the
final pin comparison. The independent loaded runtime account must equal
the qualified account. Carry the runtime's device signature public key
as the independent expected current leaf key. Keep the epoch pin in
memory and compare its epoch and identity again after the second
snapshot closes, before relinquishing the owner.

For each of two synchronous snapshots use
`GroupAuthorityView::with_group_authority_snapshot` on the owning
SQLCipher connection. Require a successful native authority read,
lifecycle Stable, active non-admin membership, no disband, and the
original native epoch. In both snapshots require
`LeaveRequestStorage::leave_request` for the exact group, with
`last_proposed_epoch == Some(pin.epoch)` and no blocked reason.
Look up the exact event ID using `MessageStorage::get_message`. Require
equal row ID and native group, equal epoch and `MessageState::Sent`.
Decode `StoredMessagePayload::decode` and match only
`SignedOpenMlsWire { exact_message, openmls_message, .. }`. Require
`exact_message` equals the whole candidate message, including ID,
payload and `TransportEnvelope::GroupMessage { transport_group_id }`,
and that its envelope routing ID equals the qualified room tag.
Require the stored inner `openmls_message.payload` to equal the peeled
bytes in the second snapshot. Apply explicit size limits throughout.
The native content marker does not substitute for any of these reads.

After snapshot one closes and before the peel, use
`NostrTransportEvent::to_verified_nostr_event` for recomputed Nostr
event ID and signature. Require its ID equals the exact request ID, the
request account equals the native runtime account, and the request
target and acknowledgement threshold equal the qualified endpoint and
threshold. Use `NostrMlsPeeler::prepare_group_message` to enforce kind
445 and `h` tag equals envelope tag. Read
`AccountDeviceRuntime::exporter_secret_with_epoch(group,
DEFAULT_EXPORTER_LABEL, 32)` and require the resulting exported epoch
equals the row epoch. This calls the engine's current group context
exporter. Construct `GroupContextSnapshot` with that epoch and room tag,
then peel the exact signed candidate with the Nostr peeler. The native
wrap round trip pins the exporter semantics. No snapshot spans an await.

In snapshot two, after rechecking all authority, leave and Sent facts,
read an available SQLite `group_replay_state_fingerprint`. Use public
`EngineOpenMlsProvider` with SQLCipher `StorageProvider::mls_storage`
and public `MlsGroup::load` to load a throwaway group. This is the only
raw OpenMLS load site. It is behind the native rewind-restore health
gate. Require that group's ID and epoch equal the same expected ID
and epoch, and that SHA-256 of its epoch authenticator equals the
observed pin identity. Require an active group without a pending
commit. Read `group.extensions().app_data_dictionary().dictionary()
.entries()`, require the entry with `NOSTR_ROUTING_COMPONENT_ID` and
decode its bytes with `decode_nostr_routing_v1`. Its Nostr group ID
must equal the owner-qualified tag, exact carrier envelope tag and
peel context tag. The one owner-qualified endpoint must be present in
that authenticated relay list. Physical output remains limited to the
one owner-qualified endpoint. Native room contents alone cannot expand
the allowed target.

Decode exact MLS TLS bytes. Require a public Proposal for the expected
group and epoch before calling `MlsGroup::process_message`, which
authenticates the membership tag and signature. Require the
authenticated group and epoch, `Sender::Member` with precisely the
group’s own leaf index, matching own leaf and processed credential
whose BasicCredential identity equals the independently read
`runtime.self_id()`, and the typed `Proposal::SelfRemove`. Require the
own leaf signature key equals `runtime.own_signature_key()`, so the
packet is from this device even when the account has sibling devices.
Do not use an unauthenticated proposal discriminator. Require the
replay fingerprint to remain equal before leaving snapshot two, and
compare the fixture's persisted fingerprint separately on reopen. The
query-only provider refuses writes even if a future OpenMLS refresh
changes proposal processing. The probe calls no proposal-store or
ratchet update. Any SQL or OpenMLS failure refuses authority. Drop
the temporary MLS object. Keep exporter, plaintext and pin private.
Never log native errors, identifying material or ciphertext from the
qualification.

Use the same OpenMLS git revisions and existing features resolved by MDK
for a scratch qualification. No MDK fork or changes to upstream. Any
direct OpenMLS dependencies in a follow-on implementation must preserve
the same types and be checked in the MDK update workflow.

## Retry and failure behavior

Only exact freshly revalidated requests can eventually receive a
one-shot publish permit tied to the durable local decline generation,
account, room, native branch, leaf, event ID and full request. The
fixture never gives an actual network adapter a permit. Reopen starts
closed and repeats all checks for one bounded eligible proposal.
Deny every unrelated item on native retry, even when it appears first.
Do not construct an output from a newest-row or sole-row heuristic.

If the native request survived but no exact fanout survives, retire
as unconfirmed within the normal deadline until a separate reviewed
re-staging proof covers that gap. Do not mutate MDK rows to make
retry possible. If the direct OpenMLS probe is unsupported or cannot
be proven rollback-only against public APIs, keep physical leave
disabled. A remote authoritative removal needs no publication.

## Qualification

Use disposable encrypted stages, authentic current-profile native
Welcomes and a no-I/O adapter. Validate a genuine current-leaf
native SelfRemove with query-only storage, close/reopen and exact retry. Reject
different account, route, branch, event ID or event bytes, and absent
or stale authoritative leave state. Reject an actual authenticated
non-SelfRemove in the same room and a wrong-device proposal and non-Proposal or private framing. Account
for any ungenerated negative vector in the exact evidence. Prove public API capability using
normal Rust dependency features, without relying on test-only
session getters. Compare persisted protocol state before and after
a successful and rejected probe. Include a negative injected storage
failure, and prove query-only mode by attempting a scratch SQL mutation
inside the fixture snapshot when a direct test handle is available. Distinguish a pass on this proof from deployed
leave, local approval and relay convergence.
