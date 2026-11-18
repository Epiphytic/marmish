# Native in-memory terminal room proof

The opt-in test runs two disposable MDK account runtimes in process against
the pinned upstream revision `8abc51befbf71a1ae3bdbb25eb0860c731e75db2`.
Each orientation starts again with fresh accounts, slots, credentials, route
and group. Both SQLite stores use the native SQLCipher memory-security and
in-memory temporary-storage settings.

```sh
timeout 180 cargo test --locked --offline -p marmish-native --lib \
  mdk_adapter::terminal_memory_proof_tests::unpublished_pending_package_joins_and_carries_terminal_frames \
  -- --ignored --exact --nocapture
timeout 180 cargo clippy --locked --offline -p marmish-native --lib --tests -- -D warnings
rustfmt --edition 2024 --check crates/marmish-native/src/terminal_memory_proof_tests.rs
git diff --check
```

The test passed both founder orientations on Linux x86-64 with Rust 1.97.1.
The joiner retained the exact locally signed KeyPackage artifact and reference
and consumed them after receiving a real, verified kind-1059 Welcome. The
artifact had no publish attempts, current package or authored relay event
before and after use. Both runtimes reported the expected two authenticated
MLS leaves. Each endpoint sent a real signed kind-445 event and the other
received the exact protected payload, including NUL, high bytes and terminal
control bytes. Both application events carried signed 60-second expiration.

| Founder orientation | Runtime setup for both endpoints | SQLite setup within runtime setup | Create and join | Founder send to receive | Joiner send to receive |
| --- | ---: | ---: | ---: | ---: | ---: |
| First in the process | 2,602 ms | 2,596 ms | 70.2 ms | 13.7 ms | 12.7 ms |
| Second in the same process | 2,551 ms | 2,546 ms | 60.5 ms | 12.8 ms | 11.8 ms |

One debug-build run on an AMD Ryzen AI Max+ 395 took 5.44 seconds. These are
single-sample construction measurements and four one-way in-process
send-to-receive samples. They exclude socket transport, batching, terminal I/O
and CLI startup. Clippy, formatting and diff checks passed. Claude Sonnet
independently approved the first test increment and reran the test offline in
about 5.7 seconds, with concurrent activity on the host.

## Linux syscall check

The native test executable was run directly under
`timeout --kill-after=2 240 strace -f -qq -e trace=%file,%network`, with a
fresh private `TMPDIR` and the same exact test selector and options above.
The trace used the binary from before a lint-only change in a membership
assertion and took 93.45 seconds. Both orientations passed. Of 57 observed
file and network syscall records, zero mutated filesystem paths and zero
opened or used network endpoints. Tokio opened one unnamed local Unix
socketpair. The temporary directory contained only the tracer's own output
after completion. Private trace data was removed. The outer timeout supplies
the wall-clock bound for synchronous SQLCipher initialization.

The fixture supplies the signed source ID to MDK and dispatches its captured
events directly into the receiving runtime. It verifies the in-memory
cryptographic path without activating any relay adapter or ordinary Command
owner. Relay authorization, pre-ingress misdelivery, coexistence with durable
Command state, process memory ceilings and host restart behavior are open
qualification work.
