# Private Linux AMD64 recipient daemon mount

This phase composes the [installed recipient](peer-installed-recipient-daemon.md)
and [candidate read protocol](peer-candidate-read-surface.md) after the separately
qualified candidate manifest boundary. The source inventory is from
`b05cb51` plus the reviewed packaging source ending at `b9ce82c`.
The deliverable is a privately installed AMD64 candidate that reaches an
application-inert native Pending through an actual v2 exchange and offers local
List and Show on the daemon's existing socket. An unavailable Peer path leaves
ordinary Command operational in the same running daemon. This plan changes no
released startup or transport behavior.

## Exact prerequisites and boundaries

- Use the existing explicit candidate cfg, clean build commit, three-executable
  check and private manifest/install. The cfg selects candidate code only. It
  supplies neither runtime admission nor operator or socket authority.
- The current `daemon.rs::open_inner` crosses its first native open at
  `native_preflight::check`; the only existing Peer start in `run_inner`
  depends on test-only `PeerFixture`. The fixture
  `peer_daemon_worker.rs::Runtime` and `peer_daemon_decision.rs` use the
  legacy custody owner and include write operations. Keep that bootstrap and
  its request types test-only. Reuse its thread/stop and listener handshake
  mechanics, with candidate-only read messages and the installed actor.
- The composed cursor now has real Consent settlement and authenticated v2
  Delivery ingest; confirmed Delivery is not itself a Pending result.
  `peer_capped_join.rs::CappedJoin` still uses fixture `ProfileSlots`,
  including an externally supplied fixture charge. The charged
  `peer_receiver_inventory` paths supply exact lease-bound recovery and clean
  measurements, not a Pending proof. The
  [capped Pending bridge](peer-installed-capped-pending-bridge.md) must first
  connect the settled ancestry to durable Held, inventory-derived snapshot and
  original file turns, one consumed JoinStarted, exact native join/readback,
  and a same-generation Pending CAS. Use its fresh cold readback of encrypted
  files and its observed restart results as the admission prerequisite. A row
  written by a synthetic initializer, a join from a different schema, a
  fixture scalar or a remembered capability cannot satisfy it.
- Start the shared listener/read work in parallel with that bridge if useful.
  Run its negative tests while Peer remains unavailable. Finish the mount and
  positive qualification as one composed code review. Keep a single installed
  actor with the sole existing profile lease, no second listener, signal owner,
  detached worker, or independent startup phase. The first mount remains Manual
  and Client or Both only, with the previously approved Unknown cap. No local
  approval, decline or arbitrary write RPC; no automatic acceptance, active
  application privilege, Group access, heartbeat, or plaintext profile data.

## Startup on one profile

1. In the actual daemon run path (no candidate side effect in
   `daemon::open`, doctor or setup), obtain the existing exclusive
   `ProfileLease` and selected material and check the existing Command config
   as today. Freeze the prepared lease in the Arc that will be used by the
   ordinary owner, listener lifetime and worker. Split the already reviewed
   Ready file/identity capture and qualification half from the physical Peer
   route bind. Capture pre-existing Ready, checked recipient/account/device,
   exact manifest/config descriptors and contents and exact private origin
   while local. Initializing completes ordinary startup with Peer unavailable.
   Gate missing, Off, Server, Auto, disallowed routes and current manifest,
   qualification, CA or profile mismatches before registration; these cannot
   open the Peer store or call out. Reuse the descriptor-backed issuer,
   qualification and route checks. An unsupported or failed local check yields
   the same closed local classification and leaves ordinary setup unchanged.
2. Only for the checked candidate, reserve the reviewed private members and
   install the existing SQLite dispatch registration before
   `native_preflight::check`, stock MDK open and Terminal/Command opens.
   Preserve default forwarding for the ordinary stores. A refusal before
   registration leaves Command usable. Once process default mutation begins,
   an uncertain registration or forwarding invariant failure fails startup
   and does not attempt to run ordinary stores through unverified dispatch.
   After verified registration and ordinary open, validate the ordinary opened
   binding independently. Preserve its original fatal error; for a qualifying
   Ready path compare the existing manifest read-only, including its inode,
   rather than rewriting it via `mark_ready`. Post-open Peer scope drift
   refuses Peer and never requalifies in that process. It does not change
   ordinary ownership.
3. Keep local socket bind and ordinary `owner.activate`, status and
   `StartupReporter::ready` in `serve_owner`. Do not start the Peer cursor,
   stage opens or relay before both the exact listener registration and first
   ordinary Active/status reporter success. The existing reporter is absent
   for foreground and successful Active publication suffices in that case.
   A tiny terminal-state handshake combines independent Registered and
   OrdinaryReady edges from the same runtime generation, allowing either order
   and firing at most once. Stopped dominates later edges. Peer registration
   failure marks only Peer stopped and keeps the ordinary listener. A
   transient accept error retains the ordinary retry diagnostic and delay.
   Treat actual listener exit as an ordinary daemon error. The worker does
   not block Active reporting or ordinary dispatch, including when Peer
   qualification later fails.
4. When released, on the sole candidate actor, recheck exact Ready/profile,
   qualify and recover charged inventory, open the existing capped stock
   cursor and bind the typed physical route against the same lease before any
   Peer request. Use the checked recipient turn, typed Consent permits and
   actual native charged bridge. Verify cold Reserved/Held/JoinStarted/Pending
   before admitting new work, applying the approved recovery matrix. On an
   ambiguous join, recover by exact readback only, with charge retained and
   no repeat ingest. Peer can be locally unavailable for missing owner,
   startup, scope drift, failed quota recovery or closed transport; preserve
   ordinary Command and Terminal status/limits. A branch that can prove exact
   local Pending after transport loss may report it only if the owner and
   its local scope remain valid. A connection alone proves no custody.
