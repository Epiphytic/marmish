# Attached Command output polling validation

[Design](../phases/command-poll-contention.md).
The unchanged baseline at `3cef7a7` and the later `f0f2aee` build each
reproduced `recovery_uncertain` using fresh profiles and an explicit White
Noise relay. A separate baseline trace observed the worker and requested
program both execute. A diagnostic build observed a Busy read and a Found
completion without a Poll reply. These were separate runs. The diagnostic
build did not identify the particular contended syscall.

The frozen patch test used the three affected Command files over `5203e4f`
on Linux x86-64. It holds the live worker's output lock, observes the actor's
typed Busy, releases the lock, and reads exact independent stdout and stderr
buffers with remote exit 7. The same fixture makes `io.lock` unsafe to access
and asserts that the resulting non-Busy error still produces the generic
Found observation. Other checks cover durable replay after reopen, unchanged
cursors and retry budget, temporary errors on stale authority, revocation,
and exhaustion after three non-Busy Found observations.

```sh
cargo build --locked -p marmish-native --bins
cargo test --locked -p marmish-native --lib poll_ -- --include-ignored
```

The frozen source passed 18 Poll tests including the real worker fixture.
The broader pre-re-review Command suite passed 208 tests with two ignored.
The two additions requested in review were validated by the focused run.

## External relay smoke test

Use two disposable private profiles with generated identities, explicit
`wss://relay.us.whitenoise.chat/`, and `--tor off`. Configure server
`setup --role both --grant CLIENT_NPUB` and client
`setup --update --trust SERVER_NPUB`. Start both daemons in the foreground.
Wait for readiness and an accepted KeyPackage publication on both sides.
Invoke `marmish --state-dir CLIENT_DIR --no-autostart --wait-seconds 90
SERVER_NPUB -- COMMAND ARGUMENTS` and compare stdout, stderr and status as
bytes. Shut down only the owned children and remove both private test
profiles.

| Frozen patch observation | Expected and observed | Wall time |
| --- | --- | ---: |
| Remote `printf %s` with shell metacharacters, quotes and Unicode as literal argument bytes | 35 exact stdout bytes, empty stderr, exit 0 | 8.358 s |
| Remote program emitting NUL and high bytes on both streams | 5 exact stdout bytes, 5 exact stderr bytes, exit 7 | 4.841 s |

The two calls used the same fresh pair. The first call includes initial
room setup. Neither returned `delivery_pending` nor `recovery_uncertain`.
No deliberate contention was introduced during the external test. Timing
here is two end-to-end observations, and deterministic contention coverage
comes from the real worker fixture. This run did not qualify Tor or native
macOS and ARM64 execution.
