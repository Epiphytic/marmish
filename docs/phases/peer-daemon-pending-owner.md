# Daemon ownership of pending Peer invitations

## Scope and dependencies

Bind Peer intake, inspection and approval to the real daemon ownership path and
same-UID Unix listener. In an ignored native case, use disposable profiles and
two independent private TLS WebSocket relays. Observe durable Pending, one
same-UID Manual approval and Automatic opt-in reaching Activating. Keep the
fixture inside the native test binary. This cut has no installed Peer option
or CLI.

The original source anchor is 4bb566ecd19c331f5102d7d150fe7d5ddd4fde32.
This amendment rechecks the actual integrated M2 source
d055771fc2668788b7ca639bf0b46b4d8df08026. Follow
[Peer activation](peer-pending-activation.md) and require M2's independent
code approval before wiring intake. M2 owns exact Held insertion, replay
disposition, JoinStarted fencing, native join and readback, Pending deadline
and the shared decision CAS. Keep MDK upstream master and qualify the actual
integrated lock. The inspected lock resolves to
9a29d1e46f11bc8e421046d5a1e20b52b59795df.

Externally enabled operation requires a qualified Peer origin, scoped live
aggregate stage limits, joined expiry/decline cleanup and ordered activation.
Activation must prove exact room history and PostJoin rotation before accepted
room traffic, receipt or telemetry. Ordinary default relays establish no Peer
trust. Admission policy, profile publication, CLI and public rollout require
their own implementation and review.

## Start, own and stop

- daemon::open_inner validates configuration, obtains one ProfileLease, resolves
  IdentityMaterial, preflights, opens the conventional MdkOwner and marks the
  binding ready. Capture the canonical CommandConfig::role before commands
  consume the configuration. After the ordinary owner, Terminal and command
  setup finish borrowing IdentityMaterial, move it into the optional private
  Peer bootstrap. Client and Both are eligible. Server, missing config or
  role, Peer admission Off and missing qualified origin create no Peer state,
  schedule or raw socket. The ordinary socket is unchanged. Peer admission
  Off is distinct from the fixture's transport RouteMode::Off (direct TLS).
  Manual and Automatic are typed fixture bootstrap inputs frozen for that
  daemon instance.
  Keep existing setup, config, frontends and signals compatible. Keep the
  bootstrap type and QualifiedOrigin constructors cfg(test) only in this cut.
  Expose no runtime constructor through JSON, argv or environment variables.
  Mint an immutable Peer mode witness only when the two-account kind-7435
  fixture is created. Bind it to that relay instance, exact origin and
  unordered account pair. Require it to construct the M3 QualifiedOrigin
  and check the resolved daemon account is the intended recipient and the
  other account the fixture founder. The existing shared helper accepts an
  untyped Relay, so keep it outside the M3 authority path. An ordinary relay
  instance, URL, connector or private CA alone proves no Peer mode.
- run_with_startup wraps the profile in an Arc and coordinates Terminal
  ownership. Reuse terminal_owner::Runtime::start's tracked current-thread
  worker pattern. Move the lease Arc and bootstrap into the worker, then borrow
  inside that scope to open one M2 owner. One worker owns the cursor/checkpoint,
  stage namespace and a single current socket. Keep identity material in the
  owner. The existing ordinary owner remains separate. Its startup reporters
  and ordinary command readiness have no Peer relay dependency. Mark failed
  Peer startup unavailable without marking an ordinary healthy owner degraded.
  PreparedConsentOwner::open_custody attaches with lease, identity, role,
  policy, route and Wall, independently of socket connect. Provision a new
  fixture namespace under its sole lease with Paths::initialize, checked
  provisional attach and Prepared Consent epoch 1 before the custody attach.
  Restart at epoch 2 directly with open_custody. An attach error or partial
  or damaged image must never trigger fresh initialization.
