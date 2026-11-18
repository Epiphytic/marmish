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
`pending_view::pair` itself, returning `Option<(Offer, Producer)>` and
propagating its actor counter exhaustion. The offer
holds the resulting reader and generation, the existing one-use
`ListenerRegistration`, and the exact registered read-only `Fatal`.
It contains no lease, route, queue, key or storage.

Give the offer the identical `profile.socket_path()` pathname passed to
`ipc::bind`. Consume it in the existing `ipc::serve_with_listener`
and check that `UnixListener::local_addr().as_pathname()` is that path.
This one-time check uses the name as bound. Later path changes in the
same-UID private directory are bounded by that directory and the held
lease, and the comparison does not attest to any later inode. A mismatch or failed check drops the offer while the ordinary listener
continues serving. A validated offer records Registered with the existing
API. A previously Stopped handshake ignores that event and never becomes
Ready, so its reads remain unavailable. OrdinaryReady remains the daemon's
responsibility.

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
requests unchanged. Use
`all(marmish_peer_receiver_candidate, target_os = "linux", target_arch = "x86_64")`
for the offer module, type, constructor, accept-loop parameter and
candidate-only call-site arguments. The build script permits the
candidate cfg only on that target. Other targets build stock and omit the
offer. Keep the existing test-only fixture and test Peer arguments.
Compile the existing drain argument for tests or the Linux AMD64
candidate so the future mount can use it. In this slice the existing
`serve_inner` path passes None for drain where it exists and None for
offer in candidate builds. The direct daemon fixture call and the
`serve_with_peer` test wrapper keep their existing drain/fixture
arguments and pass explicit None for the offer only in candidate tests.
The listener test module alone passes Some. The dedicated no-offer
`PeerRead` reply stays ahead of the ordinary queue arm.

## Qualification

Use the actual same-UID socket and shared accept loop on Linux AMD64.
Exercise wrong path, already Stopped or dropped offer, not Ready, no
view, wrong generation, Show with mismatched selection, stopped
readiness and a fifth concurrent read. Wrong path and dropped offer
stop the view and leave readiness unavailable. Test normal drain and
confirm it stops the view and revokes readiness before joined task
termination, including an accepted task parked at its write boundary. Test a cut between admission
and the pre-write check with a deterministic test seam. Verify that
aborting the serve future stops the view, any PeerRead result leaves
the ordinary request queue untouched, and ordinary Status still works.
Check that a stock build cannot use PeerRead. Add an exact synthetic test view if needed
to verify the positive wire path mechanically. Qualifying the
application's Pending and Empty proof still requires the actual actor
and its real v2 and capped SQLCipher tests, with no installed synthetic
constructor. Run the candidate checks from the intended clean commit, with
the explicit 40-character nonzero Git object ID and the ordinary feature
set. Leave `installed-checks` disabled.

```sh
MARMISH_BUILD_COMMIT="$(git rev-parse HEAD)" \
  CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS="--cfg marmish_peer_receiver_candidate" \
  cargo test --locked -p marmish-native --lib --target x86_64-unknown-linux-gnu
```

Run stock checks with no candidate cfg. Run the compiled build script
directly with `CARGO_CFG_MARMISH_PEER_RECEIVER_CANDIDATE=1`, the same
`MARMISH_BUILD_COMMIT`, and `TARGET=aarch64-unknown-linux-gnu` and
`TARGET=aarch64-apple-darwin` in separate invocations. Both must refuse
before dependency compilation. Run the existing view, readiness and
IPC tests. Obtain Claude Auto code review before publishing this slice.
