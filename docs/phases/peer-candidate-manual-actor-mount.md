# Assemble the private Manual recipient actor

This supplements the approved [Manual actor](peer-candidate-manual-actor.md)
and [daemon startup composition](peer-candidate-daemon-composition.md). Source
anchors are `efc3682443a79875860d9e94c4d5bb1cfa0d7e74` for the disjoint
registered borrows, readiness and ordinary fatal contracts,
`5b319577a1cd5606eb236b5e304069a468907e5a` for the raw route cut and
`40cb464e5358ebcaa23af7932ad65461ee090e59` for the metered package producer.
The compatible raw route, registry-issued fatal authority, Command keeper and
metered package composition is `194d1902cb7838f376c8229e0377d14ce511341d`.
Pin its independently reviewed publication tip before starting source work.
The first qualifying executable serves only a Ready-at-acquire Manual recipient
in Client or Both on Linux AMD64 with the explicit private candidate cfg. It
needs one qualified direct WSS origin, with the existing exact local trust
checks. The external founder is disposable test infrastructure. Stock,
installers, Auto, founder mode, approval, Active, Group traffic and heartbeat
gain no entry points in this phase.

## Mount and ownership

1. **Daemon and listener.** At `daemon::open_inner`, only a normal daemon run
   can try the candidate. Preserve config/overrides, identity provider and
   ordinary checks. Select the scope and register before
   `native_preflight::check` or any ordinary keyed open. Keep the original
   `Arc<ProfileLease>` and move the original `IdentityMaterial` to the actor;
   never reopen the provider or acquire a second lease. Follow the approved
   registration failure table. Carry the read-only `ForwardingVerifier` on the
   ordinary side even if Peer is refused locally. Recheck through ordinary open,
   binding validation and handoff. Consume
   `RegisteredReceiver::take_command_fence` once before ordinary keyed open.
   Registration already owns the one read-only Fatal shared by Command,
   Terminal/stream/IPC and the Peer route. Open Command through its exact
   fenced adapter and arm the existing physical-only fatal join path.
   The candidate route call is
   `ReceiverRouteAdmission::registered(RegisteredRoute)`. That transfer derives
   the raw read-only Fatal from the same registry record. Fixture-only admission
   stays outside the non-test candidate.

   Use existing `run_inner/serve_owner` and the existing Unix listener.
   Register after verifying the actual kernel-bound address and matching the
   exact nonzero listener generation. Bind the view's pair to that generation.
   Announce OrdinaryReady only after the first non-degraded Active ordinary
   update and successful startup parent acknowledgment when applicable. The
   listener and ordinary edge can arrive in either order. Before both, the new
   current-thread runtime waits on `WorkerStart` and does zero Peer cursor,
   managed-stage or WSS work. The existing test lifecycle drives no production
   mount. Keep the candidate listener handle separate from the test writer
   intake. It carries only registration/liveness and the memory Reader.

