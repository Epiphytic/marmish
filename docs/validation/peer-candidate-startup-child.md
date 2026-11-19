# Registered startup child exchange validation

Linux AMD64 source `e2c0dc48a2c49a9789ab0cb6296e605dbcea985f` was built
against its locked dependencies. This is the internal child-side protocol
under the [reviewed startup design](../phases/peer-candidate-startup-report.md).

| Profile and selector | Result |
| --- | --- |
| Stock configuration libtest, `daemon_launch::candidate_child::tests` | 11 passed, 5.00 seconds |
| Candidate configuration libtest, `daemon_launch::candidate_child::tests` | 11 passed, 5.00 seconds |
| Stock configuration libtest, `daemon_launch::tests` | 21 passed, 0.67 seconds |
| Stock configuration libtest, exact ignored `daemon::candidate_lifecycle::inert_daemon_reporter_wait_prevents_release_on_failure_or_stop` | 1 passed, 7.64 seconds, private loopback WSS |
| Candidate `cargo clippy --offline --locked --target x86_64-unknown-linux-gnu -p marmish-native --all-targets -- -D warnings` | Passed, 29.61 seconds |
| `cargo fmt --all -- --check` and `git diff --check` | Passed |

The first two runs use the same source and different configurations. Both
enable `cfg(test)`; the candidate run additionally sets
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS=--cfg marmish_peer_receiver_candidate`.
Builds recorded the full tested Git SHA in `MARMISH_BUILD_COMMIT`, disabled
incremental/debug artifacts and used at most two Cargo jobs. The stock libtest
link took 44.25 seconds. The candidate link took 43.07 seconds. The SHA-256
of the local archived stock test executable is
`0c10bfc84c3371b469d5258b975c917433fb67aa21fd09eb364f1454376904d4`.
The candidate test executable's SHA-256 is
`b05570a8c2f7935c0c1a268136e6a87c8d118e3360683a8ec635bbd221b1e007`.

Disposable Unix sockets establish one-shot ownership, delayed acknowledgement,
malformed and EOF handling, a single real five-second budget, and commit
survival after a later fatal cut. Other cases exercise both fatal and non-fatal
refusal, a caller-supplied expiry after the child consumed A and before the
next admission, and repeated admission on a genuinely backpressured one-byte
write. The bounded pending-I/O fatal test trips the real lifetime signal and
requires closure while the parent still has the full backlog. The inert
regression uses the existing stock reporter and never mounts the new helper.

The helper receives exact Admission and Fatal from the same registered owner.
Registered daemon assembly, real dispatcher replacement in that daemon,
the installed private parent, foreground commit, and complete owned stop are
covered by the larger startup design and require their own qualification.
