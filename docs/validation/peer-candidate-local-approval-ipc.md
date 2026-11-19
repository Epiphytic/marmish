# Candidate-local approval IPC validation

Linux AMD64 used Rust `1.97.1` with the locked dependency graph, offline
builds, disabled incremental compilation and test debug information, and
the private `marmish_peer_receiver_candidate` configuration on the explicit
`x86_64-unknown-linux-gnu` target. Source
`4a1faa863e50b3a63efd8ead8e052fa4f44230a7` passed candidate native all-target
Clippy with `-D warnings` in 34.41 s. Its warm libtest build took 48.21 s.
Eight local approval tests passed in 0.01 s. The archive SHA-256 was
`d370df29d8aacc76f4d440e1d7614e8dd8b22bfb3d612b85bd957d81a8cae75f`.

The default build of the same source used one job, without the candidate
configuration. `ipc::tests::stock_socket_rejects_candidate_peer_read`
passed through a real Unix socket. Both candidate-only operations were
rejected as `invalid_request`. Its warm libtest build took 62 s.

The final test-only source `8017602f0aea30cf4fb1d25ffce82634e388b855`
compiled in 47.94 s and passed all ten `peer_approve` tests in 0.01 s
using `--test-threads=2`. The final archive SHA-256 was
`633d57d553490b820abc70e047fe8ed030a40cbc81e6ef5ec469ae9b686774cb`.
Formatting and Git whitespace checks on the eight changed Rust files passed.

The real sockets covered a same-UID request, an exact echoed decision, the
exclusive claim and decision deadlines, contention, closure and extra data,
profile descriptor permissions, the held lease path, the kernel's bound
path, the exact fatal handle, and worker stop. The stock run establishes the
ordinary protocol rejection at the daemon socket. This slice leaves the
candidate writer pair unmounted. Native decision readback and durable
approval belong to the later mounted proof.