- One serial Peer loop, one operation in progress and a separate capacity-four
  IPC channel. At most one request, one due local custody turn and one relay
  turn per fair pass. An outbox attempt with its ACK is itself a relay turn.
  A native turn must finish or establish its durable recovery fence before
  the loop proceeds. Use the cursor's due/backoff cadence and no catch-up
  burst. Use bounded raw-owner connect, normal live poll, one history page or
  one exact send. Distinguish ordinary timeout, relay unavailability, policy
  and owner stop. The separate queue prevents a conventional owner.tick,
  which can await transport for 15 seconds, from owning the Peer IPC path.
  Bound live polls to 250 ms and every relay turn to 3 seconds in this
  loopback cut, well below the 15-second IPC exchange. Keep local state
  fencing for operations that can exceed their request deadline. Keep IPC
  responsive during idle retry without starting concurrent stage
  work. A raw socket outage permits local inventory and adjudication of
  existing custody under the same hardened origin, policy, clock and native
  proof checks. open_custody can attach without a socket, while the existing
  available_sample checks cursor.require_available on authority paths.
  Keep quarantine or ambiguous origin fail-closed. Require a new qualified
  socket for ingress and Consent publication.
- peer_raw_socket_owner::Owner::active retires its generation on future drop.
  Drive wait_live through its normal bounded empty return. Check cancellation
  separately from IPC scheduling. Dropped or ambiguous send/page retires that
  socket and its receipts. After a bounded turn freshen the scope and use M2's
  recovery cursor if needed. Only current-scope exact positive Consent ACK can
  settle. Preserve durable exact attempt/retry. Use M2 typed dispositions and
  the exact raw owner. The legacy InvitationLane approval/publish path does
  not own joined custody.

Use the same preparation flow in both admission modes. A Manual recipient
provisions and sends Consent without legacy Offer approval. It makes its
local decision only after native join. Reuse M2 receive_wss and recover_wss
with the custody owner, where Delivery is a durable disposition and each
exact receipt is required for page advance. The old History::Held is only
an in-memory M1 result. Parameterize the existing recovery turn's ScanStart
to use Recovery on a new socket and Periodic thereafter. Preserve any active
page and the cursor's cadence. Reuse retry_custody_due for exact deferred work.

Disposition returns status and receipt but no Offer selector. Select from
one bounded owner-private due-work projection inside the hardened checkpoint.
Do not re-parse the network. Bind each lookup to existing invitation,
provisioning, outbox or custody rows under the current origin, policy,
qualified wall and stable selector/slot ordering with fair class rotation.
Return at most one eligible item per pass. Exclude custody-owned rows from
preparation and outbox work. Resume reservations with recover_provisioning
and retry the existing outbox. Request advance_custody only for Held or
JoinStarted. Dispatch eligible Automatic Pending through the shared decision
and leave Manual-at-Held Pending for IPC. Lookup coordinates are revalidated
by the exact transitions.
Checkpoint::summaries excludes existing slots and cannot drive recovery.
Never advance a cursor to skip unprocessed durable work.

| Turn | Owner and authority | Clock | Egress |
| --- | --- | --- | --- |
| Connect or recover | QualifiedOrigin opens the exact raw Owner, then uses M2 history/live/deferred admission and Checkpoint::admit for Offer. Select only authenticated admitted durable Offer coordinates. | Qualified cursor wall before and after I/O | NIP-42 and confined kind-7435 subscriptions |
| Stage provision | Checkpoint::reserve_provisioning charges an exact generation and slot. InvitationState::prepare_provisional binds it and Paths::resume_allocation creates/reopens its generation-bound image. Use the existing M2 namespace initializer for the store. Do not allocate an unrelated legacy StageSlot or take a slot from the network. | Fresh provisional_sample, checkpoint wall floor and real leaf wall time | None |
| Consent queue | PreparedConsentOwner::queue_prepared_consent preserves attested native KeyPackage and exact signed bytes in the durable outbox. Start no fresh package when durable work is already queued. | Fresh provisional_sample | None |
| Due Consent attempt | PreparedConsentOwner::publish_consent_wss reserves the exact due Attempt, uses Owner::publish_bytes and settles only the current exact accepted receipt. Its after-settlement callback is a no-op in normal drive. Preserve its exact ACK wake and retry. No transaction across I/O. | Fresh available sample before/after bounded send | Exact stored Consent carrier only |
| Delivery and native join | M2 classifies Delivery, verifies bound Consent and exact positive settlement, commits Held or bounded deferral, then runs one due fenced join/readback turn. Only the current M2 receipt permits cursor advancement. | Fresh qualified wall for durable updates and system wall for native leaf checks | None beyond scoped subscription |
| Existing custody decision | Refresh exact row through M2 and apply the common decision CAS below, or return an inert read outcome. | Existing absolute IPC budget plus fresh qualified wall and exact row deadline | None |

