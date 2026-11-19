# Active candidate Command owner validation

This increment prepares the private Linux AMD64 Command owner. It adds no
daemon caller, IPC route, Peer operation or release activation. The owner-side
admission originates in the same registered candidate constructor that opens
the checked native account and Journal. Stock owner operations follow their
existing path.

The opt-in `candidate_native_owner_active_cut_qualification` selector uses a
fresh child process with a verified registered default VFS and a real keyed
account and Journal. It completes a healthy status read, then polls a
future that reads actual native package lifecycle evidence and suspends.
Replacing the default VFS leaves the fatal latch clear. On the very next
poll, admission refuses with `candidate_dispatcher_lost` before the native
future is polled again. The test compares evidence for the account and Journal
database families, socket count and held account descriptors, and checks that
activation, ticks, status, count-based exit and orphan compensation refuse
after displacement. The connector is inert and the test uses no relay.

Exact source `776487bc946f6c5be3c5f4586f0971aeb800dd9b` on Linux AMD64
passed the candidate libtest link (46.02 s), candidate all-target Clippy
with `-D warnings` (29.90 s), stock all-target Clippy with `-D warnings`
(30.35 s), scoped Rust formatting and `git diff --check`. Cargo used the
locked dependency graph, the existing offline caches and two build jobs.
The author ran the displacement selector against the immutable candidate
libtest (1 passed in 7.52 s). Claude independently verified its SHA-256
and reran the same selector (1 passed in 6.51 s). The artifact SHA-256 is
`39e8dff88d2a7508f732905eb9b6dcd47f73280b87f8b85eb515222beb3766fa`.
Those durations measure the whole fixture, including subprocesses and
SQLCipher, and cannot estimate active poll overhead.

Ripwire quality comparison against the pre-source revision flagged
existing long owner functions lengthened by explicit admission gates,
short-horizon churn and parallel trait guards. The substantive mount risks
are tracked as owner exit checks below. No broad owner refactor was included
in this scoped change.

The complete owner exit still requires cuts at the independent activation,
projection, ACK, room and retained publication boundaries in the
[phase design](../phases/peer-candidate-command-active.md). Proving preservation
of exact original Welcome and generic, Stream and Command publication bytes
across restart, and measuring admission cost under load, remain part of that
exit. The subsequent composed mount requires real private WSS, the same-UID
listener, startup reporting and physical custody on failure.
