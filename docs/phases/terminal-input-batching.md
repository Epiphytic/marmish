# Configurable interactive keyboard batching

Add `marmish terminal DESTINATION --input-batch-ms N`, with a default of 8
milliseconds and an inclusive range from 0 to 50. Zero sends eligible input
as soon as local credit and the connection permit. Batching affects the
foreground keyboard queue for an admitted writer. It needs no Nostr/MLS,
Terminal frame or local handshake change, and applies to all writer attach
modes and resumed foreground sockets. Observe has no remote input.

Start one absolute deadline on the first nonempty eligible keyboard queue,
after existing detach and bracketed-paste recognition. Permit an earlier
flush when one complete input record is queued. Keep the deadline fixed while
the same queue remains nonempty. A past deadline must not cause busy-polling
while the channel waits for credit. Reset when byte progress drains the queue.
Use the current queue, byte limit, IPC framing, signal ownership, record
credit, stall deadline and poll loop. Never delay the protocol controls that
share a writable socket turn with input. Continue handling output and
signals during a batch. Detach discards the existing framed queue and any
batch deadline. No timing-privacy or traffic-padding claim.

Use default-on deterministic clock tests for first-byte delay, fixed expiry,
record-size flush, zero delay, partial send followed by no extra batch delay,
queue drain and deadline under credit stalls. Exercise the normal terminal
CLI parser and fixture foreground IPC, including delayed Input, intact bytes,
control progress, and detach dropping withheld bytes. Keep opt-in latency
benchmarks outside the hosted core suite and identify the configured delay.

## R2 queue and poll contract

Use `Option<u64>` parsed in the Terminal subcommand with Clap's 0..=50 range,
so explicit Observe can reject the option. Resolve the default in the CLI
and pass it through foreground entry points. If a default invocation becomes
an observer through the Busy prompt, the setting remains local and inert for
that observer. Pre-admission prompts are unaffected. Observe and unframed
unit-test bridges keep their existing flow.

The fixed timer is owned by a small pure helper sampled using `Instant`.
Observe the bounded queue before polling, after parser input and after send
progress. This covers bytes emitted by either timer/paste parser progress or
a TTY read in the same turn as remote `POLLOUT`. Give `wants_write` only
eligible input and pass an empty input slice to `try_input` while withholding.
Framed controls still run through the existing prioritized writer. Keep the
real nonempty queue feeding the existing input progress/stall clock. The
fourth poll deadline is armed only while the batch is pending with a future
expiry. Ceil to milliseconds using existing poll behavior. Record-size flush
makes the current nonempty queue eligible even after a partial write. An
expired queue stays eligible and never alarms at zero while waiting for
credit. Batching only re-arms when the queue has drained.

Discard the bounded pending queue on detach and other foreground exits as
before. With batching, those unsent bytes can include up to an additional
configured wait's typing. Detach cannot flush input ahead of the lifecycle
control. Preserve the 32 ms tilde-prefix recognition. A single `~` may incur
32 ms plus the batching delay, subject to existing transport and credit waits.
In raw mode the Ctrl-C byte can wait for the batch. External signals remain
immediate. `0` has no armed timer and preserves the existing send behavior.

Cover full-record eligibility across partial send, both poll spin cases,
control priority, raw Ctrl-C and external SIGINT. Real PTY timing tests use
an approximate lower bound and a generous completion deadline. The pure
helper tests assert exact delayed and immediate boundaries without sleeps.
