# Fatal fence for the candidate raw Peer socket

Base `9344290` contains the registered physical route handoff. This
increment supplies a raw transport fence and a drain witness required by
the [Manual actor phase](peer-candidate-manual-actor.md) and the
[fatal shutdown design](peer-candidate-dispatcher-shutdown.md). Scope is
Linux AMD64 tests and the explicit receiver candidate build. The daemon
mount, stock relay construction, probes, unregistered test admission and
Tor behavior keep their existing paths.

## Admission and latch provenance

`ReceiverRouteAdmission::registered(RegisteredRoute, Fatal)` returns the
single-use admission and a `PeerSocketDrain` for that one direct relay.
`RegisteredRoute` supplies the exact route and signer. The mandatory
registered arm owns its signal and actual raw slot permit. An unfenced arm
remains available only via the existing test-only fixture. There is no
unfenced registered constructor, connector escape or signal replacement.

`Fatal` is an existing observation primitive. This base has no process
registry issuer for it. Limit `Trip::new` to tests in this increment if
that issuer has yet to land, and use exact private test latches to
qualify the mechanics. Before mounting the Manual actor, the registry
increment must mint the production signal, trip it under the guard on
global loss including poison, and keep Peer-local loss separate. The
mount must use the signal from its exact registered forwarding observer
for Command, Terminal and Peer and recheck the global registration even
during idle turns. No runtime caller may create a substitute healthy
latch. This source increment neither creates nor qualifies that issuer.

## Carry the one real slot below TLS

The registered constructor synchronously acquires the sole owned permit
from a fresh one-slot semaphore before handing out either end. There is
no socket or network setup at this point. The drain holds the semaphore
and revocation observers. The admission takes the linear permit into
`PeerLiveDial` and then into `PeerRelayConnector::open_live`. Use this
very semaphore as the registered connector's only route slot. Both
ordinary acquisition and this pre-acquired path enter the same existing
TCP/poll implementation after acquisition. The registered path never
invokes ordinary lazy acquisition. The connector and future remain
private to this one open and cannot reconnect.

Check the latch at the start of `ReceiverRouteAdmission::open` before
route recheck or transport setup. Install it with existing
`RelayConnector::with_fatal` before bootstrap and direct TLS. Reuse the
biased fatal race in the connection phases, then recheck before issuing
the live route. An OS poll already admitted before the cut may complete.
No later application authority follows.

Reuse the existing `FencedStream` below TLS. It fences raw read, write,
vectored write, flush and shutdown and wakes pending polls on fatal.
This also fences TLS alerts and implicit tungstenite Pong or Close
driven from a read. Add registered-only slot ownership there. The raw
owner declares the actual I/O field before its permit reference, so the
socket is destroyed before that reference drops even in error or
cancellation unwind. Carry the existing permit references in both
WebSocket halves. Stock and Terminal paths keep their current permit
behavior. The registered drain relies on actual I/O and split ownership
and uses no parallel resource counter.

## Fail closed before checked use

Retain the exact latch on the registered `LivePhysicalRoute`. Check it
first in `recheck`. Both `observe` and `check_use` must start with
that check before profile/window use. Observing fatal retires the route
via the existing shared profile cancellation on route failure. The
drain itself cannot cancel it. Keep checked readiness after the exact
AUTH acknowledgment and live EOSE in `Owner::open_socket`.
`Owner::open_checked` already normalizes open errors to
`Failure::Retired`. The actor identifies fatal from its latch and
global registration check, and does not infer fatal from this error.

In `Socket::fenced_write` and `Socket::text`, add the same fatal wake
to the existing biased profile-cancellation races. Give the Ping flush
in `Socket::inspect` that wake too. Test/ordinary paths with no
registered fatal retain the usual behavior. Keep the per-poll route
refresh in the existing fenced sink and the pre- and post-read refresh
in `text`. `Owner::predrain` may queue one frame from an already
admitted raw poll concurrent with the cut. It confers no checked
authority. `PeerRecipientPort` rechecks before returning an observation
or invoking the synchronous uses of a retained page, live record or ACK,
and before sending any CLOSE. The existing active-operation guard drops
interrupted or failed sockets. A publication already possibly exposed
keeps that classification.