- On signal, ordinary service maximum, listener failure, setup failure or
  serve_owner return, close intake and revoke new work before joining. Stop
  control is independent of queue capacity. Retire a cancelled relay once.
  Give a started M2 native turn time to reach its committed or fenced
  JoinStarted boundary. A CAS begun before stop is a synchronous owner turn
  and finishes; stop gates new turns. Close the owner, sockets and lease
  clone before the durable shutdown report. IPC exchange children hold no
  lease, but closing intake must fence their queued replies and prohibit late
  sends. Explicitly stop accepts and cancel and join the listener's bounded
  child JoinSet. Preserve Terminal's already transferred sockets under its
  own owner. Treat listener abort without confirmed drain as incomplete.
  Both Terminal and Peer release before Arc::try_unwrap. A native operation
  can exceed the reporting budget.
  Record incomplete and keep the lease until it actually joins. Stop all
  created workers even if bind or ordinary startup fails.

## Typed local decision and inventory

Add one closed versioned IPC Request::Peer variant with a separate internal
intake next to Terminal. Reuse ipc::bind's private same-UID directory and
socket, framed JSON, 65,536-byte ceiling and absolute short exchange. In
ipc::serve_inner verify UnixStream::peer_cred against effective UID before
decoding. At run_with_startup create one private pair per profile: worker
control and listener registration share one opaque incarnation with stopped
and registered state, plus a capacity-four queue. Register the bound
listener once before accepting requests. A guard owned by serve_inner retires
the incarnation on return, abort or panic. The worker independently checks
its pair and retirement, including during ordinary owner activation. Its
matching guard closes intake when the worker fails or stops. Do not depend on
the ordinary tick noticing a dead listener. Check registration both when
enqueuing and when adjudicating; a send racing retirement is inert. Poll or
wake the worker on stop outside that queue.

Use the exact same successful peer_cred read to mint a private non-Clone,
non-serializable witness into that paired listener's exchange. Verify the
witness refers to the current pair, profile and request, then take it once.
Do not expose the numeric credential in request JSON. Any conversion to
Checkpoint::authorize_local(u32) is privately inside the paired worker.
Intercept Peer before ipc::serve_inner's ordinary queue. Explicitly exclude
it from Request::is_command, make its deadline independent of that predicate
and fail closed if a Peer variant ever reaches ordinary dispatch.

Version and size-check replies like Response::inventory and
ipc::client_inventory. Add a bounded deterministic List of retained rows
(max 16) inside the hardened checkpoint and exact Show keyed by selector
and generation. Project canonical full server npub, signed Offer ID and
revision, generation, redacted state, local times and expiry.
Checkpoint::read_custody is exact-only. List reports hardened durable
metadata without opening native images and omits Held and JoinStarted.
Show verifies current custody and a fresh read-only closed-stage snapshot
for an unexpired Pending or Activating row. Project absent or mismatched
native evidence as unavailable. Held and JoinStarted yield unavailable on
Show; expiry yields expired before native access. Extract the read-only
path from M2's existing exact proof. advance_custody can mutate, so it
cannot serve inventory or approval retry. A fixture typed IPC client reads
the exact row before sending a local decision.
No remote data acts as user input. Return no room ID, original frame, route
detail, key material, host info or permission view.

