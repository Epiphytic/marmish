# Installed Peer persistence for signed v2 enrollment

This increment adds the checked recipient admission and restart lookup that
the installed Peer owner will use for a signed v2 Offer and Prepared Consent.
It pins the wire version at the same durable point as the authenticated Offer.
It adds no new acceptance, egress or user-facing entry point.

The implementation base must include exact published tip
`716f066b1a8467aa77ff0970ac92ed65e316b0ca`, including
[the v2 codec](peer-installed-control-v2.md), and the approved installed
heartbeat phase A design (`3db4a36`, blob `b1304c7`). That design defines the
later mount and both directional acceptance proofs. Integrate that document
before starting this increment. Existing v1 paths keep their exact inventory,
codec, and behavior.

## Checked store

Use the existing hardened Peer `CursorStore`, `Checkpoint`, `Source`,
`Route`, `Selector` and checked SQL writer. The attached layout selects
`Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2)`; the legacy
`Checkpoint::attach` stays on v1. The v2 opener is private, needs the same
checked SQLCipher connection and attested exact origin as its eventual
leased owner, and takes one exact route. Its tests supply a disposable
identity and use the same storage opener. Neither layout autodetects, upgrades
or opens the other. The legacy attach rejects the installed marker or
namespace before running SCHEMA or provisioning attach. The installed
opener creates its marker only if no consent/provisioning inventory exists,
on a new checked store with no prior invitation, slots or outbox. Otherwise
it requires the complete installed inventory. The existing
`peer_consent_version.version = 2` is the consent SQL schema version. The
new marker is independently the signed `ControlVersion::V2`. There is no
import of old experimental or fixture state.

Add an exact inventory discriminator through a new STRICT table
`peer_installed_offer_v2`, with these fields and constraints.

| Field | Constraint |
| --- | --- |
| `server` | 32-byte canonical founder, equal to the verified Offer author |
| `origin` | 32-byte exact configured Peer origin ID |
| `offer_id` | 16-byte signed Offer ID |
| `revision` | 32-byte signed Offer revision |
| `version` | integer with `CHECK(version = 2)` |

Its primary key is `(server, origin, offer_id)`; a foreign key on
`(server, offer_id, revision, origin)` references the exact unique tuple on
`peer_consent_invite`. Use ON DELETE CASCADE for this marker only, with
existing parent restrictions on slot, Prepared Consent and custody. No
opener mutates a populated legacy inventory. A recipient-only table
inherits the checked local account from the existing singleton and
connection, and the sole origin from the opener. The namespace and its
whole checked inventory distinguish it from custody epochs and test
publication layouts. Founder admission in the installed owner will need
its own direction-specific source and is outside this schema.

Insert the marker in the same IMMEDIATE cursor transaction that checks
the sender and policy, then inserts the exact retained signed Offer and
its existing revision fence. Admit only a parsed v2 `Offer`, with exact
recipient, author and route. Reject v1 before touching trusted application
state. The current `peer_consent_invite.server` primary key makes a different
Offer from a retained founder, including a competing same-ID revision, use
the existing capacity outcome `Backlog` and cursor disposition `Deferred`.
No marker or fence for that competing revision is written. A repeated exact admission checks the
existing marker and retains current idempotent replay; it cannot add a
marker to a legacy row. Preserve existing bounded invitation and replay
retention, including the original expiry and fence. Retain the v2 parent and marker through its existing bounded
replay horizon in the v2 sweep. Use `replay_until <= wall` for the v2 parent
in place of the legacy expiry check, retaining the existing no-live-outbox
and no-claimed-slot predicates, plus exact checked FK/ancestry protections
when the Prepared/custody layout is eventually composed. Open the marker
delete aperture only around the guarded sweep. Its longer occupancy consumes
one of the existing eight slots per class and blocks that founder
for the signed replay margin beyond expiry. Bound defer uses the existing
cursor handling and can retry after reclamation. Explicit discard and legacy
queue/decision entry points return unavailable on the installed layout. The
current v1 sweep, discard and capacity behavior stays unchanged.

Use denial triggers on direct insert/update/delete. The checked writer
opens only the insertion aperture or the exact existing parent retirement
or sweep aperture as required by the cascade, and restores and verifies
the guards before commit. No marker update operation exists. The recipient
scope checks an exact one-to-one correspondence with retained parents and
the singleton account, one configured origin and all existing store
invariants. Reopen of an unsupported, malformed or partially populated
inventory is unavailable, and cursor progress stops if existing local
provenance does not verify. Installed v2 admit always writes `decision_at = NULL`, including with
Automatic configured and in a non-test build. Installed v2 uses the actual
trusted exact cursor writer so its cleanup cannot clear a competing carrier
by shared ID. The writer must distinguish stable invalid incoming version
from corrupt or unavailable local authority.

On restart select the exact marker and parent under the same checked
transaction. Reuse `check_signed_offer` and its exact selector, carrier,
expiry and replay checks with the v2 codec, persisted local recipient,
founder and exact route. A live lookup uses fresh trusted wall and requires
`wall < expires`. The separate historical read uses the existing custody
pattern `expires.checked_sub(1)` only for signature reconstruction and
requires the retained horizon under the real wall. Its result is historical
material only and cannot be accepted by a live work API. The live result
is a private immutable authenticated Offer. An incoming Delivery must
authenticate that same selected v2 ancestry before custody or a native owner
can act. This increment leaves the actual Delivery/custody mount to the
successor below.

