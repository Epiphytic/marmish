# Marmish

Run remote commands over Marmot's MLS-encrypted Nostr transport. Marmish v0.2
uses native Rust clients and daemons, with one named Nostr identity per daemon.
The same identity can send commands and accept authorized commands.

**Native v0.2.3 is available.** It includes background daemon startup, server
PATH resolution and distinct command lookup, policy and launch errors. All three
platforms passed native verification, packaging, installed tests, provider checks
and file-backed quickstart checks. Both public installer sources passed the
six-cell HTTPS install/setup matrix. Interactive macOS Keychain consent and
recovery of the reported user denial remain unverified; full nine-pair remote
command and version-lifecycle qualification remain incomplete. Tor is experimental.
See the [current release evidence](docs/validation/native-automatic-release.md).

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

The installer pins v0.2.3 and verifies its manifest and binary checksums. It does
not select a legacy v0.1 release. The Iris and GitHub URLs are independent script sources; both were verified to
serve the same pinned native bootstrap. Versioned binary assets
are downloaded from GitHub Releases. See [Scope and validation](#scope-and-validation) for the evidence boundary. No Python, uv,
Cargo or Rust compiler is needed to install or run it.

The native 0.2.1 installer adds automatic upgrades of recognized v0.1 installer
launchers. It verifies the old marker and exact wrappers, replaces the launchers,
and retains the old runtime and launcher backups; private state is unchanged.
Unrelated programs are refused rather than overwritten. Interrupted upgrades
recover on the next installer invocation. The public bootstrap above selects the matching 0.2.3 assets.

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
your shell's PATH. On a machine accepting commands, start the daemon:

```sh
marmishd
```

Version 0.2.3 uses the configured `default` identity and detaches by
default. Successful startup is quiet and waits for credentials, storage, IPC and
transport readiness; startup errors return to the caller's terminal. Normal
output appends to the identity profile's private `daemon.log` (mode 0600), with
no automatic rotation. With default paths, logs are under
`~/.local/share/marmish/v2/<identity>/daemon.log` on Linux (or
`$XDG_DATA_HOME/marmish/v2/<identity>/daemon.log`) and
`~/Library/Application Support/Marmish/v2/<identity>/daemon.log` on macOS.
Stop the daemon before rotating its log. Use
`marmishd --foreground` for attached diagnostics or service supervision.

On a client, the first command starts its local daemon automatically if needed.
Client autostart has a 300-second startup budget, including failed-start
cleanup, independent of command `--wait-seconds`. If the launching parent or
client dies before acknowledging readiness, the new unacknowledged daemon shuts
down; an acknowledged daemon survives the caller. This does not install a login
or boot service.

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
the remote machine. Always include `--` before the command.
Arguments are sent as separate literal arguments. Shell expansion and pipelines
are not implicitly evaluated remotely. Pass `--stdin` to send up to 16 KiB of
immutable input from stdin, or `--stdin-none` to send none. By default, piped
input is read and terminal input is not.

Version 0.2.3 also accepts short command names, for example
`marmish SERVER_NPUB@relay.example -- printf '%s\n' hello`. The server resolves
these using the PATH inherited by its running daemon. Restart `marmishd` after
changing that PATH. Absolute paths, cwd-relative paths and executable symlinks
also work; command policy checks the resolved target, with deny rules also
checking its selected alias path. Remaining arguments are passed literally.
Remote children retain their minimal environment (PATH=/usr/bin:/bin).

A missing command reports `command_not_found`; filesystem permissions or file
type failures report `command_not_executable`; lookup and policy evaluation have
separate errors. `command_denied` means the server's execution policy refused it.
`command_launch_failed` identifies a known process-start failure and explains its
finite reason. Executable text needs a valid interpreter header; it is not
implicitly retried through a shell. Use `--debug` on both ends for stage/reason diagnostics; background
server diagnostics go to its private `daemon.log`. Repairing a refused command
requires a new invocation, since retries preserve recorded results.

These lookup/diagnostic changes require both peers to upgrade to v0.2.3.
Older v0.2.2 binaries require canonical absolute executable paths; symlink
spellings such as `/bin` may be refused there. Do not downgrade a profile after
executing commands with v0.2.3. See [the design](docs/phases/native-command-path-errors.md)
for PATH limits, recovery behavior and accepted filesystem-check limitations.

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

For attached debugging, use `marmishd --foreground --debug`. Background debug
output goes to the private profile log.

Stop the existing daemon before restarting it. Rerunning first-time quickstart
against an existing configuration can report `config_exists`; use `setup --update`
when intentionally changing setup.

Version 0.2.2 adds macOS `keychain-authorize` (not available in 0.2.1). If an unlocked Keychain still produces `identity: locked_or_denied`,
stop that profile's daemon and run this from a foreground macOS terminal:

```sh
marmish --debug keychain-authorize
marmishd --debug
```

Use the same `-i NAME` and `--state-dir DIR` on both commands if configured.
macOS may ask permission to read each of the two existing Marmish items. Choose
persistent access in the OS dialog if you want background operation; a one-time
allowance may fail the subsequent check. Marmish verifies both items again in a
fresh process with prompts disabled before reporting success. It preserves the
identity and storage backend, and does not start the daemon or connect to relays.
The command reports finite failure categories and the affected item purpose;
ordinary daemon errors remain less specific. Stop a running daemon first to
avoid `profile_in_use`. Linux does not support this macOS recovery command.
Interactive consent and recovery of the reported user failure still require
real macOS validation; unattended CI cannot approve a macOS consent dialog.

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

## Current release validation

The [v0.2.3 release](https://github.com/Epiphytic/marmish/releases/tag/v0.2.3)
contains 15 verified assets from source
`0d849fe4dd92b447aa16591b168fff70abcd732c`. Candidate
[41926462226](https://github.com/Epiphytic/marmish/actions/runs/41926462226)
passed all 11 jobs, including 388 default and 483 installed-feature native tests
on each platform, provider checks and installed quickstarts. Publisher
[41936735554](https://github.com/Epiphytic/marmish/actions/runs/41936735554)
passed all four jobs and updated the public bootstrap to v0.2.3.

The GitHub and Iris scripts matched byte-for-byte. Public HTTPS run
[41937495649](https://github.com/Epiphytic/marmish/actions/runs/41937495649)
passed all eight jobs: both installer sources across all three platforms,
including install/setup validation and completed cleanup. Source and release tag
were also mirrored to Radicle/Iris. These checks do not establish the full
nine-pair remote command matrix, version lifecycle, interactive macOS consent or
Tor command transport. See [current evidence](docs/validation/native-automatic-release.md).

## Historical v0.2.2 validation

The [v0.2.2 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.2)
contains 15 verified assets built from source
`89107389b9a348b23458cc59b7250499fe1b7b08`. Candidate
[41726568264](https://github.com/Epiphytic/marmish/actions/runs/41726568264), attempt 1,
passed all three packages, render, all three file-backed quickstarts and both Linux
provider checks. macOS passed its 23 offline supervisor tests; its outer provider
qualifier then failed with generic `operation_failed` after 676 ms, importing no
inner report. Whether provider execution began is unknown; this does not establish
a Marmish Keychain denial. The overall candidate remains **FAILURE**, and its
whole-success guard blocks the separate HTTPS workflow. No guard was bypassed.
Both public installer scripts and all 15 anonymous asset downloads matched their
verified hashes. A fresh Linux public-asset install passed.

The installed Linux direct-relay command check exited 0 in 19,496 ms, verifying
running-command recovery, literal argv, 16 KiB binary stdin/output, stderr/exit 7
and reported local cleanup. It also emitted a background transport worker panic
from the unchanged sink path. Independent review judged it nonblocking for this
narrow correction; it is tracked as `marmish-xvd`. Remote final-write/closure
behavior and interactive macOS recovery are not established. See
[v0.2.2 evidence](docs/validation/v0.2.2/corrective-publication.md) for exact results,
latency/goodput and qualification limits. Historical
[v0.2.1](docs/validation/v0.2/corrective-release-0.2.1.md) and
[v0.2.0](docs/validation/v0.2/initial-release.md) evidence remain separately labeled.

## Release process

Releases require an explicit reviewed native Cargo version bump. The new GitHub
workflow runs the full three-platform candidate checks before publishing
immutable assets and updating the installer. It does not infer version numbers
from commit messages. The v0.2.3 candidate and publisher passed hosted
qualification, and the installer selects v0.2.3.

Binary releases are hosted on GitHub. Source branches and tags are also mirrored
to Radicle/Iris separately; GitHub Actions does not publish Radicle releases.
See [the release design](docs/phases/native-automatic-release.md) for gates and
same-source recovery rules.