2. **Attach, cursor, route.** Inside the actor's own thread after
   `WorkerStart::ready`, recheck Ready and the forwarding invariant before
   `RegisteredReceiver::attach(&lease, &material)`. Take
   `(quota, route_control) = attached.actor_parts()`. The composed
   `ReceiverRouteControl::package_origin` gives the exact checked origin before
   route transfer and `RecoveredOwner::open_registered` already accepts the
   disjoint controller and borrowed quota. Verify quota/lease,
   retained directory identities, material continuity and the exact origin
   under the existing registration checks. Open the fixed 49 MiB-reserved
   installed cursor through its existing keyed/schema barriers and hold one
   active Checkpoint and Source. Fresh initialization may use the existing
   sequential schema bootstrap handles, never two live cursor owners. A restart
   uses the same keyed file and a new owner only after prior physical join.
   Reconcile measured inventory under this owner before taking the route.
   Package `NativeStarted` and durable `JoinStarted` remain governed by the
   existing exact recovery and no-repeat rules.

   Check readiness and remaining budget immediately before and after each
   synchronous startup operation and before the one-use `take_route`. The
   remaining duration comes from the second edge's recorded standard
   monotonic `Ready` Instant, including scheduling delay. Suspend-aware
   continuous limits still apply to later managed native turns.
   Convert the remainder to an absolute Tokio
   deadline immediately at handoff, cap it by physical route/TLS expiry, and
   recheck Ready and the original budget after the authenticated open. There
   is no second start clock, no retry against another endpoint and no separate
   actor cursor for route opening. A synchronous operation which overruns does
   not release resources until it returns. Keep the exact paired raw drain
   witness outside the opening future, including before TLS and when no port was
   minted. On normal stop, an already active port may run bounded close
   while its authority remains. On any startup abort after route transfer
   or on stop, end the opening future or port and call the new idempotent,
   non-consuming `ReceiverRouteControl::revoke(&self)` before
   `PeerSocketDrain::reap`. The controller can only cancel the exact
   retained registration token paired with its transferred route.
   Fatal already cuts raw I/O and skips clean protocol close. Retain
   ordinary owner/lease and attached borrow lifetimes through actual
   cleanup, including a stop during open without a minted port. Route
   loss stops the generation and its view. The candidate does not reconnect.

3. **Service the existing v2 state machine with the same owner.** Place the new
   candidate `peer_installed_recipient` runner under `peer_invitation`. Keep
   the cursor, quota borrow, controller, port and sole view producer as
   disjoint actor-local values. Use `PeerRecipientPort::use_page/use_live`
   and exact carrier apertures for Offer/Delivery, and
   `use_current_recipient` for local work. All callbacks use the port's actual
   account/origin/verified wall, the same Source, the exact leased material and
   selected row/generation. Inside bounded synchronous callbacks recheck the
   readiness, route/fatal, exact file and forwarding fences at the first use,
   immediately before writes and after I/O or refusal. Authenticate first, then
   classify and commit using the existing caps/receipt/deferred rules. Keep
   invalidated inputs and incomplete page transactions out of both view paths.
   Bound history pages, live records, wakeups and native work with the existing
   per-turn bounds and fair batch ordering. Never reset the absolute readiness
   allowance for a partial setup or turn a receive timeout into an empty page.

   Production-shaped package operations already expose
   `start_package -> prestage_package -> issue_package -> attest_package`.
   Reuse their single-use tokens, schema-only 15 s prestage window, separate
   5 s issue/attestation windows, scope/expiry guards and retained charge after
   ambiguous cuts. Share readiness revocation by lending a clone of the
   existing nonauthorizing watch receiver from Ready. Build the package/native
   fence with the same owner stop and route cancellation; keep fallible
   checks before/after every required aperture. The verified route time
   supplies the anchor. The independent native budget is suspend-aware.

   Carry the exact encrypted original digest `image: [u8; 32]` that
   `RecoveredOwner::attest_package` already compares into its private
   `PackageProof`. Keep native attestation construction sealed. In a
   candidate build, only an exact `PackageProof` from the attached managed
   owner can provide this proof. Keep the existing unmetered
   `NativeAttestation` construction and `SnapshotArea` fixture path under
   test. Reuse the pure comparisons in `PreparedConsentOwner` and
   `InvitationState`. Candidate verification uses measured
   `ReceiverFiles` and `NativeFence` to re-digest the original on the
   registered family and check exact stage/file/artifact, signed package
   and authority at each relevant effect. This includes Consent signing,
   queueing, attempt reservation, pre-flush and ACK settlement, and fresh
   or retained Delivery verification. Proceed after the file checks close
   within the same bounded current turn. Preserve the signed and
   qualification deadlines. A storage or proof failure uses the existing
   error and conservative durable outcome instead of a new attempt.

   A concrete candidate `ConsentTurn<'turn, 'lease>` borrows
   `&'turn mut RecoveredOwner<'lease>` and the same attached
   `&'turn ReceiverQuota<'lease>`. `PeerRecipientPort` drives this borrowed
   turn through the existing reserve, before-flush, settle and Delivery
   wake sequence. Place the adapter beside the installed Consent port.
   Offer and Delivery service/retry borrow the same recovered checkpoint,
   Source and invitation paths for a selected turn from
   `RecoveredOwner`, implementing the currently fixture-bound custody
   verification with its sealed managed proof. Pass the current checked
   callback account/origin/wall and locally rechecked native fence. A route
   callback can lend only copied relative clock data and an opaque
   revocation observer. No generic proof producer, alternate endpoint,
   cursor, SQL opener or file family enters these APIs.

   Keep the exact before-flush durable check and scope-only in-memory
   revocation for each write-driving poll. Preserve exact ACK use and
   matched attempt CAS. Existing acknowledgement uncertainty and cold
   deferred outcomes still govern retry. A missing ACK cannot authorize
   Held or another physical attempt after losing the route. Keep
   `peer_installed_consent_fixture::Fixture` and its independent cursor
   constructor out of the candidate mount.

   Extract a borrowed-owner v2 Pending entry from
   `peer_receiver_v2_pending.rs`. Its current `PendingReceiver::open` exists
   only for the fixture and opens a cursor after a port is live. Retain that
   proof entry under test. The candidate entry takes the actor's already
   open `&mut RecoveredOwner`, attached quota and current checked port.
   Use only the existing capped original Welcome poll and separately
   charged exact Joined readback against authenticated retained Delivery.
   Only a successful current Held fence can mint the single-use
   `JoinStarted`. A restart with JoinStarted only rechecks native state;
   an ambiguous or expired row remains charged. Distinguish readback that
   commits Pending once from read-only current Pending requalification.
   Move metrics, fault hooks and legacy writers to test cfg if their
   parent module is promoted. A repair-requiring Pending image is unavailable
   to the view; no native rollback is called from Pending projection.

