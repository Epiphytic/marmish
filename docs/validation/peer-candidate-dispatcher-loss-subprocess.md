# Registered dispatcher-loss validation

The candidate source under test is
`775e55663ebe3e791a429c24921f1fec71417f41` on Linux AMD64. The fault
controller ran from an ignored libtest executable with a private installed
candidate daemon layout. Its subprocess called the actual registered
Command-only daemon path with a fixture-owned dispatcher fault. The healthy
comparison uses a separate fresh exec of the private qualification example.
Each case owns disposable identities, databases and separate loopback WSS
listeners for ordinary Command and Peer qualification.

| Cut | Observed startup/IPC | Owned ordinary WSS sockets closed | Registered wait |
| --- | --- | --- | --- |
| After startup ACK, before Commit | Real Ready and ACK, no Commit or success | 1 | Exit 1 |
| After Commit, with cached active Status | Real Ready, ACK, Commit and first active Status. Stale Status returned zero bytes. Fresh Status was refused. | 1 | Exit 1 |

In both cases the listener joined, the actual fatal latch stayed set and all
five physical obligations reported complete. No Peer socket opened during
the measured interval. The profile lock stayed held at the post-stop
observation and became available after reap. The owned group no longer
existed. The pre-cut, stopped-but-still-held and post-reap images matched in
presence, device, inode, length and content for MDK, the Command Journal,
the engine state and their SQLite sidecars. The stock reopen with the
original material authenticated the expected account and empty Command
state, with zero engine job rows and no job-spool files.

### Iterative checks

The author initial passing isolated run took 26.52 s, including 13.090 s
for the pre-Commit scenario and 13.422 s for the live Status scenario.
Claude independently reran both in 28.14 s. These are qualification
fixture wall times, including setup and stock reopen. They are not
relay or command-latency benchmarks. Before those runs, the original
test-only child selector and the ordinary relay witness recorder were
corrected. The production package for the iterative tests was stamped
`d7a4722`. The test-only changes through `775e556` did not change
the normal binaries' source.

The exact final test source also passed 36 startup protocol cases
(5.70 s), 17 physical owner/stop cases (59.82 s), the opt-in private
WSS physical closure case (4.19 s), and the image-tamper/sidecar-removal
control. Stock native all-target Clippy with `-D warnings` passed
(31.24 s). Scoped Rust formatting and `git diff --check` passed.

### Exact installed package

The native release tool packaged three normal candidate executables from
the reviewed source. The installer accepted the local manifest by pinned
SHA-256 and activated that version in a private prefix. The final fault
test used a test executable built with the same source stamp and copied
into the private installed layout for the test. It passed in 24.60 s,
including 12.201 s for pre-Commit and 12.400 s for live Status.
The healthy source-matched fresh-exec and Command round trip passed in
42.41 s. The observed daemon readiness was 6.400 s and the finite ordinary
Command took 5.677 s on the private relay. Candidate all-target native
Clippy with `-D warnings` passed in 29.10 s.

The fault-control entry, parser and IPC observers, physical report
accessor and relay witness recorder were absent from the symbol tables
and strings of the three installed normal binaries.

Run the ignored selector
`daemon::registered::registered_loss::parent::registered_dispatcher_loss`
from the source-matched test executable in the private candidate version's
`deps` directory. Set `MARMISH_TEST_REGISTERED_QUALIFICATION_DAEMON` to
that installed version's canonical daemon path. This is a local
qualification for the private Linux AMD64 candidate. The test exercises
the three measured file images at the named observation boundaries.
