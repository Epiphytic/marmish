# Peer heartbeat send-fence validation

This local test target mounts the fixture-only store on a hardened
SQLCipher file. It checks durable sender-side reservations before any
accepted Peer owner or publication adapter is wired in.

```sh
cargo test --locked -p marmish-native --test peer_heartbeat_send_fences
cargo test --locked -p marmish-native --test peer_heartbeat_send_fences \
  peer_heartbeat_send_fences::core_tests::core_
cargo clippy --locked -p marmish-native --test peer_heartbeat_send_fences -- -D warnings
cargo test --locked -p marmish-native --test peer_heartbeat_send_fences \
  benchmark_peer_heartbeat_send_fences -- --ignored --nocapture
```

On Linux x86-64 with Rust 1.97.1, the full target passed 24 cases
(12 new store cases and 12 mounted codec cases), with two opt-in
benchmarks ignored. The exact core filter passed five store cases
and selected no codec test or benchmark. Strict Clippy for the
integration target and rustfmt for all three Rust files passed.
Tests and Clippy ran against the committed base
`b6bbb684d35aba0f5f431431781e40559a6064e5` with the three new Rust files
copied in. This excluded unrelated in-progress branches of the
shared worktree.

The storage cases cover exact directional authority and policy
matching, schema and PRAGMA refusals, preserved bytes on a refused
open, full-width counter exhaustion, range checks, per-room sample
floors and profile-wide wall rollback, retirement and capacity,
reopen, a held SQLite writer lock, an actual commit-hook abort,
and rollback failure that poisons the wrapper. The invalid-key
case intentionally produces SQLCipher decryption diagnostics.
Every fixture verifies it has no sidecars after use and removes
its dedicated temporary directory. The core and benchmark test
processes left their dedicated temporary root empty.

## Local debug benchmark

A single test process ran against an ext4 disposable database with
`synchronous=FULL` and DELETE journaling. Timings include the local
transaction and SQLCipher commit for reservations, and opening,
validating, restoring and closing for the reopen measurement.

| Operation | Samples | p50 µs | p95 µs | p99 µs | Max µs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Heartbeat reservation | 200 | 51,642 | 90,224 | 142,172 | 170,342 |
| Permission issue reservation | 200 | 51,589 | 88,359 | 202,395 | 307,671 |
| Close, reopen and restore | 100 | 707 | 968 | 1,223 | 1,991 |

The five core cases took 1.55 seconds in this run and the separate
benchmark took 23.13 seconds. These values reflect this host's
local durable storage, including filesystem synchronization.
No relay, MDK publication, native acceptance, cross-platform
installation or Command-path latency was exercised.