## Prove physical ownership ended

The drain has no guard mutation, semaphore close, readiness, cancel, open
or shut-down method. It refuses a wait while both revocation observers
are healthy, leaving the route usable. After fatal or a route
cancellation it waits for the real slot under the caller's existing
absolute deadline. Keep the slot held by admission, dial and connection
until that owner has dropped all raw and split ownership. Unopened
admission fields release the route first so cancellation precedes slot
release. The proof still applies if a readiness select dropped the
opening future or `open_checked` returned an error. Because the exact
open has no means to acquire the slot again, no later network activity
can invalidate a successful proof.

The actor drops the port or opening future on fatal. It does not invoke
`PeerRecipientPort::stop`, send Nostr CLOSE or a WebSocket close, or
perform MDK deactivation, native retirement or journal work. Its drain
witness can corroborate ownership discharge under the same shutdown
deadline. Route cancellation alone and a slot wait timeout are
incomplete. A retry can wait on the same proof without buying another
open or modifying either observer. The enclosing fatal phase owns
profile retention, parked storage and final daemon status.

## Validation and exit

Extend the fresh-process registered qualification harness with bounded
private WSS cases. Keep new live cases opt-in. Add only exact test
observations to the relay: active runtime recipient sockets, terminal
reason (WebSocket Close versus EOF/reset), Nostr CLOSE count and accepts.
For no-dial also observe the exact connector's TCP attempt count at the
call site through test-only access to the registered pair, and allow a
bounded settle before asserting accept count. Fixture teardown is
outside the assertions about the target connection.

- Hold startup after actual NIP-42 AUTH using missing exact ACK. Cut
  only after AUTH is recorded. Require prompt refusal and no runtime REQ,
  publication or close. The preissued drain survives the error, and the
  relay observes EOF/reset after drain within a bounded wait. Separately
  exercise a dropped incomplete open and a healthy malformed live EOSE.
- Cut an established port during a pending idle read. Require prompt
  retirement, no observation, publication or close, and the same physical
  drain and remote evidence. Include a subsequent `stop` on a freshly
  tripped live port and show it sends neither kind of close.
- Prove the buffered authority boundary apart from raw I/O. Retain an
  actual checked page, live record and accepted ACK from separate healthy
  session cases, trip before checked use, and require no callback.
  Arrange two actual live frames and establish one remains in the
  owner's bounded live reserve before the cut. A post-cut `wait_live`
  refuses despite that reserve. Use targeted deterministic tests for
  each accessor as needed. Make its first refusal independent of a
  previous call retiring the same owner. Test-only poll counters or an
  already-decoded reserve prove these refusals do not depend on the raw
  fence and cannot issue a new request.
- Trip before opening registered admission and require zero local TCP
  attempts, no fifth relay accept after a bounded settle and no runtime
  AUTH, REQ or publication. Holding the unopened but revoked admission
  prevents drain. Dropping it completes drain without dialing.
- With a genuine TLS/WebSocket split, retain each half in turn after
  dropping the other and revoking the route. Require bounded timeout
  until the survivor drops, then drain followed by bounded observation
  of remote EOF/reset. Add a deterministic drop-recording I/O stub
  beneath the registered wrapper that asserts the permit remains owned
  during its destructor. Verify healthy drain refusal leaves a live port
  usable. Any split-only examination stays in the internal test aperture.
- Run existing duplex fence, registered route, physical TLS and
  forwarding bracket regressions. Run strict stock and non-test
  candidate compile/Clippy checks. The latter must compile mandatory
  registered admission with test-only issuance and fixture admission
  absent.

Default enablement, public relay access, heartbeat, custody, native
terminal retirement and cross-platform qualification are out of scope.