## Code boundary and merge test

Own `peer_consent_checkpoint.rs`, its current rows/inventory helpers and
one small installed schema module. The existing Checkpoint, CursorStore
and v2 Offer decoder already compile outside tests. Select the layout and
codec there. Make the existing `commit_trusted_exact` wrapper in
`peer_cursor_retry.rs` available to this checked store without changing
ordinary callers or replay semantics. No `cfg(test)` proof runtime, WSS
adapter, fixture constructor or activation schema moves into the installed
daemon for this increment.

The existing full Prepared Consent and custody proofs stay on their
fixture layouts. This increment exercises persisted/reopened v2 Offer
with the existing v2 Consent writer and exact pure `Transcript`
reconstruction in its own deterministic tests, using the live Offer returned
by the checked lookup. It does not run a v2 parent through the legacy
fixture lookup. In the next real owner mount use the checkpoint-selected
codec and exact installed Offer lookup in `peer_prepared_consent.rs`
(`read_live_prepared` and saved Consent reconstruction) and
`peer_pending_custody_checkpoint.rs` (`exact_consent` and Delivery
commit verification). Those paths currently construct v1 codecs. They must
route by the checked attached layout so v1 fixture callers remain unchanged.
The installed mount must exercise these same admitted bytes and reject the
same version and lineage faults in a separate real native, private exact WSS
activation test.

Add deterministic tests with temporary checked SQLCipher state and real
disposable signed v2 control inputs. Cover valid admission and exact live or
historical reopen, original expiry/replay treatment, already-applied replay,
deferred competing revision, slot exhaustion and recovery at replay horizon,
unsupported fresh/open layout, v1 attach rejecting v2, transplanted account,
wrong founder or route, corrupt/missing version marker, v1 input at the v2
opener, missing or mismatched saved ancestry, rollback at Offer/marker write
and guard restoration, and sweep with a claimed stage slot. Exercise v2
Prepared Consent transcript reconstruction from lookup and pure rejection of
a substituted v1 Delivery. Test Automatic with the actual unconditional
installed admission branch and confirm `decision_at` stays NULL and
legacy work/outbox refuse. Run that deterministic case in the release
profile as well as a non-test release `cargo check` of the native crate.
Only the deterministic tests are candidates for core CI.

## Installed owner follow-through

The next installed-owner increment has one evidence chain per direction
under the same live profile lease. It must use the authenticated v2 record
above, the user's current grant/block and role, and a current qualified
origin. Freeze the exact successor contract before mounting the owner.

- Recipient `peer_activation_active` is committed when receipt publication
  is reserved. Before reusing M5, make its receipt reservation and restart
  reconstruction explicitly v2-aware, using the sealed
  `StatusNegotiation` from verified Delivery for the six receipt
  coordinates and exact canonical signed bytes. Reuse the checked
  publication states and once-issued egress. Require exact `kind=receipt`
  Published after native `ConfirmedReceipt`, plus fresh exact local
  native owner and source readback. Any separate materialization must
  occur before `acknowledge_published`, use the Published wall as the
  historical local completion timestamp, and fail closed on new
  publication or native effects after expiry. This milestone records local
  completion.
- Founder uses its locally authenticated signed Offer, exact verified
  Consent, native Welcome and signed Delivery under its current execution
  client grant. Recompute the v2 receipt expectation from that sealed
  negotiation and the **post-rotation** native canonical pin. Reuse the
  account-runtime-generic `peer_accept_receipt_verify::pending` snapshot
  and exact sender/leaf check. The old founder SQL singleton and
  `PeerFounderRuntime` retirement are fixture-only. The installed owner
  needs a directional exact prepared selection and locked native ownership
  with new bounded allocation. Commit the authenticated source's inner
  app ID and content-derived native MessageId before local ACK. Preserve
  exact historical readback/cleanup after a lost response, without
  treating the outer wrapper identity as a durable native source.
- Use one serialized owner and recheck its unchanged native readback
  before each acceptance SQL commit. Persist group, app room, directional
  pair, origin and exact routing component, local generation, file witness,
  full epoch pin and both canonical leaves, and authorization revision.
  Native membership drift, stale generation/qualification, replacement
  or current grant/block closes authority. Stored SQL alone never mints
  the later live heartbeat permission.
- Enforce the approved owner-level combined bound of 64 retained bindings
  and the charged-native storage bounds when allocations are wired. The
  current original recipient files occupy at most two fixed slots and
  cannot be recycled while retained. Keep this lower capacity unless
  separately approved allocation work changes it. This increment adds
  no file moves, quota registry, founder file creation, receipt transition,
  enablement or status authority.

The real owner activation test must cover both directions, the correct
current native pair/epoch, acceptance after durable receipt evidence and
a clean restart, plus wrong route, wrong leaf and ambiguous native
publication. Neither fixture-only SQL history nor this design supplies a
production eligibility result.
