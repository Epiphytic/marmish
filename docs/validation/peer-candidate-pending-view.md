# Candidate Pending memory view

Exact integrated source `1f63e9fd9405586c8687e61a8d69b67379ee8491`
on Linux AMD64 with the explicit Cargo candidate configuration.

Seven tests passed. They covered the exact Pending and healthy-empty views,
all selector coordinates, separate actor and listener identity, publication
invalidated during a turn, four-slot admission and release, producer and
listener stop, and poisoned state. Clock cases included the last nanosecond
of a fresh proof, the exact one-second rejection, absent or backwards
samples, the authenticated wall floor, and process-local expiry rollback.
An independent fresh view illustrates the documented persisted-floor limit
after restart.

Strict candidate Clippy passed for all native targets in 30.55 s. Both
candidate executables built in 19.79 s, with no view symbols in their
symbol tables. Formatting and the staged diff checks passed.

At this baseline, the view compiled only into candidate tests. A simulated
in-flight selection crossed the freshness boundary and was refused. A
real slow socket reader and request-scoped native and SQL counters require
the live mount.

## Candidate reply admission

Exact source `f82d832033ef9553b51002474c1d5019f1097b57` on `584e439`.
The Linux AMD64 candidate build passed all 11 `ipc::peer_read` lib tests.
The new test uses real clocks and covers an unavailable initial view,
wrong listener generation, the exchange deadline, a qualified reply and
listener stop. Strict candidate `--lib --tests` Clippy with `-D warnings`
passed in 27.54 s, including the non-test library. Scoped formatting and
the diff check passed. Claude independently reran eight view tests.

The candidate memory API provides `pair(listener_generation)`,
`Reader::admit(listener_generation)` and `Reader::stop()`. After same-UID
verification the listener can use the admitted
`Read::reply_before_write(request, listener_generation, exchange_deadline)`
and hold its permit through the deadline-bounded write. The current
request route still returns Unavailable. Positive publication remains
test-only until the future owner supplies an opaque, one-use Pending or
healthy-empty proof bound to the original observation ticket. The raw
observation, its constructor and injectable clocks stay private to the
view. No live listener or actor was enabled by this increment.
