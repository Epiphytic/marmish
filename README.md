# Marmish

Asynchronous remote commands and resumable shells over Marmot, using explicitly configured relays.

The first milestone is noninteractive remote commands that continue running after client disconnection. Interactive PTY/tmux sessions follow, with takeover and simultaneous read-only attachments. Application messages default to a configurable 24-hour lifetime. The planned destination syntax is `marmish npub@relay`; the remote identity may have a configured alias, and relays must always be explicitly selected.

See [the phased build plan](docs/build-plan.md). Beads tracks implementation and review work. The offline foundation includes explicit relay configuration, bounded message framing, and an MDK version probe. The Linux-only `marmish-engine` library adds durable same-user command jobs, detached workers, ordered stdin, bounded output with explicit gaps, cancellation, and reconnect/replay protection. No working remote-shell release is available yet; live MDK integration is gated on the prerequisites in the [compatibility report](docs/mdk-compatibility.md).

## Offline development

Use the pinned Rust 1.97.1 toolchain. The SQLCipher build vendors OpenSSL and needs a C compiler, make and Perl. After preparing the dependency cache:

```sh
cargo test --offline --locked --workspace
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --offline --locked --workspace
```

Observe an explicitly selected local executable without starting a daemon:

```sh
cargo run --offline --locked -p marmish-mdk --example probe -- /absolute/path/to/wn
```

The probe runs only `--version` with an empty environment. It does not approve the binary's provenance or enable network operations.

## Local command engine

Phase 1 exposes a Rust API, not a remote command CLI. `EnginePolicy` requires a private absolute state directory, an explicitly selected trusted `marmish-worker` executable, a server identity, and an allowlist of principals. The frontend is responsible for authenticating that principal before calling `authorize`. Both frontend and worker refuse root and account switching.

Commands run as the local Unix account with a fixed environment and account home as cwd unless server policy supplies another directory. Direct argv execution is the default; shell strings require explicit policy permission. Dropping or restarting the frontend leaves detached workers running. Service managers must preserve workers across frontend restarts; a unit configured to kill its entire control group would defeat this behavior. The embedding frontend must call `maintain` at least once a minute for bounded reconciliation and retention cleanup.

Output defaults to 8 MiB per stream with a 24-hour lifetime. Reads report truncation, expiry, and uncertain output tails explicitly. Invocation replay records last at least 48 hours independently of output. Worker death or reboot can leave an uncertain result and surviving descendants; these jobs are never automatically executed again. Process groups do not contain descendants that deliberately create a new session. Private state and logical payload deletion provide no forensic secure-erasure guarantee.

The engine encrypts its state database and retained stdout/stderr with SQLCipher. It creates a private `engine.key` in the state directory so detached workers can reopen storage unattended. Keep this key safe: losing it loses the stored data. For an encrypted-data-only backup, exclude `engine.key` and protect a separate key backup. Copying both the data and key provides no protection against whoever obtains that copy. Encryption does not protect against the same Unix account, child-created files, execution memory, swap or core dumps. Job names, file sizes, timestamps and execution-authority tokens remain outside content encryption. Output quotas count logical bytes; each stream database allows about twice its logical cap plus 256 KiB, with additional bounded rollback-journal space during writes.

This unreleased storage format rejects older plaintext roots intact. Finish existing plaintext workers and preserve their old state directory before selecting a fresh encrypted root. There is no automatic migration, deletion or key replacement. Missing, malformed or incorrect keys fail closed; do not remove a key to reset a root containing jobs. See [the reviewed storage design](docs/phases/sqlcipher-storage.md) for recovery rules and limits.

The separately built production/fault-worker test matrix is documented in [the phase 1 design](docs/phases/phase-1.md#fault-injection-build-and-acceptance-matrix). Fault injection is off by default and must remain disabled in production builds.
