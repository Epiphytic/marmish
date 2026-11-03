# Marmish

Run remote commands over Marmot's MLS-encrypted Nostr transport. Marmish v0.2
uses native Rust clients and daemons, with one named Nostr identity per daemon.
The same identity can send commands and accept authorized commands.

**Initial native 0.2.0 release: macOS full testing is still in progress.**
Product unit tests, native builds and ABI inspection passed on all three release
platforms for baseline source `347cf799ae5baa9e577ded1a00566c7a6b99b933`;
macOS signing also passed. Its macOS installed-tool feature suite passed 426
tests and failed two local benchmark child-process fixtures. Their cause is not
yet proven; fixes and further testing are planned for v0.2.1. All-platform
installed end-to-end, version-lifecycle and real keychain-provider qualification
remain pending. This release does not claim those checks passed.

Source: [GitHub](https://github.com/Epiphytic/marmish) ·
[Radicle](https://radicle.network/nodes/iris.radicle.network/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg)
(`rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg`).


Baseline run [41655210565](https://github.com/Epiphytic/marmish/actions/runs/41655210565)
failed overall; its Linux jobs were cancelled after those product steps passed.

## Install and quickstart

Run as your normal user, from a terminal:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh | sh -s -- --quickstart --debug
```

If Iris is unavailable, the same installer is available independently on GitHub:

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/Epiphytic/marmish/master/scripts/install.sh | sh -s -- --quickstart --debug
```

The installer pins v0.2.0 and verifies its manifest and binary checksums. It does
not select a legacy v0.1 release. The Iris and GitHub URLs are independent script sources; both are intended to
serve the same pinned native bootstrap after cutover. Versioned binary assets
are downloaded from GitHub Releases. See [Scope and validation](#scope-and-validation) for the evidence boundary. No Python, uv,
Cargo or Rust compiler is needed to install or run it.

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

The native installer also refuses to replace unmanaged launchers. If you installed
v0.1, use its uninstaller or move its old `marmish` and `marmishd` launchers aside
before installing v0.2. Keep legacy profiles separate; they are not adopted.

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

The baseline test results above apply to source `347cf799…`. Release source
`73ed2b41b64ecf614d245668e3ab3b2e5b753895` changes only release-workflow and
initial-release documentation; runtime, build, dependency and scripts trees
are unchanged. Asset-build run 41664164076 attempt 1 completed successfully. Release manifests identify the actual build source; neither source
equality nor earlier tests establish byte-identical binaries or full installed qualification.

Release manifests record the actual asset source and checksums. Source mirror
availability, installer delivery and full installed qualification are separate
observations; publication alone does not establish the pending test results.

## Initial asset verification

Asset-build run [41664164076](https://github.com/Epiphytic/marmish/actions/runs/41664164076),
attempt 1, successfully built, inspected and packaged all three targets and
rendered the native bootstrap from this release source. All 15 product assets
were individually checked against their manifests. This caveated packaging run
skipped product test reruns, installed-test tooling, provider qualification and
quickstart jobs; its success is not full qualification. The exact rendered shell
also installed Linux AMD64 from the artifact asset directory: release metadata
matched source/version/formats `[2,6,1,1,1]`, and all four installed product files
matched their expected hashes and modes. Public release and both installer URL cutovers have been verified. The local installed native protocol check passed, as described below.

The installed Linux AMD64 direct-relay command check passed (exit 0), using the
installed production owner, engine and worker with disposable identities. Total
elapsed time was 19444 ms. Its running-command owner-restart/recovery case took
6295 ms and verified exact recovered output, execution count one, identical-submit
attachment and remote conflict refusal. Literal argv completed in 1590 ms;
16 KiB binary stdin/output in 2778 ms (5896 output bytes/s); stderr plus exit 7
in 1466 ms. Joined engine/MDK stops, engine quiescence, runtime cleanup and state
cleanup all passed, with no retained state. This is one local installed native
protocol check, not the public-CLI platform matrix, an OS restart test, or full
benchmark/provider/lifecycle qualification.

The [initial v0.2.0 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.0)
is public with all 15 product assets; server sizes and digests match the verified
manifests. GitHub and Radicle tags both identify the release source. Bootstrap-only
commit `7e3e74411bed8657e8b8f5b599b36c2b702fe5bd` changes only `scripts/install.sh`.
Both public installer URLs were independently fetched and matched the rendered
bootstrap SHA256 `46b4f369792f4f4ea36726c24414c5cca2ef977fec61712d821a1b81f985367a`.
The two-source/three-platform HTTPS run [41667214179](https://github.com/Epiphytic/marmish/actions/runs/41667214179), attempt 1, passed its resolver, all six install/setup cells and rollup. This validates both script sources on all three targets within that test scope; it does not establish full command-matrix, provider or version-lifecycle qualification.

A separate Linux AMD64 install downloaded the public release assets without an
asset-directory override and passed (exit 0), selecting active generation 2
with the same version, source and format tuple. See the
[initial release evidence](docs/validation/v0.2/initial-release.md) for exact reports and boundaries.