4. **Memory publishes with checked owner proof.** Promote the existing
   private `Producer` ticket/invalidation/publish machinery without exposing
   its raw Observation constructor. Put sealed proof construction into the
   exact owner witness branches. The view receives only the ticket and the
   bounded redacted projection from a sealed proof. Keep the producer
   non-Clone and the Reader incapable of issuing any ticket. The ticket pins
   actor/listener/sequence and captures `Instant` and a continuous clock
   before the earliest relevant read/native fence. Invalidate before any
   operation which could change the image. At publication both ages must be
   strictly below one second, including native copy, cleanup and SQL readback.
   Keep the view and clock lock short; neither encloses file, SQL or network
   I/O. Independently refresh on a 500 ms maintenance cadence.

   A positive row requires the exact visible Pending coordinates. After CAS
   or clean read-only snapshot native proof, synchronously recheck current
   account/origin/wall, row and generation, signed ancestry, Held timestamp,
   Pending deadline, native group/epoch/leaf, and effective persisted wall
   floor. Compare the row before/after replay. Neither stage presence nor a
   successful OK/EOSE is proof of Pending. Functions that recover, mutate
   or attest packages never run from IPC.

   **Healthy Empty** requires exact held authority and inventory absence.
   In the same ticket prove the current registered lease/route and Manual
   policy, recovered cursor/source, floor, no unclassified recovered or
   outstanding eligible, deferred, stage or custody ownership, and no
   remaining charged file/row outside fixed cursor reservation. Use the
   completion of this actor's authenticated recovery scan on its current
   port incarnation. Keep that barrier actor-local. Mint it only after the
   last exact checked page transaction commits the scanner's completion
   with no unclassified/deferred work. Bind the barrier to that route,
   cursor/source generation and completed scan coordinates. Old
   `scan_state.completed_wall` can reconcile progress but cannot alone mint
   the runtime witness. Invalidate it on new incomplete recovery,
   unclassified input, changed source/scope or route loss. Retain it across
   clean 500 ms local refreshes if checked coordinates and floors still
   match. A timeout, truncation or unavailable authenticated page leaves
   the barrier unsatisfied. This assertion covers only the qualified
   origin and the bounded scan's completed coverage. Check absence against
   durable counts and measured inventory without changing the floors.
   Preserve exact-expired-proof List masking until that proof goes stale.
   The existing wire `List { entries: [] }` is the same for the mask and
   fresh Healthy Empty. Qualification distinguishes them by the proved
   outcome in the actor and by the controlled harness history. Ambiguous,
   charged, lost or expired-without-current-proof is Unavailable.

   After UID, frame and existing per-connection admission, the real
   listener acquires a nonwaiting Reader permit and holds it through the
   bounded write attempt. `Read::reply_before_write` alone resolves List or
   exact Show against listener generation, request deadline and rechecked
   view stamp/time. Synchronously stop the shared view at listener/actor
   revocation without waiting for native work. No request submits work to
   the actor, wakes its timer or touches storage, filesystem or a relay.

