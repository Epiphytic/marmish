# Bind the candidate Peer read view to the existing daemon listener

This is the same-UID IPC slice of the
[approved Manual actor mount](peer-candidate-manual-actor-mount.md), based
on `dfeaf94`. It exercises the listener and bounded projection before the
native actor is available. Installed foreground and background entry
points continue to pass no candidate read capability until the final
composition supplies the actual actor. Test-only projected rows do not
qualify an installed Pending or Healthy Empty.

## Ownership and readiness

Add a candidate-only, non-Clone listener offer containing the expected
socket path from the held lease, the existing one-use
`ListenerRegistration`, the same generation provided to
`pending_view::pair`, the resulting `Reader`, and the exact registered
read-only `Fatal`. The daemon composition is the only intended production
constructor. The offer contains no lease, route, queue, key or storage.
Consume it into the existing `ipc::serve_with_listener` and check
`UnixListener::local_addr` against the expected pathname. A mismatch,
failed check or retired offer stops the view and drops the readiness
edge, while the ordinary listener continues serving. There is no retry
with a changed generation. Only after validation can this listener
record Registered. OrdinaryReady remains the daemon's responsibility.
The listener registration survives for exactly the owning accept loop
and stops the handshake and view before its client JoinSet drains.

Read task intake can clone the immutable capability. It carries the
view reader, its expected generation, the revocable readiness view
from the registration, and the input-only fatal observer. Dropping one
intake cannot close shared authority. Do not treat the presence or
absence of this optional read registration as ordinary fatal state.
The installed composition separately owns dispatcher fatal and its
accept/ordinary-service gate, including if Peer refuses locally.

## Serve read requests in the existing socket task

Reuse the normal same-UID predicate, global 64 accepted-connection
budget and one accept-time exchange deadline. Intercept only
`Request::PeerRead` in the accepted socket task after the existing
checks. If exact readiness is current and the fatal is healthy, try
`Reader::admit` for the same generation and hold its four-slot permit
through a single bounded write. Recheck the same readiness and fatal
immediately before asking `Read::reply_before_write` for the bounded
projection. That method verifies the view stamp, matched selection,
timing and liveness without native operations. Keep the permit until
the write attempt finishes, even if a post-admission check fails.
An already admitted local write can race a later revocation. The
candidate composition must cancel and actually join its socket task
set on shutdown, before releasing ownership. No proof is claimed
about bytes already written before the cut.

Unavailable, invalid, stale, mismatched or rate-limited Peer reads
receive the existing fixed `peer_unavailable` response by that same
short exchange where possible. There is no forwarding to the ordinary
queue and no direct or refresh request to the actor. Keep the bounded
typed List and Show decoder, fixture-only decision IPC and all other
requests unchanged. Keep stock compilation free of the new capability.

## Qualification

Use the actual same-UID socket and shared accept loop on Linux AMD64.
Exercise wrong path, not Ready, no view, wrong generation, stopped
readiness, tripped fatal, occupied four-permit budget and unconditional
ordinary Status fallback. Add an exact synthetic test view if needed
to verify the positive wire path mechanically. Qualifying the
application's Pending and Empty proof still requires the actual actor
and its real v2 and capped SQLCipher tests, with no installed synthetic
constructor. Confirm candidate and stock source checks and run the
existing view, readiness and IPC tests. Obtain Claude Auto code review
before publishing this slice.
