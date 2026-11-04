# Native background daemon validation

This source increment implements the approved [background daemon design](../phases/native-background-daemon.md). Published v0.2.2 assets retain their previous foreground behavior. No macOS result is inferred from a Linux test.

## Linux live execution

The [machine-readable results](native-background-daemon-linux.json) use locally built Rust binaries, disposable file-backed client/server identities, Tor off, and the explicitly selected `wss://relay.us.whitenoise.chat/`. This is a live Marmot command test, not a publication-only check or installed release qualification. Both profiles have a 60-second uptime limit. Private state is removed only after both daemons emit joined engine/MDK/runtime cleanup receipts.

The server starts with default background semantics and debug enabled, returns zero with empty stdout/stderr after activation, and reports active through IPC. A second start fails visibly with `identity: profile_in_use`. Two clients start simultaneously against the same cold local identity and execute canonical `/usr/bin/printf` remotely. Arguments include dollar signs, semicolons, backticks, quotes, backslashes, slash, newline, tab and Unicode. Returned bytes must exactly match inputs; byte counts and SHA-256 digests are retained. Both private mode-0600 logs exclude the command-output sentinel and contain no panic.

Two initial harness inputs were corrected: specifying both an explicit relay argument and a relay-bearing destination was rejected as conflicting_destination; `/bin/printf` was rejected as noncanonical_executable. The passing test uses one relay source and canonical `/usr/bin/printf`. These were fixture corrections, not relaxed product validation. All three runs had bounded daemon lifetimes, and their owned private fixtures were removed after joined receipts.

## Checks and review

Actual Claude design reviews and integrated code reviews are under `docs/reviews/native-background-daemon-*`. The first full native run passed 370 tests and failed one existing test at `fs::copy(current_exe)` with ENOENT; that test passed alone. A concurrent shared-target rebuild is a plausible explanation, not an established diagnosis. Subsequent full checks are run without another executable rebuild in parallel.

The added closed-stdio regression initially restored stdin over its own parent startup socket before checking readiness. The fixture now completes the nested handshake before restoring stdio. Production reserves the child socket above standard descriptors before Command configures stdio.

`ripwire . --quality-delta` reports exit 2, not a clean gate. The real command_socket complexity increase was removed by extracting autostart construction. Remaining findings are small repeated private test-directory fixtures, a similar fcntl shape across crates with different flags, historical churn, and 12 added serve_owner lines for readiness/rate limiting. Those bounded additions are retained for direct lifecycle handling; no baseline or suppression was introduced. Rust formatting, strict lint, full-suite and platform results are recorded below when complete.

## Final local checks

- `cargo test -p marmish-native --all-features --lib --locked`: **471 passed, zero failed**, 366.58 seconds, run sequentially after the fixture correction with no concurrent executable rebuild. This includes installed-check feature tests.
- `cargo clippy -p marmish-native --all-targets --all-features --locked -- -D warnings`: **passed**, 11.79 seconds.
- `cargo fmt --all -- --check` and `git diff --check`: **passed**.
- `cargo build -p marmish-native --bins --locked`: **passed** after the final suite.
- Actual final Claude Sonnet 5.5 source review: **APPROVED**, session `a4bc8c96-bc8f-45b9-b286-ecca47bbb8b4`, with final suite/lint/live evidence required before commit.
- Nonblocking review refinements are tracked as Beads `marmish-e25`.

The earlier all-feature run passed 470 tests and failed only the now-corrected closed-stdio fixture. This earlier result is retained here rather than represented as a product pass.

The final rebuilt-source live repeat passed all six cases in 66.647 seconds, with private state removed after joined cleanup. Quiet server readiness took 805 ms. Concurrent cold-client commands returned exact 46-byte and 27-byte outputs in 4,622 ms and 4,828 ms. These include startup/relay/MLS work and are fixture timings, not steady-state throughput benchmarks.

Platform candidate CI is pending source publication; no binary release or tag is included in this increment.
