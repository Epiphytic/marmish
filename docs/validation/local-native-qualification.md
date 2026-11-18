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
version, target and elapsed time. It omits child output and disposable identity
and state details. The host-local installer and the native private WSS runtime
fixture produce independent evidence. Cross-host interoperability, Keychain
providers, the three-target release renderer and benchmarking have their own
gates.

## Observed Linux x86-64 result

The artifact-only run used debug binaries from
`3721bf52aff085de9203d31b75d3f8f76ca45967` packaged as 0.3.1. It passed in
33.963 seconds. The installed CLI and daemon reported matching native
provenance, the tampered payload was refused, repeat setup retained the same
identity, and the closed relay startup failure left no private runtime or
test state. This run left the positive live relay and both Terminal fixture
modes unexercised.
