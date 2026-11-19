# Drain the candidate daemon's existing listener on dispatcher loss

This is the local socket slice of [dispatcher shutdown](peer-candidate-dispatcher-shutdown.md).
It supplies a private listener entry for the future Linux AMD64 registered
branch. The daemon remains unmounted. The stock API and ordinary request
framing, UID checks, one listener and semaphore remain shared.

## Authority and lifetime

The candidate caller supplies a single candidate-only listener argument carrying the original read-only
`Fatal` and optional `peer_read_listener::Offer`, plus the existing drain
receiver. Existing wrappers and fixture entries pass no fatal. Only the future
registered owner can obtain the production fatal observer. No IPC request
sets or repairs the cut. In this phase exercise the new entry using the exact
socket and disposable test latch. The full daemon will exercise actual
dispatcher loss and its physically owned Peer and Terminal workers.

Turn existing test and production entry points into wrappers around the same
listener body. The registered entry uses the same accept/credential/permit
logic. Race the listener's drain receiver (including sender loss) against the fatal
wake. Either works independently, including with no receiver. Check the
sticky latch at the top of the loop and at task admission, without changing
stock select priority. Race the retry delay against that same pinned drain
and break permanently if drain completes. A client still in the backlog
when the listener closes can see EOF or reset. Keep the existing close order: drop the
candidate read registration and listener, cancel task progress, join the
owned tasks, then return. The same fatal path remains armed if the candidate
reader cannot register. The outer owner remains responsible for a stream
already transferred into the Terminal worker. It stops that worker before
waiting for the socket or worker to finish.

For a registered task, check the latch on request admission before dispatch
or forwarding to Terminal. A task already waiting on a framed request or a
queued reply can be canceled by the listener drain. Use the existing generic fatal-fenced stream for every candidate
frame read and write, borrowing the original Tokio socket. Make the shared
frame helpers generic over `AsyncRead` or `AsyncWrite` plus `Unpin` and keep
the same stock types at call sites. The existing
frame-duration bounds and framing bytes stay the same. A read that crossed
the final syscall admission may complete. Recheck the latch after frame read
and before classifying the result, including suppression of an invalid-frame
reply on a fenced error. Check again at queue dispatch. The NIP/Peer read view continues to check its own exact
generation and latch. Use the ordinary listener latch to gate that reply's
socket writes under the same framing helper. No same-UID cached Status or
queued Command response crosses a new write poll after the cut.

A Terminal transfer needs its own immediate guard immediately before moving
its socket into the intake queue. Pass an optional borrowed `Fatal` to the same transfer entry under the
candidate gate. Release the frame borrow before moving the original socket.
Check once before the synchronous `into_std` and `try_send`; no await lies
between them. Use the same write fence for any refusal frame.
After transfer, the already-reviewed Terminal consumer owns the socket and
readiness/fatal checks. A socket sent under admitted synchronous transfer
before the cut can still arrive: it cannot become new Terminal work after
revocation. Test the listener's physical responsibility using the still-owned
socket and document that the joined listener does not prove stopped lifetime
for already-transferred sockets.

## Validation

- Stock and candidate strict builds with existing socket and handshake tests.
- Fresh same-UID sockets for Status, queued ordinary result, incomplete frame,
  and Terminal transfer. Demonstrate a normal operation on the exact registered
  listener and exercise fatal both before listener starts and while it is
  accepting. Hold an owner response pending across fatal, then attempt to send
  success. Establish listener join, failed new connects and no successful
  cached or queued reply admitted after the cut.
- Prove that a Terminal request whose frame completion is attempted after cut
  cannot reach the intake. An already-transferred socket remains exclusively
  owned by the Terminal runtime and is included in the later composition
  acceptance. Track the difference between a pre-cut admitted syscall and a
  suppressed new operation. Exercise healthy explicit drain separately.

Claude Code Auto session `ef56abaa-dc4d-42fe-b5da-ca458b08c8e5` reviewed
`1126708` and approved with the loop, retry, framing, cfg and transfer
clarifications above before implementation.
