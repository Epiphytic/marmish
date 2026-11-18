# Bind the candidate Peer read view to the existing daemon listener

This is the same-UID IPC slice of the
[approved Manual actor mount](peer-candidate-manual-actor-mount.md), based
on `dfeaf94`. It exercises the listener and bounded projection before the
native actor is available. Installed foreground and background entry
points continue to pass no candidate read capability until the final
composition supplies the actual actor. Test-only projected rows do not
qualify an installed Pending or Healthy Empty.

## Ownership and readiness

The daemon composition mints the nonzero listener generation. A
candidate-only, non-Clone constructor accepts the generation and calls
`pending_view::pair` itself, returning `(Offer, Producer)`. The offer
holds the resulting reader and generation, the existing one-use
`ListenerRegistration`, and the exact registered read-only `Fatal`.
It contains no lease, route, queue, key or storage.

Give the offer the identical `profile.socket_path()` pathname passed to
`ipc::bind`. Consume it in the existing `ipc::serve_with_listener`
and check that `UnixListener::local_addr().as_pathname()` is that path.
This one-time check uses the name as bound. Later path changes in the
same-UID private directory are bounded by that directory and the held
lease, and the comparison does not attest to any later inode. A mismatch,
failed check or already Stopped registration drops the offer while the
ordinary listener continues serving. Only a valid listener records
Registered. OrdinaryReady remains the daemon's responsibility.

The offer retains a drop guard before and after consumption. On mismatch,
early exit and future drop it calls `reader.stop()` and drops the
registration to stop readiness. For a served offer, stop both before
dropping the listener and awaiting `tasks.shutdown()`. The installed
candidate must adopt the drain channel and explicitly await each accepted
task set. The existing abort fallback remains stock or a deliberate test
of view-stop-on-drop, which does not prove the joins.

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
timing and liveness without native operations. Pass the same short
reply deadline as the other inline exchanges, `exchange_deadline - 100ms`.
Write under `timeout_at(exchange_deadline)`. Keep the permit until
the write attempt finishes, including the unavailable response on
post-admission refusal.
An already admitted local write can race a later revocation. The
candidate composition must cancel and actually join its socket task
set on shutdown, before releasing ownership. No proof is claimed
about bytes already written before the cut.

Unavailable, invalid, stale, mismatched or rate-limited Peer reads
receive the existing fixed `peer_unavailable` response by that same
short exchange where possible. The existing 64-connection limit drops
the connection before reading any request. There is no forwarding to the ordinary
queue and no direct or refresh request to the actor. Keep the bounded
typed List and Show decoder, fixture-only decision IPC and all other
requests unchanged. Add an offer parameter only under
`cfg(marmish_peer_receiver_candidate)`, independent of the existing
`cfg(test)` inert listener fixture. The current `serve_inner` and
daemon calls pass None in candidate builds until installed composition.
Stock has no offer parameter. The dedicated no-offer `PeerRead` reply
stays ahead of the ordinary queue arm.

## Qualification

Use the actual same-UID socket and shared accept loop on Linux AMD64.
Exercise wrong path, already Stopped or dropped offer, not Ready, no
view, wrong generation, Show with mismatched selection, stopped
readiness and a fifth concurrent read. Test a cut between admission
and the pre-write check with a deterministic test seam. Verify that
aborting the serve future stops the view, any PeerRead result leaves
the ordinary request queue untouched, and ordinary Status still works.
Check that a stock build cannot use PeerRead. Add an exact synthetic test view if needed
to verify the positive wire path mechanically. Qualifying the
application's Pending and Empty proof still requires the actual actor
and its real v2 and capped SQLCipher tests, with no installed synthetic
constructor. Confirm candidate and stock source checks and run the
existing view, readiness and IPC tests. Obtain Claude Auto code review
before publishing this slice.
