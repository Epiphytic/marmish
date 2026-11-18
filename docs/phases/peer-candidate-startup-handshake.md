# Candidate recipient startup handshake

This implements the startup handshake in the approved [private candidate mount](peer-candidate-daemon-mount.md) as an independently reviewable dependency. The candidate daemon will use the existing ordinary listener and readiness reporter. This patch supplies the local synchronization types, deterministic tests and no daemon call sites. It does not open the Peer cursor, store or relay. The captured Ready and dispatcher preflight checks, charged Pending bridge, and actual mount retain their existing requirements.

## Ownership and edges

Create one `(Startup, ListenerRegistration, WorkerStart)` tuple per candidate owner generation. Each of these handles is private and non-clone. They share one Tokio watch state. `send_if_modified` serializes transition updates and retains the state even if all observers have gone away.

| Current | Registered edge | OrdinaryReady edge | Stop |
| --- | --- | --- | --- |
| Waiting | Registered | OrdinaryReady | Stopped |
| Registered | Registered | Ready | Stopped |
| OrdinaryReady | Ready | OrdinaryReady | Stopped |
| Ready | Ready | Ready | Stopped |
| Stopped | Stopped | Stopped | Stopped |

There are no supplied generation numbers or external keys. Pair construction and the interior channel are private to native code. No handle expresses profile, transport or durable mutation authority.

- The ordinary startup side calls `Startup::ordinary_ready(&self)` only after the first ordinary Active status publication and successful reporter result. Foreground uses the Active publication alone. Error, limit or signal before Active stops the pair. Mount cleanup must call `Startup::stop` immediately after the scoped daemon result block and before any drain signal, listener abort or await. All exits within that block flow through this cleanup. A drop also stops the pair on earlier exits or unwind.
- The shared listener calls `ListenerRegistration::registered(&self)` after successful bind and exact candidate registration on that listener. The listener retains the handle for its entire lifetime. Failure to register Peer or actual listener exit drops the handle and stops the pair. The integrated mount must handle candidate registration failure by dropping that registration and continuing the ordinary listener with no Peer intake, instead of inheriting the fixture error return. It must also keep the existing ordinary retry and delay on transient candidate accept errors and keep the registration current. Preserve the stock no-pair behavior and existing owned shutdown on an actual listener failure. `ListenerRegistration::readiness` supplies a cloneable observation for intake to recheck before queueing. It carries no edges and enables no worker startup.
- The worker consumes `WorkerStart::ready` exactly once. It awaits both edges or returns closed on stop. Dropping an unconsumed launcher or canceling its await stops the generation, including a thread launch failure. The resulting non-clone `Ready` continues to hold that stop-on-drop behavior for worker exit. It includes `is_current` for synchronous rechecks and `stopped` for a cancellation branch. Stop ordered before the observed Ready prevents startup. A just-observed Ready can be revoked before the first worker action. The actual mount must check it again before consumption and run the current Ready/profile, cursor, quota and physical route rechecks before opening or exposing Peer. The worker and read intake also apply their existing independent ownership/deadline/close rules. An already-running native action remains subject to the owning actor's cancellation and join contract.

Use one isolated module with candidate and test cfg. Leave its candidate source callable when the mount lands. Keep any temporary unused-code suppression at the deliberate unmounted boundary. The stock non-test build skips the module. No wire, credential, Command, Terminal, startup, listener or runtime behavior changes in this patch.

## Verification

Use Tokio tests with bounded or immediate observations for Registered-first, OrdinaryReady-first, duplicates, stop before each edge, stop after Ready, drop of either producer, and independence of two paired generations. Exercise an awaiting worker waking on Ready and on Stop, and confirm that a retained Ready observation becomes noncurrent on listener or worker drop. Tests use no sleep or wall-clock decisions. Run exact candidate unit selection and strict candidate checks. In the integrated mount, exercise `candidate_registration_failure_keeps_ordinary_listener`, `candidate_transient_accept_retries_ordinary_listener` and `candidate_stops_peer_before_listener_drain`. Those checks observe fixed Unavailable, later ordinary Status and Command as applicable, and the explicit stop before the drain starts. Review that mount separately with its real same-UID listener, reporter failure and installed Pending acceptance criteria.
