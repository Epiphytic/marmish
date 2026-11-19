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
  successful open, unbundle the owned receiver and material exactly once
  for the actor. Keep the existing Command admission, Fatal and lease in
  the ordinary owner throughout. Never derive trust from a listener or from
  the background reporter.
- Specify an explicit private invocation intent at the gated example's
  boundary. The current plain qualification must still exercise registered
  Command without any Peer traffic. The second intent is available only to
  a validated private invocation; it cannot be selected from profile
  settings, the installed binaries, IPC workers or stock environment.
  Validate the intent before config and authorization. Both private
  invocations retain the signed installed-layout, exact prepared-profile
  and distinct qualified Peer/ordinary route requirements.
- Create a single existing readiness pair for the existing listener path,
  using the exact captured socket path. Carry the `ListenerRegistration`
  with the candidate listener, check the actual kernel local address and
  register once. Drop it before final listener task drain or on local
  failure. This passive increment needs no view Producer. The follow-up
  can use the existing `peer_read_listener::Offer` to carry registration
  together with the exact generation's memory projection. Avoid allocating
  an unused projection or adding a second listener. The socket reports
  `peer_unavailable` for read requests throughout this increment.
- Reuse `Actor::spawn`, its idle `WorkerStart` and current-thread runtime.
  A route/actor-local spawn or startup failure leaves ordinary Command
  available and removes the passive generation. Do not retry or reacquire.
  Treat loss of the verification of the exact registered dispatcher as
  global. An ordinary F or unacknowledged startup always revokes the
  pending actor and never admits Peer work.
- On the parent path record the ordinary readiness edge only after C,
  admitted ordinary Active publication and a fresh stop/listener/
  lifetime/fatal check. For foreground use the same post-publication
  check without an inherited report. Do not recalculate the second edge's
  absolute startup budget. Active Command work remains independently
  available on its ordinary route. No listener operation provides group
  admission or chooses a Peer route.

## Shutdown

When ordinary service leaves its event loop, stop the readiness pair and
the actor before initiating normal listener drain or Command owner
quiescence. Observe actor join on the existing bounded wait iterations.
The passive actor retains the existing route revoke and physical reap path,
including a stop while opening. An already joined local failure does
not block normal Command cleanup. Never treat a timed-out, canceled or
panicked join as physical evidence. Verify accepted worker exits and
their proof obligations against the exact `Actor` implementation before
wiring the loop. A global dispatcher fault cuts both connection paths,
withholds further normal status/replies and retains the profile through
the actual physical shutdown decision. An incomplete physical stop
retains both custodians and the original lease. Do not clear reservation
state or report graceful exit on an unproved stop.

## Validation

- Exercise the actual fresh private example, validated installed assets,
  native SQLCipher and real qualified direct WSS. Confirm ordinary relay
  publication and the lack of post-qualification Peer connection before
  acknowledgment; after admitted C require a new authenticated,
  process-owned Peer subscription. Keep those counts distinct from the
  four fixture qualification connections.
- While the passive socket is active, run a real finite ordinary Command
  with exact bytes and verify the socket stays `peer_unavailable`. Verify
  it stays available to Command if the passive route closes locally.
- Cover a refusal with no acknowledged readiness, stop during physical
  route opening and normal stop of an opened Peer route. Assert both
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