Approve via full canonical server npub plus exact Offer ID, revision and
generation. Reject aliases, email, partial keys, duplicate fields and overrides.
M2 activate_custody currently re-reads credentials from Option<&UnixStream>,
rejects Activating and calls its cut only after commit. Keep that raw-socket
wrapper scoped to the existing M2 tests. Extract one owner-local decision
core taking the paired Manual capability or explicit Automatic cause.
Use the same native proof and commit_custody_activating CAS. Recheck current
reply channel, shutdown, listener, role, policy/block, generation, qualified
time and expiry before the actual CAS, including after any native readback. No await between this final check
and the synchronous CAS. Start the short exchange budget at IPC accept
handling and give its write the existing 100 ms margin. Timeout before the
check grants nothing. During or after possible commit return uncertain.
A same-selector and same-generation retry within the qualified row lifetime
reads exact Activating with the same proof as Show and no write, native join
or reapplication. It reports durable state without attributing it to that
request. If the row is still Pending, reapply the full manual admission and
commit checks to the new exchange. Expired, stale or unavailable evidence
provides no authorization. Closing the reply receiver is visible
on exchange timeout or listener abort. Same-direction EOF is normal since
the client half-closes writes. A client disappearing after sending cannot
be distinguished by the waiting exchange, and cannot revoke an authenticated
request by EOF.
Return typed bad shape, forbidden, stale, expired, busy, stopped and uncertain
errors. Do not add another Unix socket or use the legacy approval method.

Automatic uses the same M2 CAS with a private cause and no simulated
listener witness. It requires durable Automatic at Held and eligible current
Automatic mode, role and exact unexpired Pending. A Manual Held row remains
manual under a later Automatic daemon. Both paths leave Activating closed to
acceptance, command/Terminal authority and host details. Joined decline stays
unavailable in this cut pending the qualified exact SelfRemove teardown and
deadline recovery. Hiding an entry does not imply native departure.

## Closed native custody

Reuse the existing closed-room isolation with fresh bound stages.
peer_stage_native::OpenStage::open_provisional already uses isolated SQLCipher
and explicit DELETE journal. Reuse Paths, checked closed image and native
receipt proof, cursor and physical deny-all runtime adapter through M2.
MdkOwner::activate runs ordinary native traffic and maintenance.
terminal_live_enrollment::ReservedRoom is a reaping model with activated
transport. Reusing either account runtime or any existing Command/Terminal
room would bypass the one-time package, Welcome and receipt provenance. Keep
the same isolated stage across Pending and Activating. Avoid copying it into
normal runtime.

Public AccountDeviceRuntime::ingest_delivery can persist history, own-leaf
PostJoin rotation and future KeyPackage obligations. M2 alone uses the public
API with refusing native transport and package adapter, exact embedded
Welcome and the checked original stage. Pending performs no native general
drain, maintenance, subscriptions, fanout replay, application ACK or fetch.
OpenMLS 6100df3 verifies staged Welcome leaf lifetime on system wall time.
Fixture packages use actual wall time. Inject synthetic time only into app
deadline tests. Do not use MDK test support, session_mut, direct SQL to mint
authority or process-wide VFS changes in the daemon. Preserve existing 64 KiB
carriers, class admission/backoff bounds and file/sidecar witness. Closed-byte
sampling is not a live aggregate SQLCipher cap. Use M2's reviewed integrity
checks, and track deployment cap and leave cleanup independently.

## Validation and evidence

1. Add fixture-only bootstrap and distinct owner intake to the common daemon
   entry. Reuse real same-user socket tests patterned after
   client_inventory_ipc_tests. Keep timeout/queue tests deterministic. Include
   Off, Server, absent and unqualified endpoints, an ordinary RelayPolicy URL
   rejected as Peer authority, malformed frame, foreign UID
   predicate only, wrong-profile/retired-listener witness, queue full, a Peer
   frame never entering normal dispatch, no-live-socket inventory, disconnect,
   stale generation and timeout before versus after the final CAS. Verify a
   stuck reservation cannot starve a fresh Offer or a due outbox. Do not
   fabricate an accepted witness from an integer. Prove setup, listener bind
   and stop failure handling with a full queue or active stage/relay turn.
