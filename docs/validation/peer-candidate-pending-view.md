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

The view is compiled only into candidate tests. Sealed durable observation
construction and same-UID listener integration remain separate. A simulated
in-flight selection crossed the freshness boundary and was refused. A
real slow socket reader and request-scoped native and SQL counters require
the live mount.
