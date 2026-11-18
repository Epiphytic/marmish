# Local Terminal supervisor validation

The test-gated supervisor ran on nonroot Linux x86_64 with tmux 3.4.
The test used an offline source snapshot based on `0041356`, a native
`marmishd` build, synthetic file-backed identities, disposable profiles, and
private Unix sockets. It made no relay connections.

## Native results

- The core tests passed 2 of 2. The management parser rejects unsupported
  versions, ambiguous session inventory and overlong Unix socket paths. The
  encrypted registry rejects wrong keys and profile identities, a future
  schema, a symlink rollback journal and exhausted counters or capacity.
- The native supervisor tests passed 5 of 5 in 27.13 seconds. An active writer
  and a read-only observer saw the same pane, and the writer controlled pane
  geometry. A different account presenting the writer's exact claim received
  the same missing result for read, write, resize and detach. A stale takeover
  or delayed operation under a superseded claim did not mutate the pane.
  Detaching the writer outside the supervisor invalidated all three observers,
  and a fourth observer was refused. The same slot and shell state survived
  detach and supervisor reopen, and a new slot kept its own state. After grant
  removal and restart, tmux terminated both sessions. The supervisor removed
  their exact sockets and rows. The MDK and Command journal ciphertext digests
  and the presence of Command engine storage were unchanged.
- Restart adopted a prepared slot with an exact tmux marker and kept missing,
  unmarked, wrong-marker and stale sockets quarantined without starting them.
  A mismatched socket inode refused termination, and the cleanup probe did
  not treat a live connection as proof of retirement. A retiring default row
  with ambiguous cleanup remained unavailable. Sixteen quarantined rows
  rejected both default and new selection before a worker or server started.
  A separate process flushed an uncommitted prepared-to-ready transition,
  leaving a hot SQLCipher journal. After the parent killed only that child,
  reopen restored the committed prepared row and recovered the journal. The
  Terminal key derived from the same synthetic master differed from the MDK,
  Command journal and Command engine keys.
- Existing regression checks passed. The backend core selector passed five
  unit and two worker dispatch/descriptor tests, including malformed
  four-argument dispatch and the exact observer argv. The opt-in native
  backend tmux test passed in 0.38 seconds. Engine encrypted storage passed three
  tests and engine fault injection passed eight in 40.05 seconds. Clippy
  passed with warnings treated as errors.

| Native local operation | Result |
| --- | ---: |
| Echo p50, 30 sequential round trips | 5.153 ms |
| Echo p95, 30 sequential round trips | 5.192 ms |
| Initial slot and writer launch | 239.283 ms |
| Initial launch through first confirmed render | 260.404 ms |
| Observer attach through confirmed render | 128.852 ms |
| Writer detach | 10.138 ms |
| Reattach through confirmed resumed-shell output | 146.585 ms |

Echo timings include the fixture's 5 ms polling interval when I/O would block.
They describe the local fixture and it recorded only durations and counts.
The same native attach and resume test passed 20 of 20 times in 181.16 seconds
with the test process, tmux children and a CPU load process pinned to one Linux
CPU. Each trial used a 25-second deadline and 30 sequential round trips
for 600 total samples. Per-trial echo p95 ranged from 5.123 to 6.395 ms.
First render ranged from 353.142 to 534.998 ms, and resumed output ranged
from 118.590 to 178.100 ms.
Native macOS and Linux ARM64 supervisor runs, tmux 3.3 availability, active
daemon routing, and Marmot transport latency are outside this evidence.
The same-UID login shell can modify its own tmux session or start additional
local clients, causing ownership checks to refuse. Revocation still denies
the account when cleanup is ambiguous. Tmux session termination does not
track detached shell descendants. Resolving quarantined retirement rows and
draining a final readable chunk after a writer exits require the activation
phase's PTY-to-frame lifecycle.

## Reproduction

Run from the repository root with a trusted absolute tmux executable
version 3.3 or newer and a normal, nonroot account. The fixture defaults to
`/usr/bin/tmux`. Set `MARMISH_TEST_TMUX` to override it.

```sh
cargo test --offline --locked -p marmish-native --lib core_tests_terminal_supervisor
cargo build --offline --locked -p marmish-native --bins
MARMISH_TEST_DAEMON="$PWD/target/debug/marmishd" \
  cargo test --offline --locked -p marmish-native --lib terminal_supervisor_ \
  -- --ignored --nocapture --test-threads=1
cargo clippy --offline --locked -p marmish-native --tests -- -D warnings
cargo test --offline --locked -p marmish-engine --test encrypted_storage
cargo test --offline --locked -p marmish-native core_tests_terminal_backend
MARMISH_TEST_TMUX=/usr/bin/tmux \
  cargo test --offline --locked -p marmish-native --test terminal_local_tmux \
  terminal_native_backend_tmux -- --ignored --exact
```

For the optional Linux contention run, use the executable printed by `cargo
test --offline --locked -p marmish-native --lib --no-run`. Place that executable
and same-build copies of both `marmishd` and `marmish` in a private fixture
directory. Set `fixture_dir` to that directory and give the test executable
the name `supervisor-test`. Choose an available CPU with `taskset -pc $$` and
substitute it for CPU `0` below.

```sh
timeout 360s taskset -c 0 sh -s "$fixture_dir" <<'SH'
  set -eu
  export MARMISH_TEST_DAEMON="$1/marmishd"
  yes >/dev/null &
  burner=$!
  trap 'kill "$burner" 2>/dev/null || true; wait "$burner" 2>/dev/null || true' EXIT
  trap 'exit 130' INT TERM
  for trial in $(seq 20); do
    timeout 25s "$1/supervisor-test" --exact \
      terminal_backend::supervisor::tests::terminal_supervisor_native_tmux_and_resume \
      --ignored --nocapture
  done
SH
```

The engine fault suite needs separately built fault and production workers.
Use a private temporary directory for the two copies so that the second
Cargo build cannot replace the first fixture binary.

```sh
fixture_dir=$(mktemp -d)
cargo build --offline --locked -p marmish-engine --bin marmish-engine-worker \
  --features fault-injection
cp target/debug/marmish-engine-worker "$fixture_dir/worker-fault"
cargo build --offline --locked -p marmish-engine --bin marmish-engine-worker
cp target/debug/marmish-engine-worker "$fixture_dir/worker-production"
MARMISH_TEST_FAULT_WORKER="$fixture_dir/worker-fault" \
  MARMISH_TEST_PRODUCTION_WORKER="$fixture_dir/worker-production" \
  cargo test --offline --locked -p marmish-engine --features fault-injection --test faults
rm -f "$fixture_dir/worker-fault" "$fixture_dir/worker-production"
rmdir "$fixture_dir"
```
