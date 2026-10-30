# Marmish

Source: [GitHub](https://github.com/Epiphytic/marmish) ·
[Radicle](https://radicle.network/nodes/iris.radicle.network/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg)
(`rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg`).

## Experimental binary installation

[v0.1.0-experimental.1](https://github.com/Epiphytic/marmish/releases/tag/v0.1.0-experimental.1)
is available for Linux AMD64, Linux ARM64 and macOS ARM64 (macOS 15+).
Both Linux archives require glibc 2.39 or newer. All three passed native build
and installation checks. The exact Linux AMD64 release archive passed all 15
installed command tests locally through Marmot; the hosted runner denied the
required namespace operation. See [the distribution report](docs/validation/distribution.md)
for source, archive bindings and publication verification status. ARM64 Linux and macOS
confined execution are not validated in this distribution. Earlier self-hosted
validation found a Linux ARM64 namespace-policy failure and a macOS broker abort;
these are not observations from hosted packaging jobs. Successful building or
installation does not establish working confined execution on those platforms.
Intel macOS is not included.

### Copy-and-paste quickstart

Run this block on both machines as your normal Unix user with Python 3.11+ and
curl. Have the platform prerequisites described below in place; the installer
does not install system tools, initialize a keychain service or enable namespaces.
Choose `server` on the remote machine and `client` on the connecting machine,
using the same explicit relay URL:

```sh
(
  set -e
  installer=$(mktemp "${TMPDIR:-/tmp}/marmish-install.XXXXXX")
  trap 'rm -f "$installer"' 0
  trap 'exit 1' HUP INT TERM
  curl --proto '=https' --tlsv1.2 -fsSL \
    https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh \
    -o "$installer"
  sh "$installer"
  rm -f "$installer"
  PATH="$HOME/.local/bin:$PATH"
  export PATH

  printf 'Role (server or client): '
  IFS= read -r ROLE
  case "$ROLE" in
    server|client) ;;
    *) printf '%s\n' 'Choose server or client.' >&2; exit 1 ;;
  esac
  printf 'Relay URL (wss://your-relay.example/): '
  IFS= read -r RELAY
  IDENTITY="$HOME/.marmish/default"
  marmish --doctor
  marmish -i "$IDENTITY" --relay "$RELAY" --key-storage auto --init

  case "$ROLE" in
    server)
      printf 'Client public identity to allow (npub or hex): '
      IFS= read -r CLIENT_NPUB_OR_HEX
      marmishd -i "$IDENTITY" --relay "$RELAY" \
        --allow "$CLIENT_NPUB_OR_HEX" --cwd "$HOME"
      ;;
    client)
      printf 'Server public identity (npub or hex): '
      IFS= read -r SERVER_NPUB_OR_HEX
      marmish -i "$IDENTITY" --relay "$RELAY" -u "$SERVER_NPUB_OR_HEX" "$RELAY" -- \
        /usr/bin/printf '%s\n' 'Hello through Marmot'
      ;;
  esac
)
```

Initialization prints `{"account":"…"}` with a public identifier in lowercase
hexadecimal. Exchange those public identifiers between the machines when
prompted; `npub` values work too. Enter the client identity on the server and
start it before submitting the server identity at the client prompt. The server
stays in the foreground, so keep its terminal open while running the client.
An allowed client can execute commands as this Unix user, starting in `$HOME`.
Its allowlist applies to that invocation; supply `--allow` again on
each restart. See [the step-by-step setup](#first-run-with-installed-binaries)
for restarting without reinstalling and allowing multiple client identities.

The chosen `~/.marmish/default` profile is reused on subsequent runs; this path
is a quickstart convention, and the CLI still requires `-i`. If both endpoints
run on one machine, edit `IDENTITY` to use separate directories for client and
server because a profile permits only one active frontend. Key storage defaults
to the available native keychain, with files selected only for a proven absent
backend; locked, denied or failed keychains stop setup. The block stops on a
failed download, install, prerequisite check or initialization. Its environment
changes stay in the subshell. For later commands, add `~/.local/bin` to your
shell's PATH and set `IDENTITY` and `RELAY` again as shown in the step-by-step
setup. The runtime limitations above still apply.

The script is served from Radicle through Iris and downloads complete native
archives and `SHA256SUMS` from [GitHub Releases](https://github.com/Epiphytic/marmish/releases).
It selects an experimental prerelease; pin one with `sh install.sh --version
0.1.0-experimental.1` after downloading the script. For an immutable installer,
replace `head` in the Iris URL with a full reviewed commit SHA. Mutable `head`
trusts the script currently served by Iris; same-release checksums detect
corruption and are not independent signatures.

By default, versions live under `~/.local/share/marmish/versions` and launchers
under `~/.local/bin`; add that directory to PATH if needed. `--prefix /absolute/path`
uses that prefix's `bin/` for launchers. Paths containing spaces and shell
characters are supported. The installer refuses root, unsafe destinations,
unrelated existing launchers and corrupt archives. It does not edit shell
profiles, identities or keychains. Reinstalling identical bytes is safe;
upgrades retain old version trees, and running processes keep their old paths.
There is no automatic state downgrade. `sh install.sh --uninstall` removes only
installer-owned launchers and bookkeeping, retaining versions and user state.

Installed `marmish` and `marmishd` supply their bundled daemon, manifest, native
keychain helper, bridge and worker paths. Keep specifying identity, relays and
server authorization/cwd as in the examples below; explicit helper overrides
remain supported. `--doctor` reports static prerequisites without accessing
keys or proving confinement. Linux needs Bubblewrap, util-linux/unshare,
iproute2, `/usr/bin/python3` 3.11+, CA certificates and working unprivileged
user/network/PID namespaces. Each archive records its measured minimum glibc and
shared libraries; installation checks those requirements. macOS builds are
ad-hoc signed, without Developer ID signing or notarization; upgrades may need
renewed Keychain permission and fail closed if access is denied.

This release uses script installation. `cargo install marmish` is not available:
the application frontends are Python and the current pinned SQLCipher patch
needs a separate source-distribution design. Building the Rust workspace alone
does not install the complete application. The release workflow does not use a
registry token for incomplete crate publication.

Marmish is [MIT licensed](LICENSE); complete runtime archives include original
dependency license texts, native dependency notices and pinned source references
for both Marmish and MDK. See the [distribution design](docs/phases/distribution.md).

Asynchronous remote commands and resumable shells over Marmot, using explicitly configured relays.

The current Linux prototype runs noninteractive commands over Marmot. Commands continue running after client disconnection. Interactive PTY/tmux sessions, takeover and simultaneous read-only attachments follow in later phases. Application messages default to a configurable 24-hour retention policy. Destination syntax is `npub@relay`, also accepting a hexadecimal Nostr public key; aliases and SRV discovery are later work. Relays must always be explicitly selected.

See [the phased build plan](docs/build-plan.md). Beads tracks implementation and review work. The prototype uses a bounded adapter to a hash-pinned MDK daemon and the `marmish-engine` library for durable same-user jobs, detached workers, ordered stdin, bounded output, cancellation and replay protection. It is an experimental single-device command transport, not a production remote-shell release. See [prototype validation and limits](docs/validation/phase-2b-prototype.md) and the earlier [MDK compatibility report](docs/mdk-compatibility.md).

## First run with installed binaries

Run as your normal Unix user. On **each machine**, choose the same explicit
relay URL and initialize a local identity:

```sh
printf 'Relay URL (wss://your-relay.example/): '
IFS= read -r RELAY
IDENTITY="$HOME/.marmish/default"
marmish -i "$IDENTITY" --relay "$RELAY" --key-storage auto --init
```

For a new profile, initialization generates the local Nostr key and prints its
hexadecimal public identifier as JSON: `{"account":"…"}`. Copy that public
identifier when configuring the other endpoint; an `npub` is accepted too. Keep
using the same identity directory
to reuse the key and saved sessions. `~/.marmish/default` is a convention in this
quickstart; the CLI still requires `-i`. If client and server run on one machine,
choose separate identity directories because a profile allows only one active
frontend.

`auto` is already the default: it stores keys in macOS Keychain or Linux Secret
Service when available. Only a proven absent backend selects private files,
with a notice on stderr; a locked, denied, uninitialized or failed keychain stops
setup. Linux native
storage needs an initialized persistent Secret Service collection. Installation
itself does not create identities or access the keychain; setup needs your
explicit relay selection.

On the **server**, enter the client's public identity and start the server in
the foreground:

```sh
printf 'Client public identity to allow (npub or hex): '
IFS= read -r CLIENT_NPUB_OR_HEX
marmishd -i "$IDENTITY" --relay "$RELAY" \
  --allow "$CLIENT_NPUB_OR_HEX" --cwd "$HOME"
```

Repeat `--allow` for additional client identities. These permissions apply to
this server invocation; supply them again on restart. Authorized commands run as
this Unix user in the selected directory. Keep the server running; interactive
shells remain future work.

On the **client**, enter the server's public identity printed during its setup
and run a command:

```sh
printf 'Server public identity (npub or hex): '
IFS= read -r SERVER_NPUB_OR_HEX
marmish -i "$IDENTITY" --relay "$RELAY" -u "$SERVER_NPUB_OR_HEX" "$RELAY" -- \
  /usr/bin/printf '%s\n' 'Hello through Marmot'
```

Here `-u` selects the remote Nostr identity, and the destination is the explicit
relay URL. The equivalent destination form is `npub@relay-host`. Arguments after
`--` are literal command arguments. The platform runtime limitations above still
apply; successful installation or `--doctor` alone does not prove confinement.

## Run the command prototype

Build the worker and engine bridge with `cargo build --locked --workspace` using the prerequisites below. The Python tools need Python 3.11 or newer and the standard library. Run as a non-root Unix user. On each endpoint, install `bubblewrap`, util-linux `unshare`, iproute2 `ip` and `sysctl`; the current Linux layout expects `/usr/bin/python3`, `/usr/sbin/ip`, `/usr/sbin/sysctl` and `/etc/ssl/certs`. Unprivileged user, network and PID namespaces must be enabled by the host. Startup fails if the confinement checks cannot pass; it does not change host namespace policy.

Build the pinned daemon with `python3 tools/build_mdk.py --output /absolute/private/build/wnd`.
Native key storage requires this build's private bootstrap capability and provenance
manifest on both platforms. The historical unpatched daemon is usable only with
explicit `--key-storage files`; its old hash does not attest the new capability.
Install `target/debug/marmish-keychain` at a stable private executable path on PATH
(or alongside the Python entry point), or pass `--keychain-helper /absolute/path`.
On Linux, native storage uses an already initialized persistent Secret Service
collection. macOS uses the user's default legacy Keychain.
Set these variables on each endpoint, choosing your relay and local daemon path. Repeat `--relay` for additional explicit URLs. Supported URLs are `wss://` DNS origins with an optional port and root path; there are no built-in public relays.

```sh
WND=/absolute/path/to/wnd
WND_SHA256=PASTE_SHA256_FROM_WND_MANIFEST
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

## Local key storage

New profiles default to `--key-storage auto`: use native Keychain on macOS or
Secret Service on Linux when available. Only a proven absent backend selects
private files, with a message on stderr. A locked, denied, uninitialized or failed
keychain stops startup. `--key-storage native` requires native storage;
`--key-storage files` explicitly selects file storage. Existing profiles retain
their backend, and a saved native reference never falls back or regenerates a key.

Use `--key-name work` (or another local name) when creating a native profile. Many
names can coexist. A name remains bound to its original `-i` directory and Nostr
identity; use that same directory to reopen it. A different profile cannot adopt
that name or copy its MLS state. Without a name, Marmish generates a unique name.
The private catalog is `$XDG_CONFIG_HOME/marmish/keys.json` on Linux (default
`~/.config/marmish/keys.json`) or
`~/Library/Application Support/Marmish/keys.json` on macOS. `--key-catalog PATH`
selects an isolated catalog. Catalog and profile bindings contain public metadata
and immutable random references, never the private keys. SQLCipher masters are
separate random keys per engine root, independent of the Nostr key name.

To migrate an existing profile, stop its frontend and wait for its commands to
finish. Repeat its usual client or server invocation with
`--migrate-keys-to-native`; server migration needs its usual bridge, worker,
allowlist and cwd options. Migration preserves the Nostr identity, MLS rooms and
SQLCipher master, verifies native readback, commits references, then removes the
legacy key files. It refuses active workers and resumes interrupted cleanup on an
explicit retry. Keep the same arguments and profile; do not delete the journal or
keys to repair a failed migration. Unlink does not guarantee forensic erasure. Migration requires an initialized encrypted database and all retained jobs to
be in the `exited` state. A `lost` or `unknown_launch` record refuses migration;
there is currently no automatic acknowledgement or repair for such records.

If fresh provisioning stops after reserving a reference but before creating its
secret, restart fails closed rather than generating a different key. Preserve
the pending journal for diagnosis; no automatic reset command is provided. A
missing helper also stops auto selection (`key_helper_required`), because Marmish
cannot establish that the native backend is absent without the helper.

Detached workers receive keys through a bounded private pipe and keep running if
the keychain locks or a frontend disconnects. A new frontend needs access to the
same native entries to reopen state. Keep a separate secure key backup: losing a
SQLCipher master loses the encrypted state. Keys pass through memory; this is not
protection from the same Unix account or authorized arbitrary commands. Rebuilt
macOS helpers may require renewed ACL authorization; install helpers at a stable
location. Native macOS execution remains a pending validation gate.

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
it applies the maintained private-bootstrap patch on both platforms and the
restricted-transport patch on macOS, builds the native
helpers and signs the executables locally. It requires Rust 1.97.1, Python 3.11+
and a native C toolchain. macOS additionally requires APFS, macOS 15 or newer,
`sandbox-exec` and `codesign`. The manifest is `<wnd>.manifest.json`; macOS startup
requires and verifies it, as does native key storage on Linux, with an optional
explicit `--wnd-manifest PATH`.
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

The engine encrypts its state database and retained stdout/stderr with SQLCipher. The standalone file-backed engine API creates a private `engine.key`; the frontends instead supply the master from native storage when selected. Detached workers receive it over private stdin and do not reopen the keychain. Keep this key safe: losing it loses the stored data. For an encrypted-data-only backup, exclude `engine.key` and protect a separate key backup. Copying both the data and key provides no protection against whoever obtains that copy. Encryption does not protect against the same Unix account, child-created files, execution memory, swap or core dumps. Job names, file sizes, timestamps and execution-authority tokens remain outside content encryption. Output quotas count logical bytes; each stream database allows about twice its logical cap plus 256 KiB, with additional bounded rollback-journal space during writes.

This unreleased storage format rejects older plaintext roots intact. Finish existing plaintext workers and preserve their old state directory before selecting a fresh encrypted root. There is no automatic migration, deletion or key replacement. Missing, malformed or incorrect keys fail closed; do not remove a key to reset a root containing jobs. See [the reviewed storage design](docs/phases/sqlcipher-storage.md) for recovery rules and limits.

The separately built production/fault-worker test matrix is documented in [the phase 1 design](docs/phases/phase-1.md#fault-injection-build-and-acceptance-matrix). Fault injection is off by default and must remain disabled in production builds.