5. **Exits.** A local problem with the independent global invariant still
   valid stops only the Peer generation and leaves ordinary operation
   running. Any suspected displacement/poison gets the independent global
   verifier even when the local operation already failed. Before ordinary
   ownership exists, drop what was acquired. Once it exists, trip the actual
   [physical-only fatal short circuit](peer-candidate-fatal-command-keeper.md)
   before queueing ordinary stop or taking a further SQL turn. Preserve
   existing durable finite workers, close streams and physical transport
   ownership, skip ordinary logical reports/settlements, and never fall
   back onto an unverified VFS. Actor exit class and a true thread join govern
   final disposition. Keep the exact lease through every retained handle
   and report an incomplete join as incomplete. Neither views nor raw
   transport reaping can update terminal custody or release its charges.

## Ownership and gate order

| Increment or owner boundary | Exact files and review concern |
| --- | --- |
| Root coordination with daemon and provenance owner | `daemon.rs`, `ipc.rs` and one candidate revocation on `peer_candidate_registration.rs`. Real watcher/report edges, existing listener and ordinary fatal entry points. |
| Receiver actor and local projection | New `peer_installed_recipient.rs`, its `peer_invitation.rs` mount, the read-only `Ready` accessor, `peer_pending_read_view.rs` proof adapter, existing `peer_receiver_inventory.rs` checked borrow, candidate diagnostics and private actor tests. |
| Candidate package/Consent/Delivery bridge | `peer_candidate_package_owner.rs`, `peer_prepared_consent_owner.rs`, `peer_installed_consent_port.rs`, the existing offer ingress, `peer_installed_delivery_{service,ingress,retry}.rs`, and necessary leaf cfg and exact checkpoint apertures. One managed proof and turn-local re-digest. |
| Capped Welcome/Pending and view facts | `peer_receiver_v2_pending.rs`, `peer_installed_pending_checkpoint.rs`, narrow same-owner durable absence/floor read and focused tests. One cursor throughout. |
| Independent acceptance | Test-side qualified WSS/founder fixture and installed process/bench harness; no fixture capability in the candidate |

Coordinate adjacent one-line mounts centrally. First compose registered
borrow/clock, measured package, raw provenance and controlled fatal source
and keep the IPC view unavailable. Prove ordinary startup, both readiness
edge orders, early abort, one lease/cursor, dispatch forwarding and actual
fatal stop before enabling a candidate actor. Next verify same-cursor
package through Pending, without exposing positive read replies. Finally
review exact sealed observation/IPC integration and run the installed
acceptance. No implementation starts on this supplement until the exact
code owners and interfaces are agreed.

## Acceptance and latency

- Compile probes and symbol audit on stock, ordinary installed-check and
  private candidate builds. The candidate cannot construct a fixture
  admission, raw observation, test clock, generic proof provider, legacy
  writer or second quota/cursor. Keep the existing stock core CI.
  Run candidate check, Clippy and focused tests as documented opt-in
  commands on the existing native self-hosted Linux AMD64 host. Use the
  same host for full real private runs and latency samples. Add no
  GitHub-hosted runners or new CI legs.
