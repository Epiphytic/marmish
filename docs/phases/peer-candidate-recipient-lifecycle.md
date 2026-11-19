# Compose the registered Manual recipient lifetime

This increment applies the ownership portion of the approved
[Manual actor mount](peer-candidate-manual-actor-mount.md) to the registered
Command daemon qualified at `dc5a1d7`. It exercises the existing passive
`peer_installed_recipient::Actor` in a private Linux AMD64 process.
Keep installed activation deferred until the registered startup and
shutdown acceptance checks pass. This is a prerequisite for the managed
Offer, Consent, Delivery and Pending service, which adds protocol and
projection behavior in the next mount.

## Ownership and activation

- Reuse the original `IdentityMaterial`, `Arc<ProfileLease>`,
  `RegisteredReceiver` and registry-issued `Fatal`. The ordinary Command
  setup borrows material before this transfer. Keep the receiver available
  through ordinary owner construction and its initial checks. After
  successful open, an optional startup payload in `RegisteredOwner` holds
  the owned receiver and original material until `Actor::spawn` takes them
  exactly once. Without this intent, drop material at the existing point.
  A thread spawn failure drops the moved payload before any attach or
  route open. Keep the existing Command admission, Fatal and lease in
  the ordinary owner throughout. Never derive trust from a listener or from
  the background reporter.
- Specify an explicit private invocation intent at the gated example's
  boundary. The current plain qualification must still exercise registered
  Command without any Peer traffic. The second intent is available only to
  a validated private invocation; it cannot be selected from profile
  settings, the installed binaries, IPC workers or stock environment.
  Validate the intent before config and authorization. Both private
  invocations retain the digest-pinned installed layout, exact prepared profile
  and distinct qualified Peer/ordinary route requirements.
- Create a single existing readiness pair for the existing listener path,
  using the exact captured socket path. Add an explicit registration-only
  mode of `ipc::CandidateListener`. It carries the captured socket path
  and `ListenerRegistration`, with no read view. The existing listener
  checks the actual kernel local address and calls `registered()` once.
  Hold registration alongside the existing Offer until the listener
  starts draining, using mutually exclusive states for the two modes.
  Drop the registration before client-task drain or on local failure.
  The read branch continues to report `peer_unavailable` when there is
  no Offer intake. The follow-up replaces this mode with the existing
  `peer_read_listener::Offer`, which also carries the memory projection.
  Use the existing listener, queue, UID checks and reply fence.
- Reuse `Actor::spawn`, its idle `WorkerStart` and current-thread runtime.
  A route/actor-local spawn or startup failure leaves ordinary Command
  available and removes the passive generation. Do not retry or reacquire.
  Treat loss of the verification of the exact registered dispatcher as
  global. An ordinary F or unacknowledged startup always revokes the
  pending actor and never admits Peer work.
- On the parent path record the ordinary readiness edge only after
  accepting the parent's A, writing C, admitted ordinary Active
  publication and a fresh stop/listener/
  lifetime/fatal check. For foreground use the same post-publication
  check without an inherited report. Do not recalculate the second edge's
  absolute startup budget. Active Command work remains independently
  available on its ordinary route. No listener operation provides group
  admission or chooses a Peer route.

## Shutdown

When ordinary service leaves its event loop, stop the readiness pair and
the actor before initiating normal listener drain or Command owner
quiescence. Keep the exact actor in ordinary shutdown custody and observe
its join on the existing bounded wait iterations. Its Drop cancels but
detaches, so the finish path cannot rely on dropping the handle.
Keep the existing route revoke and physical reap, including a stop while
opening. Tag the worker result with an independent physical disposition.
A runtime-build failure before attach proves no start. A caught panic in
`hold_port` can prove physical end after the paired revoke and reap return.
A panic before the sealed proof, including one in attach or in the reap
itself, and an error joining the thread leave physical end unproved.
A join timeout or canceled observer retains the owned handle. A joined
local failure permits ordinary cleanup only with a proved physical
disposition. The Command raw-slot barrier does not substitute for the
actor's exact drain witness.

The exact forwarding recheck still runs at the final worker edge.
A registry-turn panic trips the shared Fatal and must never be reduced
to local unavailability. A global dispatcher fault cuts both connection
paths, withholds normal status and replies, and retains the original lease
through the stop decision. Do not let classifying an exit as global mask
an unproved physical result. Gate `stop.complete` and `cleanup_runtime`
on actually joining the actor with proof. On unproved physical end, park
with both custodians and the original lease under the existing
unproved-stop policy.

## Validation

- Exercise the actual fresh private example, validated installed assets,
  native SQLCipher and real qualified direct WSS. Confirm ordinary relay
  publication and the lack of post-qualification Peer connection before
  acknowledgment. Hold the real inherited channel at R before deliberately
  sending A. Compare the relay accept count with its four completed
  qualification connections, then require a new authenticated
  recipient subscription after admitted C. Isolate this private recipient
  identity so the observation cannot be attributed to the launcher.
- While the passive socket is active, run a real finite ordinary Command
  with exact bytes and verify the socket stays `peer_unavailable`. Verify
  it stays available to Command if the passive route closes locally.
- Cover an invalid kernel-path registration, a refusal with no acknowledged
  readiness, stop during physical route opening and normal stop of an opened
  Peer route. With test-only panic seams, prove distinctions between a
  pre-proof panic, a hold-port panic followed by reap, and a registry-turn
  trip. An unavailable idle route or unsolicited application record ends
  the passive generation while Command stays available. Assert both
  registered socket halves drained, original lease retained during exit
  and trusted cold stock reopen after healthy completion. Reuse the
  existing actual displacement selectors for the ordinary failure cuts.
  Include one composed global loss with a real Peer connection in the
  installed-layout process once the approved loss fixture lands.
- Check that neither relay sees a Peer application publication. Test the
  old qualification's continued absence of Peer traffic and the stock
  launcher behavior. Run strict stock and candidate Clippy and report
  lifecycle timings with explicit sample sizes. Keep live proof opt-in
  and the private example out of packages and default CI.
