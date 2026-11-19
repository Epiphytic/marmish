# Local native qualification

Run `tools/marmish_local_qualification.py` as a nonroot operator to check a
single host and a single source commit. The tool requires Python 3 for the test
harness. The installer and installed binaries run with a utility-only `PATH`.

Use a clean checkout and stage that checkout's native binaries with the
existing `marmish-release-tool package` command. Set `MARMISH_BUILD_COMMIT` to the
full source SHA when building with `cargo build --locked --release -p
marmish-native --bins`. Package `marmish`, `marmishd` and `marmish-worker` from
that build with its license inventory and the actual target's minimum runtime.
Use `--signing none` on Linux and sign the staged macOS copies before packaging
with `--signing adhoc`. The staged bundle must stay unchanged during the run.

```sh
python3 tools/marmish_local_qualification.py \
  --source "$SOURCE_CHECKOUT" --commit "$SOURCE_SHA" \
  --version "$VERSION" --assets "$NATIVE_ASSETS"
```

The tool renders the checked-out shell installer with the actual host manifest
digest and fixture-only zero pins for the other targets. It uses `--asset-dir`
throughout and refuses a same-size tampered payload before installing. It checks
fresh file-backed quickstart, idempotent setup, launcher targets, installed
payload digests and native provenance for all three executables. It then starts
the installed foreground daemon with a held, closed loopback WSS endpoint. The
expected result is a typed `relay_connect` startup failure and runtime cleanup.
The report labels live activation `not_exercised` in that mode.

Pass `--live-relay wss://relay.example/` only with authorization to use that
production-trusted TLS relay. This substitutes a positive live foreground
check. The installed CLI must report the fresh profile active, a second daemon
must receive the expected profile lock error, and the first daemon must stop on
SIGTERM. The fresh identity may leave public relay events. This check exercises
relay activation and local status without a remote command or terminal session.

Optionally append `--terminal local --tmux /absolute/path/to/tmux` to run the
legacy and framed controlling PTY render cases from the same source. Use
`--terminal wss` to include writer, read-only observer and named takeover cases
on the private loopback WSS fixture. WSS mode requires the tmux binary selected
by the normal product lookup. These fixture modes use offline locked Cargo
and can compile debug binaries, so prepopulate the dependency cache. The harness
runs each selector serially and checks that it executed exactly once.

The JSON report contains fixed stage results, fixture case names, source SHA,
version, target and elapsed time. When a fixture Cargo command exits nonzero,
`operation_failed` includes a `fixture_exit_code` if it is in `[-128, 255]`.
`fixture_failure_kind` is `selected_test_failed` when libtest appears to report
one selected failure, or `no_recognized_failure_summary` otherwise. This is a
heuristic based on the libtest summary and does not identify the underlying
error. Timeouts and the child output limit have their own categories. The
report omits child output, panic text and disposable identity and state details.
The host-local installer and the native private WSS runtime
fixture produce independent evidence. Cross-host interoperability, Keychain
providers, the three-target release renderer and benchmarking have their own
gates.

## Build, install and run the installed Terminal subset

Use this opt-in mode on Linux ARM64, macOS ARM64, or Linux AMD64 as a
comparison. Use Python 3.11 or later, the pinned native Rust toolchain, the
native build prerequisites and tmux 3.3 or later from the stock lookup. The
host needs space for a fresh release build and libtest target in a private
directory under the operator's home. Collect the license inventory from the
same lockfile, then run with the locked Cargo dependencies already cached.

```sh
python3 tools/marmish_local_qualification.py \
  --source "$SOURCE_CHECKOUT" --commit "$SOURCE_SHA" \
  --build-installed-terminal --licenses "$NATIVE_LICENSE_INVENTORY" \
  --tmux "$STOCK_TMUX_ABSOLUTE_PATH"
```

This mode builds and packages the locked checkout, inspects its native ABI,
checks the local installer and a rejected tampered replacement, then exercises
digest-identical copies of the installed executables. The selected native
controller uses one private loopback WSS fixture to check ordinary Command,
literal Unicode output, terminal resize, Ctrl-C, detach/resume and PTY
restoration. The server shell comes from the test account's normal login shell.
The independent stock foreground-daemon check expects typed closure on a held,
non-listening loopback endpoint.

The JSON distinguishes the installed subset from the broader Terminal suite,
stock-to-stock private WSS, cross-host runs, Observe/Takeover and Peer. Those
are all marked `unrun` by this mode. Linux packaging enforces glibc 2.39 or
earlier dependencies. macOS packaging requires a 15.0 deployment target and
strictly verifies ad hoc signatures. The fixture and packaging commands make
no public relay requests. Run on each ARM64 platform to qualify that platform.
[Phase plan](../phases/terminal-arm64-installed-baseline.md).

