# Marmish

Asynchronous remote commands and resumable shells over Marmot, using explicitly configured relays.

The current Linux prototype runs noninteractive commands over Marmot. Commands continue running after client disconnection. Interactive PTY/tmux sessions, takeover and simultaneous read-only attachments follow in later phases. Application messages default to a configurable 24-hour retention policy. Destination syntax is `npub@relay`, also accepting a hexadecimal Nostr public key; aliases and SRV discovery are later work. Relays must always be explicitly selected.

See [the phased build plan](docs/build-plan.md). Beads tracks implementation and review work. The prototype uses a bounded adapter to a hash-pinned MDK daemon and the `marmish-engine` library for durable same-user jobs, detached workers, ordered stdin, bounded output, cancellation and replay protection. It is an experimental single-device command transport, not a production remote-shell release. See [prototype validation and limits](docs/validation/phase-2b-prototype.md) and the earlier [MDK compatibility report](docs/mdk-compatibility.md).

## Run the command prototype

Build the worker and engine bridge with `cargo build --locked --workspace` using the prerequisites below. The Python tools need Python 3.11 or newer and the standard library. Run as a non-root Unix user. On each endpoint, install `bubblewrap`, util-linux `unshare`, iproute2 `ip` and `sysctl`; the current Linux layout expects `/usr/bin/python3`, `/usr/sbin/ip`, `/usr/sbin/sysctl` and `/etc/ssl/certs`. Unprivileged user, network and PID namespaces must be enabled by the host. Startup fails if the confinement checks cannot pass; it does not change host namespace policy.

Select a trusted MDK daemon explicitly. The observed artifact used during compatibility work is `/home/openclaw/.local/bin/wnd`, SHA-256 `1324c70d389fb49f37ae8f7f6ab2a16840b138a0c438d7aef10bb6f9eee20a03`. The hash identifies that tested artifact; its exact build provenance is unverified. The adapter was checked against MDK source at `37c248a`. Another binary or hash needs its own compatibility validation.

Set these variables on each endpoint, choosing your relay and local daemon path. Repeat `--relay` for additional explicit URLs. Supported URLs are `wss://` DNS origins with an optional port and root path; there are no built-in public relays.

```sh
WND=/absolute/path/to/wnd
WND_SHA256=1324c70d389fb49f37ae8f7f6ab2a16840b138a0c438d7aef10bb6f9eee20a03
RELAY=wss://your-relay.example/
```

Generate an identity on the client, then generate the server identity on the remote host. Each command prints a public `account` identifier. Keep identity directory paths short enough for a Unix socket (the internal `mdk/s` path must fit within 100 bytes).

```sh
# Client host
python3 tools/marmish.py -i "$HOME/.marmish/client" --relay "$RELAY" \
  --wnd "$WND" --wnd-sha256 "$WND_SHA256" --init

# Remote host
python3 tools/marmish.py -i "$HOME/.marmish/server" --relay "$RELAY" \
  --wnd "$WND" --wnd-sha256 "$WND_SHA256" --init
```

On the remote host, allow the client's public identity and choose an existing working directory. Commands execute as this server's local Unix account. The server refuses root and does not switch users.

```sh
CLIENT_NPUB_OR_HEX='paste-client-public-identity'
python3 tools/marmishd.py -i "$HOME/.marmish/server" --relay "$RELAY" \
  --wnd "$WND" --wnd-sha256 "$WND_SHA256" --allow "$CLIENT_NPUB_OR_HEX" \
  --bridge "$PWD/target/debug/marmish-engine-bridge" \
  --worker "$PWD/target/debug/marmish-worker" --cwd "$HOME"
```

From the client host, address the server's public identity at the explicitly configured relay host. Arguments following `--` are literal argv, with no shell interpretation. Binary stdin, stdout and stderr are supported separately; stdin from a terminal is treated as empty for this noninteractive prototype.

```sh
SERVER_NPUB_OR_HEX='paste-server-public-identity'
TARGET="$SERVER_NPUB_OR_HEX@your-relay.example"
python3 tools/marmish.py -i "$HOME/.marmish/client" --relay "$RELAY" \
  --wnd "$WND" --wnd-sha256 "$WND_SHA256" "$TARGET" -- \
  /usr/bin/printf '%s\n' 'literal spaces, $HOME and `id`'
```

The equivalent identity option is `-u "$SERVER_NPUB_OR_HEX" your-relay.example`. Here `-i` selects a persistent private identity **directory**, not an SSH key file. Importing a supplied Nostr secret key is not implemented. Preserve the directory to reuse its identity and session; only one frontend may use it at a time.

Use `--detach` to return an invocation ID after submitting the command and sending stdin. The command keeps running if either frontend disconnects. Later, use the same identity and destination with `--status ID`, `--wait ID`, `--cancel ID` or `--retry ID`. Waiting reads retained output from the beginning and propagates the command's exit code or signal. Retry consults status before resubmitting the original invocation ID and argv; it never automatically allocates a replacement invocation. Its private journal is durable before the first submit. Interrupted stdin is not a durable client-side replay stream. A client timeout or transport error does not imply the command was cancelled.

Each remote identity and canonical relay set reuses one saved conversation by default. `--new-session` creates a fresh independent MLS group; it can still inspect jobs belonging to the same authenticated principal. `--retention SECONDS` configures a new group's application-message policy (default 86400, accepted range 1–604800). Retention is best effort: relay copies, unread or deferred local messages and backups can outlive that duration. Engine output retention is a separate policy. Explicit shell execution uses `--shell-command` and requires the server's `--allow-shell` option.