5. On SIGTERM, Ctrl-C, Command limits, startup error or listener exit, close
   Peer intake and cancel the actor/route before draining the existing owning
   listener and joining workers. Stop is permanent for that generation.
   Retain the lease and its native handles until actual stop completion;
   reuse the owned-stop failure reporting. On reconnect or restart, stale
   listener, socket or owner generations have no authority. Stock path with
   no pair retains current listener shutdown behavior.

## Candidate local projection

Use candidate-only Clap verbs `marmish servers list` and
`marmish servers show --server NPUB --offer-id HEX --revision HEX --generation N`
and the existing exact socket/profile selection. Keep explicit remote host,
alias and rsync forms as specified in the read-protocol design. Marshal a
distinct closed candidate `ipc::Request::Peer` and read reply. The test-only
fixture's decision types stay separate even when testing candidate cfg.
Reject malformed forms and unknown fields or variants before actor queueing.
Reuse `ipc::serve_with_peer` on the one ordinary listener, the same real
peer credentials, accept-time frame deadline, client semaphore and Peer queue
bound. Refuse before queueing while not Ready; recheck UID, worker owner and
generation, deadline and response liveness after queueing and before reply.
A stopped actor or full queue follows fixed short local categories. Never
redirect to ordinary command dispatch or a remote send.

Serve both reads on the single installed actor. A successful proof may be
held as private actor memory only while its exact durable Pending row, owner,
charged scope, deadline and native joined image remain current. Seed it only
from the already required Pending readback or freshly checked cold recovery.
Before returning it, check current local scope, exact row/class/generation,
image identity and immutable proved file under the approved charge checks,
and current wall against durable floor and Pending deadline. Fail closed if
cheap read-only validation cannot establish this; permit the actor's scheduled
capped readback to reestablish proof. A read request cannot run a native join,
cause recovery, change durable timers, bind a socket or mint a manual
capability. Do not translate the test worker's `Service::show` or
`daemon_inventory` directly: they accept broader custody states and can open
native state through the fixture. Apply exact Show equality and project at most
16 live Pending entries as selector, state, held time and deadline. Omit
invalid/expired List rows and return Unavailable for an unavailable owner;
return Unavailable for all non-Pending, expired, stale, wrong-generation,
unbound or uncertain Shows. Publish no artifact paths, relay coordinates,
hidden state or debug reasons. No daemon listener request accepts an authority
token from the client.

## Verification and composition

- Use deterministic tests for pre-ready Registered/Ready event order,
  stop-before/after-registration, invalid/foreign UID, malformed/raw write
  frames, queue full and deadline, exact selector, stale/expired/clock rollback
  and replaced inode, projection omissions, accept retry and stopped
  listener. Add candidate no-write type and default/installed-checks CLI
  absence checks. Keep stock IPC/Command/Terminal regressions core; run the
  opt-in compose suite after exact packaging and on authorized self-hosted
  Linux AMD64 only. Preserve the measured capped-Pending bridge prerequisites
  and its negative matrix; reuse its existing cut/oracle where it actually
  observes the installed call path, and mark a missed cut inconclusive.
- Privately package and install the exact non-test source in short disposable
  paths with actual CLI/daemon/worker. Start the installed daemon with genuine
  file/keychain material and system clock. Use a separate founder process,
  independent profile/credentials and actual MDK for the v2 private TLS
  relay. Qualify the recipient via the existing installed command and real
  issuer evidence. Observe its native one-time KeyPackage consumption and
  same-generation Pending, bound List/Show, exact Delivery replay with no
  second native effect, and clean shutdown/restart with fresh cold checked
  Pending and no resumed approval. Exercise durable Held and post-join
  pre-Pending restart using the approved external cut and one-poll counters.
  Check a fresh wrong/expired Show and a raw rejected write through the
  actual socket. Report timing, bounds and zero forbidden egress with no
  raw content or session keys.
- Run a harmless real Command to the same installed candidate PID on
  independent authorized ordinary accounts, with literal hostile-looking
  argv, exact stdout/stderr and exit, while one Peer readiness/route failure
  keeps List/Show unavailable. Verify profile/ordinary socket and executable
  identity and that no daemon restart changed the measured outcome. Repeat
  through a qualified Pending interval as required by the parent, along with
  its separate Terminal check. The ordinary connector uses compiled WebPKI
  roots (`transport.rs::tls_stream`); a private fixture root, OS trust
  store override, transport injection or direct plaintext loopback cannot
  qualify stock Command. Configure the existing ordinary relay independently
  of the Peer URL. Use an already authorized, stock-trusted explicit ordinary
  relay with disposable accounts and bounded traffic, or an existing
  authorized stock-trusted private WSS if available. Keep the Peer controls
  on the private qualified relay only. If a trusted ordinary route is
  unavailable, report the same-daemon Command qualification pending;
  local Status or a test fixture are separate evidence.
- Keep code ownership to the single daemon startup/worker composition, the
  candidate CLI and read-only IPC additions, and integration tests. Reuse
  charged bridge and transport ports via their sealed types. No release
  distribution or hosted runner dispatch follows this step. Record actual
  compile/installed paths, private relay scope and remaining platform limits,
  and obtain Claude Code Auto review of the composed code.