- Use the explicit candidate build marker, actual installed non-test
  `marmishd` and CLI, and real installed foreground and background setup.
  Generate distinct disposable receiver, external founder and ordinary
  Command identities. Bind private loopback TLS/NIP-42 and explicit
  qualification. Prove a fresh no-row Ready profile yields an
  authenticated Healthy Empty, startup before the history barrier remains
  Unavailable, and an ambiguous charged profile also remains Unavailable.
  Exercise Client and Both, including real same-daemon Command and Terminal
  across available/unavailable Peer. Observe durable v2 one-package
  Offer/Consent/Delivery, one native Welcome and exact inert Pending.
  Terminate and start a different process on the same profile. Show the
  exact row through new capped readback, no second poll or package, exact
  List/Show, wrong selector, expired row and unavailable route. Verify
  same-UID read only, four slots with immediate saturation, stale windows,
  listener loss/final-write race, and no request-attributed SQL/native
  operations. Candidate remains incapable of approval or activation.
- Unit/integration fences cover late wake counting from second edge,
  loss before open and on both sides of native effect, exact normal stop
  revocation during raw open and mid-exchange with both halves drained,
  source/account/
  origin mismatch in the checked callback, wrong profile/material or
  cursor generation, failed durable CAS, cold Held/JoinStarted/Pending,
  ambiguous charge, lost latch after SQL has begun and exact lease
  retention until physical join. Reuse previously approved refusal and
  raw WSS matrices for leaf coverage, then prove at least one local and
  one global failure across the actual installed boundary. Never use
  mock EOSE or a prebuilt Pending row as the installed positive proof.
- Report 20 consecutive scheduled near-cap Pending refresh attempts on
  the qualified host after cold real Pending recovery and also 20
  independent List/Show requests with exact result/timeout counts; include
  Healthy Empty samples separately. Establish the heaviest valid image
  the test harness can actually reproduce under the 16 MiB closed-stage
  cap with separately charged copy and total charge strictly under
  128 MiB. Record exact logical and allocated extents, total charge,
  native validation, host and build. Aim for an exact valid original with
  at least 15 MiB logical extent and under the existing 16 MiB stage cap.
  An offline SQLCipher fixture may prepare the image if it preserves the actual
  MDK schema, independently qualifies the same signed/native Pending
  row under cap after fresh process start, and uses the same measured
  fixture interfaces. Before relying on such a fixture, prove exact image
  validity and charge. No sparse padding, raw fake page count or release
  test hook. If a valid near-cap image cannot be produced, record extents
  and keep the near-cap qualification open.
- Measure origin-before-first-read to actual publish admission on both
  clocks, phase timings for readiness, first authenticated WSS and
  package/Welcome, consecutive publication gaps and client reply latency.
  Use p50/p95/max (nearest rank), publish the attempt count, late/refused
  turns and bounded reasons separately. Include an IPC-saturated sample
  of the same actor without assigning independent reads to actor work.
  p95 of successful scheduled near-cap origin-to-publish latency must
  be at most 500 ms. Each counted publication must take under 1 s on both
  clocks. For every consecutive publication pair, the successor must be
  admitted before the earlier ticket's origin plus 1 s on both clocks.
  Report maximum measured publish-to-publish gap and minimum remaining
  prior-proof slack alongside that criterion. Retain misses and the
  longest unavailable interval
  so dropping late samples cannot masquerade as meeting the availability
  gate. On the actual candidate executable extend the opt-in `--debug`
  diagnostics with fixed event/outcome enums and numeric relative durations
  for scheduled attempt, both proof clock ages at admission, and elapsed
  time since the previous publication on both clocks. Capture numbers on the actor and
  emit after the proof decision, outside locks and physical/durable
  effects. Time client IPC independently in the harness. Incomplete event
  sets cannot qualify publication gaps. Permit no address, arbitrary text,
  time-of-day, selection, identifiers or payload in the event. Disable
  emission by default and compile this schema only in the candidate. Report
  output overhead as part of the apparatus. Do not make telemetry an input
  or fault injector.
