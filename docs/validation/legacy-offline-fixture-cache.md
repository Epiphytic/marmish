# Legacy fixture cache validation

Local Linux AMD64 validation on 2026-11-09 used the actual managed CPython
3.13.13 interpreter and Rust 1.97.1 native tools. The reviewed cache source was
unchanged after integration onto `7c0c40678de148d9973e644ef2a2651e33ad3bab`.
This is optional reference-harness evidence, not native release qualification.

The exact selected test was
`test_marmish_installed_fixture.NativeFixture.test_binary_roundtrip_and_exclusive_receipt`.

| Stage | Actual result |
| --- | --- |
| Empty-cache offline control | Cargo exit 101, missing `libc` from crates.io in offline mode; zero tests, one class-setup error |
| One preparation through the new helper | Exit 0; locked fetch in 1.117 seconds; fresh cache 16,092,936 bytes |
| Same selected test with prepared cache | One test passed, zero failures/errors/skips; one offline Cargo build |
| Full `NativeFixture` class | Six tests passed, zero failures/errors/skips; eight offline Cargo builds |
| Focused cache contracts on managed Python | 27 tests passed, zero skips |

The selected control and success used the same source, compiler, test and
environment except for `CARGO_HOME`. The observer forwarded the original
commands unchanged, checked expected counts, and was restored after each stage.
No full discovery, namespace probe or namespace-positive test ran. The new RED
control failed on `libc`; an earlier historical diagnostic reported `serde_json`.
Neither identifies the original hosted failure's cause.

The private operation result SHA256 is
`3adcf3e9973088f1ea4ee64e7812e381d6b889ecf3bc2e112eff36bce0a24079`.
Its fixture source, manifest and lockfile hashes are respectively:

- `ff3090b39889143d2f23f92ff8d78ac50772be34dac3969e29b09b229139053e`
- `896191480804b1be46228fc7f6b4d5a612cac392fe4867edfd3facffe3ecd28a`
- `6acd8c25f0e1fdfab28b44287639d12779fcadb1e511b52e0b8c83c6b924aa20`

Existing supervision reaped each leader and attempted its owned group signal.
Group absence and shared-host cleanup status remain UNKNOWN. Private state and
failed setup allocations were preserved, with no retry or automatic deletion.
These focused successes do not qualify the full legacy suite, macOS, Linux ARM64
or native v0.3.1 binaries. The fetch timing is a local test-preparation measurement,
not Marmot transport latency or throughput.