### Linux AMD64 author control

For `bc39ea06f9503060d0db1abbb86e63de93c3d27b`, the author built the
native payloads and exact fixture controller with locked offline Cargo in a
warm debug target. The real package producer, installer and installed fixture
steps passed in 77.468 seconds after compilation. The one selected Terminal
controls case took 30.764 seconds. This run passed the glibc 2.39 inspection,
fresh quickstart, idempotent setup, manifest and byte provenance, both
same-size tamper refusals, the stock foreground closed-relay refusal and
explicit private IPC, tmux and runtime cleanup. The installed fixture used
digest-identical shadow payloads and a private loopback relay.

The separate clean-target release command passed preflight and entered the
build before the author stopped it to preserve build resources. The author
control did not exercise a clean optimized build or either ARM64 host.
[Claude review and failure disposition](../reviews/terminal-arm64-installed-baseline.md).

## Observed Linux x86-64 result

The artifact-only run used debug binaries from
`3721bf52aff085de9203d31b75d3f8f76ca45967` packaged as 0.3.1. It passed in
33.963 seconds. The installed CLI and daemon reported matching native
provenance, the tampered payload was refused, repeat setup retained the same
identity, and the closed relay startup failure left no private runtime or
test state. This run left the positive live relay and both Terminal fixture
modes unexercised.

A full `wss` run on the same source and package passed in 418.233 seconds
using a dedicated Cargo target directory. The installer and closed-relay
checks passed, then both controlling PTY fixtures and all three private
WSS fixtures ran exactly once. They cover native writer, read-only observer
and named takeover. The native WSS test binaries came from that Cargo
checkout. The installed binary smoke and WSS fixtures are separate checks.
The optional positive installed relay mode remained unexercised in this run.

An earlier run using a shared Cargo target directory completed the installer
and local PTY checks but stopped at the writer selector with
`operation_failed`. The writer passed alone in 70.10 seconds before the
full retry. The first harness report could not identify the cause.

## Observed optimized Linux x86-64 result

A clean checkout of `b02b1895a4374d2bc45dc781341effff0a4d8209` built the
optimized native binaries with Rust 1.97.1. The timed build took 549.25
seconds. All
three executables passed native dependency inspection at glibc 2.39.
The local release tooling packaged them for 0.3.1 with the freshly
collected inventory from the same lockfile.

The artifact-only installer check passed in 12.775 seconds. It verified
quickstart, identity reuse, installed executable provenance, payload
tamper refusal, typed closed-relay daemon refusal, and cleanup of daemon
runtime and test-owned state. This run used local assets and skipped live
activation and Terminal fixtures. The separate private WSS evidence above
comes from the debug build and earlier source revision.

## Installed interactive controls from a clean Linux AMD64 build

Source `7f7d7bc1d71523f0950d7f7ed3647738616b3422` passed the full
`--build-installed-terminal` mode on a local Linux AMD64 host with
glibc 2.39, Rust 1.97.1, and tmux 3.4. The qualification took 843.518 seconds
in a fresh Cargo target after license collection. Optimized release builds
of the CLI, daemon and worker passed the glibc 2.39 package ABI gate. The
run verified installed bytes and quickstart, reused the
identity on repeat setup, refused both same-size tampered payloads, and
checked stock foreground startup refusal against the held loopback socket.
The separately compiled unoptimized libtest controller exercised one case
using digest-identical shadows of the installed payloads. The case took
42.501 seconds over fixture-trusted private loopback WSS. It exercised ordinary
Command, literal Unicode, resize, Ctrl-C, detach/resume, and restoration.
The fixture owner joined and cleanup checks passed. Observe, Takeover, Peer,
stock-to-stock private WSS, cross-host and ARM64 product checks were unrun.
[Structured result](interactive-installed-terminal-amd64.json).

The separate trusted self-hosted inventory on the older master source
`3cef7a798d1da9e435aa4c1fcf68f5138c509d67` passed all three jobs on
2026-11-19. The Linux ARM64 job began 10 minutes 20 seconds after Linux
AMD64 completed, and macOS ARM64 ran next. Linux ARM64 reported Python 3.12.3
and glibc 2.39. Linux AMD64 reported glibc 2.43 on the host and passed the
rootless glibc 2.39 container smoke. The macOS runner resolved `python3` to
3.9.6, below the qualification harness requirement of 3.11. The jobs
ran the preflight capability probe from that master checkout. The feature
commit’s build, test and installer checks remain unrun on the runner pool.
[GitHub Actions run 44592119081](https://github.com/Epiphytic/marmish/actions/runs/44592119081).
