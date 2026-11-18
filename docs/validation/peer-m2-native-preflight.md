# Peer M2 public runtime preflight validation

The opt-in integration test qualifies the closed SQLCipher image used after a
provisional Welcome join. It uses locked upstream MDK
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`, disposable accounts and a refusing
transport adapter and KeyPackage publisher. The signed carrier is the
fixture-local kind 24860 with no subscription ID. The test connects to no relay.
The later M2 work must separately qualify the M1 kind-7435 delivery and the
custody owner's file guards, state transitions and recovery.

The recipient opens with explicit DELETE journaling, beginning with private
KeyPackage preparation. After preparation, fresh `TransportAdapter` and
`KeyPackagePublisher` counters cover one public
`AccountDeviceRuntime::ingest_delivery` and two independent snapshot readbacks.
The test checks the exact expected native Welcome ID in the sole `GroupJoined`
effect and durable receipt. It also checks the current protocol profile,
purpose, route, local leaf, founder as sole admin and consumption of the exact
prepared KeyPackage. Every measured recipient open reports zero migrations
through the public `migration_summary()` API.

Each snapshot stays within a 16 MiB ceiling, has a distinct inode in a private
directory and begins with the exact digest of the authoritative closed image.
The original inode and digest remain stable across both readbacks. The test
checks both images for an unexpected SQLite journal, WAL or SHM after closing
and checks cleanup of each disposable copy. An initial diagnostic run found
that MDK's writable readback can change the encrypted copy's bytes. The final
assertions permit that change on a disposable copy while requiring its file
identity and the recovered authority to remain exact. The test does not assign
the byte change to a specific upstream write.

## Reproduce

At source commit `69a1edca0499b6f14ccffd53fe320d7d30bca756`, all five
opt-in public-runtime tests passed in 109.28 seconds, excluding compilation.
Strict Clippy passed for `peer_public_runtime`. The existing wrong-key recovery
probe emits SQLCipher authentication errors as part of its expected rejection.

```sh
cargo test --offline --locked -j2 -p marmish-native --test peer_public_runtime \
  m2_preflight::pending_delete_image_reopens_through_public_runtime \
  -- --ignored --exact --nocapture
cargo test --offline --locked -j2 -p marmish-native --test peer_public_runtime \
  -- --ignored --nocapture --test-threads=1
cargo clippy --offline --locked -j2 -p marmish-native \
  --test peer_public_runtime -- -D warnings
```

The complete opt-in test target includes one existing private loopback relay
scenario. The new preflight uses no relay or public infrastructure.
An independent Linux run traced network syscalls using
`strace --seccomp-bpf -f -qq -e trace=network`. It passed in 20.00 seconds
and recorded one local Unix socket pair and no IP sockets or connections.
The preflight remains opt-in for CI.

## Local timings

One unoptimized Linux x86-64 run with Rust 1.97.1 on AMD Ryzen AI Max+ 395
measured the following times. These are local fixture timings and exclude
compilation. Setup includes disposable KeyPackage preparation and founding.

| Stage | Time |
| --- | ---: |
| Test setup | 21,805.69 ms |
| Public join, close and closed-image check | 259.71 ms |
| Two disposable snapshot readbacks and cleanup | 599.82 ms |
| Total | 22,665.21 ms |

## Composed validation

On Linux AMD64 the native preflight and quota dispatcher increments were
composed on `4bb566e` as tree `e94a7eca62791d02bf94893e2b7f6b4accc051e3`.
Each source scope was byte-identical to its independently approved version.
Upstream MDK master still resolved to the existing locked revision
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`.

- Strict offline locked all-target workspace Clippy passed in 28.12 seconds.
- The five ignored scenarios in the public runtime integration target ran
  explicitly and passed in 92.46 seconds.
- The seven opt-in quota harness entries passed in 34.49 seconds, including
  ten separate dispatcher child invocations. The harness confirmed exact
  cleanup and recovery using its own isolated proof directories.
- The previous M1 commit's live Terminal and ordinary Command regression
  passed in 37.17 seconds using the complete sibling-launcher layout. A
  copied test launcher directory without the required `marmish` sibling
  had failed worker discovery. The corrected directory restored that
  prerequisite.

Static quality analysis flagged new overlap among test adapters and other
existing test or operating-system helpers, VFS callback duplication and
higher fixture churn. The scoped Claude reviews and tests cover these
proofs. Installed cross-platform operation and a deployed Peer owner
remain separate qualification milestones.
