# Connect the candidate daemon's ordinary service admissions

This slice implements the ordinary startup and service-loop ownership from
[daemon composition](peer-candidate-daemon-composition.md) and
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md). It uses the
existing Linux AMD64 candidate build gate. Keep the registration and physical
cut internal until the constructor, Command startup, Terminal, IPC and active
loop can all use the same ownership boundary. The ordinary stock `daemon::open`
and default release behavior remain as they are. Manual actor and Pending reads
are a follow-on mount after the complete ordinary path is qualified.

## Construct and transfer ownership

- Keep one config selection, prepared material, policy and `ServiceLifetime`.
  Factor the existing preparation into an internal value if needed to keep one
  stock setup path. Only `run_inner` may request the registered path. Decline
  effective `policy.needs_tor()` before preparing a receiver. Build the same
  ordinary connector; create its checked fatal derivative only after registration.
- Preserve the registration failure table in the composition plan. Prepare on the
  original `Arc<ProfileLease>`. Unwrap that same lease on a pre-registration
  exclusion or proved `Untouched`. Treat uncertainty and duplicate registration
  as startup errors. Take the Command fence once, derive `Admission` from it and
  retain the exact verifier and `Fatal`. A later Peer-local decline keeps the
  ordinary registered mode and the same Arc for the daemon lifetime. The registered
  account binding check is read-only and uses the opened binding, never a second
  acquisition or a rewrite of the Ready manifest.
- Use guarded native preflight and the checked account constructor. At each
  later preparation step admit the effect and own returned resources before a
  subsequent check. Use the existing retention and inventory reconciliation.
  Split an admission check between independent durable turns inside Command
  setup, including history, limit configuration, engine binding and recovery.
  Keep one common `enable_commands` body. Resolve worker and helper through the
  same checked native layout. Select the existing `CommandEngine::start_candidate`
  with the same Admission after the recovery snapshot. Once the engine returns,
  attach it to the owned Command service before any final admission or return.
  Carry `ServiceLifetime` as before. Never detach a returned worker on startup
  failure. Check Command and Terminal turn boundaries even on Client profiles
  where there is no ordinary launcher or Terminal supervisor registry.
- Use the reviewed Terminal route from the same fatal-fenced connector and the
  same held profile. Keep the registry creation, worker transfer and join on its
  own checked path. Startup must retain both the owner and any started Terminal
  runtime until their selected disposition can observe the real tasks. If this
  proof is unavailable, the registered branch remains unmounted.

## Local service and disposition

Keep one `serve_owner` and one existing IPC listener. Enable its bounded drain
path for registered service independent of fixtures or whether Peer becomes
available. A checked process-fatal wake participates in the existing selects and
wins against dispatch, activation, tick, room work and ordinary stop. Use exact
admission for synchronous stateful turns and have owner methods check separately
resumed effects where they can span multiple admissions. A canceled async branch
is discarded. An in-flight synchronous body may return; a new effect or reply
needs a new admission. Ensure that Status and Terminal transfers on the listener
cannot use stale authority while the owner is scheduled out. Preserve existing
stock retries, signal ownership, limits and startup/error behavior.

Start each physical stop cut before awaiting any of them. One guard owns the
startup/active owner, lease and worker obligations. If the exact dispatcher
continues to verify, use the existing healthy stop, with guarded resource close
admissions. On fatal, revoke IPC/Terminal intake, cancel and drain the listener,
reap Terminal and the independently reviewed owner physical cuts, and park
untrusted native roots. Do not call graceful deactivation, Journal, SQLCipher
close or runtime-tree removal through a lost dispatcher. If displacement occurs
during ordinary shutdown, transition at the next independent effect into the
remaining physical disposition; already observed obligations stay complete. A
failed or timed-out observation retains ownership and the original lease. Every
startup error path, including after worker creation or listener bind, enters the
same owned disposition. An active candidate may only enter the new startup path
when all the above dependencies are present together.

## Qualification

Use independent disposable Linux processes because dispatcher registration is
process-global. Run stock native daemon behavior and candidate refusal for
Initializing, non-running entry points and effective Tor. Run real qualified
startup with the original Ready manifest and matching native layout, under
foreground and parent-reported startup. Verify one profile lease, one listener,
one report, finite Command success and Terminal byte round-trip over controlled
WSS on the admitted path. Exercise real cuts before native opens, during worker
setup, while activation/dispatch/tick/room work is pending, after a local reply
has queued, and during verified shutdown. Use actual displacement for decisive
fence tests; test-only hooks may synchronize observation with a particular turn.
Require listener/socket and worker proof, retained lease, no stale successful
reply or clean report after fatal, and unchanged native evidence after the last
admitted storage call. Prove safe clean restart after healthy shutdown. Run
stock and candidate strict checks and obtain exact integrated Claude Code Auto
review before publishing or wiring a live actor.