Engine state and retained output use SQLCipher as described below. The frontend's private 0600 JSON files hold session metadata and, while retry is needed, original command arguments and authorization tokens; these frontend files are not encrypted. Protect the whole identity directory and keep it out of source control. Completing a wait or observing terminal status with status/cancel removes command/token fields from its retry journal, without guaranteeing forensic erasure.

## Command restrictions

On the server, `--allow` selects identities that may connect. Add
`--command-policy /absolute/path/policy.json` to restrict their executable paths.
The policy must be a private regular file owned by the server account, with no
group or other write permission; symlinks are rejected. For example:

```json
{
  "version": 1,
  "defaults": {"allow_exec": ["/usr/bin/**"], "deny_exec": ["/usr/bin/sudo"]},
  "keys": [
    {"principal": "CLIENT_NPUB_OR_HEX", "restrictions": {"allow_exec": ["/usr/bin/printf"]}}
  ]
}
```

Defaults and **every** entry for a key apply together. Each allow list must match;
any deny match wins. Repeated entries add restrictions regardless of order.
An omitted allow list adds no constraint; an empty one denies all commands.
Policy entries must name identities already selected by `--allow`.

Patterns match the entire absolute executable path, case sensitively. `*`
matches bytes up to a `/`; `**` can cross `/`. Slashes remain literal, so
`/usr/**/echo` does not match `/usr/echo`. Backslash escapes only a literal star
or backslash. Other characters are literal. Restricted commands must supply an
absolute, already canonical executable path: PATH lookup, relative paths and
symlink aliases are refused. Resolve the executable on the server first.

These rules select executables; they do not restrict arguments or sandbox their
behavior. Allowing an interpreter, shell, `env` or a program that launches other
programs grants those capabilities. Explicit shell-string requests are refused
whenever applicable restrictions exist, even with `--allow-shell`. Policies are
bounded and malformed policies prevent startup. Matching has a shared one-million-state
budget; unusually large policies or paths can exhaust it and deny a command.
Reload by restarting the server;
accepted jobs and exact retries retain their original authority.

The engine configuration now uses version 2. New binaries read version 1 roots;
older binaries reject a version 2 root. Back up state before upgrading and do
not downgrade binaries against the upgraded root.

## Native macOS validation

The native macOS port is implemented but still awaiting runtime validation on the
supplied runners. The supported baseline above remains the tested Linux
prototype. The [native validation report](docs/validation/native-platform.md)
separates local checks from required native evidence; AMD64 macOS has no confirmed
native runner yet.

For native development, build a clean pinned daemon and its provenance manifest:

```sh
python3 tools/build_mdk.py --output /absolute/private/build/wnd
```

The output must not already exist. The script fetches the pinned MDK revision;
on macOS it applies the maintained restricted-transport patch, builds the native
helpers and signs the executables locally. It requires Rust 1.97.1, Python 3.11+
and a native C toolchain. macOS additionally requires APFS, macOS 15 or newer,
`sandbox-exec` and `codesign`. The manifest is `<wnd>.manifest.json`; macOS startup
requires and verifies it, with an optional explicit `--wnd-manifest PATH`.
Use its recorded binary SHA with `--wnd-sha256`. Linux retains namespace isolation;
macOS uses Seatbelt and a bounded SOCKS broker restricted to explicit relay origins.
Failure to establish confinement prevents startup.

The trusted-master CI workflow runs native prerequisite probes, offline checks,
clean daemon builds, real Marmot commands with disposable identities and commands
in both Linux/macOS directions. It uses explicit test relays; none becomes a
public relay default for the application.

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

The underlying Rust API remains available independently of the prototype CLI. `EnginePolicy` requires a private absolute state directory, an explicitly selected trusted `marmish-worker` executable, a server identity, and an allowlist of principals. The frontend is responsible for authenticating that principal before calling `authorize`. Both frontend and worker refuse root and account switching.

Commands run as the local Unix account with a fixed environment and account home as cwd unless server policy supplies another directory. Direct argv execution is the default; shell strings require explicit policy permission. Dropping or restarting the frontend leaves detached workers running. Service managers must preserve workers across frontend restarts; a unit configured to kill its entire control group would defeat this behavior. The embedding frontend must call `maintain` at least once a minute for bounded reconciliation and retention cleanup.

Output defaults to 8 MiB per stream with a 24-hour lifetime. Reads report truncation, expiry, and uncertain output tails explicitly. Invocation replay records last at least 48 hours independently of output. Worker death or reboot can leave an uncertain result and surviving descendants; these jobs are never automatically executed again. Process groups do not contain descendants that deliberately create a new session. Private state and logical payload deletion provide no forensic secure-erasure guarantee.

The engine encrypts its state database and retained stdout/stderr with SQLCipher. It creates a private `engine.key` in the state directory so detached workers can reopen storage unattended. Keep this key safe: losing it loses the stored data. For an encrypted-data-only backup, exclude `engine.key` and protect a separate key backup. Copying both the data and key provides no protection against whoever obtains that copy. Encryption does not protect against the same Unix account, child-created files, execution memory, swap or core dumps. Job names, file sizes, timestamps and execution-authority tokens remain outside content encryption. Output quotas count logical bytes; each stream database allows about twice its logical cap plus 256 KiB, with additional bounded rollback-journal space during writes.

This unreleased storage format rejects older plaintext roots intact. Finish existing plaintext workers and preserve their old state directory before selecting a fresh encrypted root. There is no automatic migration, deletion or key replacement. Missing, malformed or incorrect keys fail closed; do not remove a key to reset a root containing jobs. See [the reviewed storage design](docs/phases/sqlcipher-storage.md) for recovery rules and limits.

The separately built production/fault-worker test matrix is documented in [the phase 1 design](docs/phases/phase-1.md#fault-injection-build-and-acceptance-matrix). Fault injection is off by default and must remain disabled in production builds.
