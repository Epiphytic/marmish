# Retry transient Command output polling

A fresh-profile attached CLI call on the unchanged `3cef7a7` baseline
returned `recovery_uncertain` over White Noise even though a separate
traced reproduction observed both the worker and requested program
exec successfully. A diagnostic build of the baseline observed
`EngineError::Busy` from `poll_outputs` and a
`Found` completion without a corresponding Poll response. These
observations are distinct runs. They do not establish which operation
inside `poll_outputs` was busy. The engine permits transient Busy while
the worker owns a nonblocking output lock.

Preserve that distinction at the Command actor boundary. For
`EngineOperation::Poll` only, return a typed busy observation with the
pre-read fallback status and `WorkerState::Unknown` when `poll_outputs`
returns exactly `EngineError::Busy`. These are the facts that the current
fallback retains.
Leave the generic fallback for any other error. Keep the existing
nonblocking engine behavior. If one stream was read before the second
reports Busy, discard both tentative chunks. The engine read has no
cursor-acknowledgment side effects.

For a matching current-revision Poll slot, use the same authority gate
and terminal-response commit path, returning the already supported
wire `ErrorCode::Busy` with empty detail. Project the captured status
and unknown worker through the usual durable fact path. Leave the
failure count unchanged and preserve both delivery cursors. Authority
revocation suppresses the slot and is checked before the current revision
comparison. A Busy for a superseded revision with valid authority yields
no Poll response and increments `failed_attempts`, reaching
`RecoveryUncertain` on the third attempt. A mismatch of key, operation,
completion revision, owner or pending slot remains invalid before
response projection.
Existing clients already consume Busy and issue a fresh Poll ID with the
current cursors and bounded deadline. A retransmission with the same
request ID still receives the same durable Busy reply. For a persistently
busy output lock and current-revision polls, the client deadline governs
exhaustion and leaves a pending handle. There is no new
wire kind, error code, persistence schema, launch retry, or policy
change. Storage and uncertain failures keep their existing handling.

Exercise the specific actor branch using a deterministic nonblocking
output lock fixture with an already admitted invocation. Prove no output
or cursor advance on Busy, release the lock, and poll with a fresh ID for
the exact bytes and exit. Test current completion projection, unchanged
failure count, durable matching replay, revocation, a stale Busy
incrementing `failed_attempts`, and a non-Busy error retaining its
exhaustion behavior. Add a client Poll test that consumes a
Busy reply with the original start cursors, writes no output, and retries
with a fresh ID and the same cursors. Run a fresh actual attached
client/server command with disposable identities and explicit relay
selection after the fix, plus focused Command tests and the core CI
selection. Check whether contention was reproduced in the live run
separately from the deterministic fixture.