2. Wire M2's reviewed same-account exact cursor, staged owner, decision and
   inventory. Use real socket decisions, elapsed expiry and Manual-to-Automatic
   restart. Restart a daemon from a durable Pending and from possible approval
   reply loss. Show and the same-generation committed retry return Activating
   with exact native readback and no additional join or CAS. Cover metadata-only
   List versus Show with a missing or changed proof, same-generation expiry,
   same-UID approval during a raw-socket-only outage and denial during
   quarantine. Keep reviewed M2 tests independently runnable.
3. Run an ignored real native daemon case on Linux AMD64. Use one private
   cfg(test) input at open_inner: a separately allocated root supplied
   through the normal Config, a policy-matched ordinary test connector,
   the qualified M1 Peer bootstrap and a same-build native layout path.
   The fixture creates config and disposable file identity through ordinary
   profile provisioning, releases any setup lease before daemon start and
   asserts the daemon acquires the only live lease. A cfg(test) stop receiver
   feeds the same shutdown branch in serve_owner as native signals. Never
   override a production connector or credential helper via public inputs.
   The ordinary lane uses the existing trusted Terminal/Command relay mode for
   MdkOwner::activate. The Peer relay uses the exact NIP-42 two-account kind-7435
   mode. Run common run_with_startup and serve_inner internals with a test-owned
   stop signal beside normal signals. Run the Manual and Automatic profiles
   sequentially. Verify authenticated Peer traffic, exact stage and inventory,
   actual IPC approval, Pending restart, absence of native Peer egress and no
   commands, Terminal grants or telemetry sourced from either state. Check
   ordinary readiness on the same local socket. In a second opt-in increment
   break only the Peer relay and prove a normal Command through the ordinary
   daemon IPC and its separate private relay. Set the daemon under test to
   Client and reuse terminal_command_fixture::CommandPeer as conventional
   remote Server with its real worker. Build marmish-native --bins.
   native_layout::resolve rejects the libtest executable, so use a private
   fixture input at open_inner's layout lookup to resolve the same-build
   marmishd, marmish and marmish-worker siblings. Keep production lookup path
   unchanged. Client enable_commands does not open a local engine or resolve
   its helper again. Register both conventional accounts on the relay, grant
   the test client one concrete command and have the real Server counterpart
   activate and tick. Drive Request::CommandPrepare (normal room discovery),
   CommandSubmit and CommandPoll/CommandResponse and inspect CommandInvocations
   through ipc::command_request. Verify literal stdout and exit 0. Assert
   the Peer relay has no ordinary MLS kinds or native 1059 and the conventional
   relay has no kind 7435 traffic. Use bounded fixture counters, not bodies.
   Assert owned socket, accepted exchanges, worker, stage handles and lease
   are closed after stop. No public relay or system trust change.
4. Keep only bounded deterministic core tests in routine CI. Keep private WSS,
   native and restart cases opt-in. Record hardware, platform, exact source/
   MDK, sample count and join, decision, uncertainty/retry and teardown timing.
   Run strict targeted Clippy and scoped formatting. In-process daemon evidence
   qualifies common logic and this kernel only. Installed package, macOS,
   production relay, live storage cap, joined leave and Active semantics need
   independent qualification. Obtain actual Claude Code Auto integrated code
   review before publication.

## Qualification result

The final fixture qualifies one exact pinned founder per daemon instance. The
checkpoint filters that founder before reading due work and the lookup selects
at most one eligible class for the founder. This refines the original
inter-class rotation design for the implemented single-founder scope. Each
pass retains distinct local, IPC and relay opportunities. Inventory omits
expired rows. See [validation](../validation/peer-daemon-pending-owner.md)
and the [independent code review](../reviews/peer-daemon-pending-owner-code.md).
