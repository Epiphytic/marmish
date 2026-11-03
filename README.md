# Marmish

Run remote commands over Marmot's MLS-encrypted Nostr transport. Marmish v0.2
uses native Rust clients and daemons, with one named Nostr identity per daemon.
The same identity can send commands and accept authorized commands.

**Native v0.2.1 corrective prerelease is available.** It adds automatic upgrades
of recognized v0.1 launchers and fixes credential-helper selection. All three
platforms passed native verification, packaging and file-backed quickstart checks;
both Linux real-provider checks passed. macOS real Keychain qualification was
not invoked because its supervisor tests failed. Full cross-platform command,
provider and lifecycle qualification remains incomplete. This corrective release
was explicitly authorized for out-of-band macOS testing.

Source: [GitHub](https://github.com/Epiphytic/marmish) ·
[Radicle](https://radicle.network/nodes/iris.radicle.network/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg)
(`rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg`).


## Install and quickstart

Run as your normal user, from a terminal:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh | sh -s -- --quickstart --debug
```

If Iris is unavailable, the same installer is available independently on GitHub:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/Epiphytic/marmish/master/scripts/install.sh | sh -s -- --quickstart --debug
```

The installer pins v0.2.1 and verifies its manifest and binary checksums. It does
not select a legacy v0.1 release. The Iris and GitHub URLs are independent script sources; both were verified to
serve the same pinned native bootstrap. Versioned binary assets
are downloaded from GitHub Releases. See [Scope and validation](#scope-and-validation) for the evidence boundary. No Python, uv,
Cargo or Rust compiler is needed to install or run it.

The native 0.2.1 installer adds automatic upgrades of recognized v0.1 installer
launchers. It verifies the old marker and exact wrappers, replaces the launchers,
and retains the old runtime and launcher backups; private state is unchanged.
Unrelated programs are refused rather than overwritten. Interrupted upgrades
recover on the next installer invocation. The public bootstrap above selects the matching 0.2.1 assets.

Supported binaries are Linux AMD64 and ARM64 (glibc 2.39 or newer), and macOS 15
or newer on Apple Silicon. Use a native ARM64 terminal on macOS. The macOS
binaries are ad-hoc signed, not notarized; upgrades may require renewed Keychain
approval. Linux musl and Intel macOS binaries are not provided.

Quickstart creates the `default` identity and asks for:

- A role: enter `c`, `s` or `b`, then Return. Return alone selects **both**.
- Your explicit relay URL(s), separated by spaces. No public relay is selected automatically.
- Tor routing (`auto`, `always` or `off`): `always` routes all relays through Tor;
  `off` refuses onion relays. Return selects experimental `auto`, which
  connects directly to non-onion relays. Tor remains
  experimental and is not a v0.2 release qualification claim.
- Client npubs allowed to execute here. Leave this empty to disable inbound
  execution, even when the role is **both**.
- For client or both roles, trusted remote server npubs, separately from inbound client permissions.

Only grant identities you trust to execute programs as your local Unix user.
The initial command policy permits executable paths broadly; configure path
restrictions in the private configuration before granting more limited access;
see [command policy](docs/phases/command-policy.md). Defaults and matching per-key
restrictions are additive; `*` does not cross `/`, while `**` does.

The setup report prints your public `npub`, selected credential backend, and
whether inbound execution is enabled. It does not print your private keys.
Setup initializes local state and exits; it does not leave a server running.

Launchers are installed in `~/.local/bin`. If necessary, add that directory to
your shell's PATH. On a machine accepting commands, start the daemon and leave
it running:

```sh
marmishd
```

This runs in the foreground using the configured `default` identity. On a client,
the first command starts its local daemon automatically if needed. Configure
service supervision separately if you want the server to start at login or boot.

## Connect to another identity

Exchange the npubs printed by setup. On the server, grant the client's npub; on
the client, trust the server's npub. These are independent permissions. For an
existing configuration, stop its running daemon before changing setup:

```sh
# On the server:
marmish setup --update --grant CLIENT_NPUB
marmishd

# On the client:
marmish setup --update --trust SERVER_NPUB
marmish SERVER_NPUB@relay.example -- /usr/bin/printf '%s\n' 'Hello from Marmish'
```

Replace `CLIENT_NPUB` and `SERVER_NPUB` with complete `npub1…` values and
`relay.example` with your configured relay hostname. The relay selection must
match the profile's complete configured relay set. A bare hostname means
`wss://hostname/`; a full relay URL can include a path. For multiple configured
relays, use `marmish -u SERVER_NPUB -- /absolute/program` to select the full
configured set. Repeated `--relay` URLs may explicitly restate that exact set.

The npub is the remote identity, not a Unix username; the host is a relay, not
the remote machine. Always include `--` before an absolute executable path.
Arguments are sent as separate literal arguments. Shell expansion and pipelines
are not implicitly evaluated remotely. Pass `--stdin` to send up to 16 KiB of
immutable input from stdin, or `--stdin-none` to send none. By default, piped
input is read and terminal input is not.

Aliases are configured in the named profile's `config.json`, while its daemon is
stopped. For example, add an entry to its existing `aliases` object:

```json
"box": {"remote": "SERVER_NPUB", "relays": ["wss://relay.example/"]}
```

The alias must include the profile's complete relay set. Then run
`marmish box -- /usr/bin/printf '%s\n' hello`. Explicit DNS SRV selection is also
available with `-u SERVER_NPUB --srv _nostr._tcp.relay.example`; this requires
actual configured SRV records whose resolved relay set matches the profile.
SRV discovery is unavailable for forced Tor and onion routes.

Commands continue after client disconnection or expiration of its wait. The
client prints a durable command handle to stderr for detached or pending work; use `commands`,
`command-status`, `wait`, `output` or `cancel` with an explicit selector to find,
resume or cancel an invocation. `--wait-seconds` changes the observation wait;
it does not cancel the remote job. `--detach` submits without waiting for output.
Piped stdin is captured before submission; `--stdin-none` explicitly sends none.

Ordinary commands reuse a default command room for the remote identity. Add
`--new-session` to a new command to create a separate authenticated MLS room;
retries retain that preparation rather than creating another room. Each profile
has 64 active command-room slots, shared by inbound, default and fresh rooms.
There is currently no room-retirement command, so use the default room for
ordinary work. There is also a lifetime limit of 4096 fresh-room preparation
markers per profile and 4096 lifetime command-room metadata rows. These are separate from command handles, which identify
individual invocations.

## Named identities and key storage

Use `-i NAME` consistently for setup, the daemon and commands:

```sh
marmish -i work setup --interactive
marmishd -i work
```

Each named identity has separate configuration, encrypted state and credential
references. The default `--key-storage auto` uses macOS Keychain or Linux Secret
Service when available. For a new identity in auto mode, an unavailable provider permits private-file storage;
a locked, denied or failing keychain does not silently switch backends. The
chosen backend remains bound to that profile. Use `--key-storage keychain` to
require the keychain, or `--key-storage file` explicitly for a new file-backed
profile. Both the Nostr secret and the SQLCipher master key use that backend.

Default profile roots are `~/.local/share/marmish/v2/NAME` on Linux (or
`$XDG_DATA_HOME/marmish/v2/NAME`) and
`~/Library/Application Support/Marmish/v2/NAME` on macOS. Override the root with
`--state-dir`; keep profiles private and do not commit them. v0.2 uses fresh
native state; legacy v0.1 on-disk profiles are not migrated by the installer.

The installer automatically upgrades recognized v0.1 launchers, retaining old
runtimes and launcher backups. It refuses unrelated launchers. Legacy profiles
remain separate and are not adopted.

For an already configured installation, update the binaries without rerunning
quickstart, then restart the daemon:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh | sh
marmishd --debug
```

Stop the existing daemon before restarting it. Rerunning first-time quickstart
against an existing configuration can report `config_exists`; use `setup --update`
when intentionally changing setup.

## Retention

New room application messages default to a one-day lifetime; configure it when
starting the daemon with `marmishd --retention-seconds 86400`. This is separate
from stored command payloads and output: the profile configuration's
`limits.payload_retention` defaults to 86400 seconds and accepts 1–604800.
Invocation tombstones retain replay facts separately for a fixed 172800 seconds. Relay
expiry is not a guarantee that every relay or backup erases a message.

## Scope and validation

The implemented v0.2 feature set provides noninteractive remote execution, durable command recovery and
explicit cancellation. Interactive terminals, tmux attachment, X11 forwarding,
port forwarding and file transfer are future work. Embedded Arti/Tor support is
experimental; working Tor command execution is deferred to v0.3 and is not
a v0.2 qualification claim.

Inspect the installed version with `marmish --version` and its source/target
metadata with `marmish release-info`. Inspect the default managed installation with:

```sh
marmish install-status --prefix "$HOME/.local/share/marmish-native" --bin-dir "$HOME/.local/bin"
```

Use `marmish --help` for command options.
The old Python distribution's `--doctor` and namespace requirements do not apply
to the native v0.2 distribution.

## Corrective release validation

The [v0.2.1 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.1)
contains 15 verified assets built from source
`7c49c3785eab010bf017c1bb7f03511524312b95`. Candidate
[41698628787](https://github.com/Epiphytic/marmish/actions/runs/41698628787), attempt 1,
passed all three package jobs, render, all three file-backed PTY quickstarts and
both Linux default-auto keychain jobs. macOS passed 351 default and 446 installed
feature tests. Its provider job then failed offline supervisor tests before
invoking real Keychain qualification, leaving the **overall workflow failed**.
The reviewed supervisor correction landed after the immutable release source;
it does not retroactively establish a passing Mac provider result.

The existing whole-candidate-success guard prevents dispatching the six-case
HTTPS qualification for this failed candidate; no guard was bypassed. Both
public installer URLs were independently fetched and matched the rendered
bootstrap. A separate Linux AMD64 install downloaded public release assets
without an asset-directory override and passed in 10,285 ms.

The installed Linux AMD64 direct-relay protocol check passed in 19,882 ms:
running-command recovery 6,856 ms (one execution), literal argv 1,545 ms,
16 KiB binary stdin/output 2,689 ms (6,092 output B/s), and stderr/exit 7 in
1,713 ms. Joined engine/MDK stops, quiescence and local runtime/state cleanup
passed. The probe reports `purpose_ownership_unknown=true`; this does not prove
automatic probe-room deletion. These are single observations, not full public-CLI
matrix, OS-restart or benchmark qualification.

See [corrective release evidence](docs/validation/v0.2/corrective-release-0.2.1.md)
for source, tag, bootstrap and finite reports. Historical
[v0.2.0 results](docs/validation/v0.2/initial-release.md) remain separately labeled.
