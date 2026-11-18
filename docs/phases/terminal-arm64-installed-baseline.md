# Local same-source installed Terminal baseline

Bead `marmish-dwt.1.5`. This is the callable installer and native Terminal
subset of the [cross-platform plan](interactive-peers-cross-platform-qualification.md).
It covers Linux ARM64 and macOS ARM64 on their own machines. Linux AMD64 is an
optional control on the same interface. Each host runs both fixture endpoints
over private loopback WSS. The stock installed foreground daemon independently
tests startup against a held, non-listening loopback socket.

## Entry point and inputs

Add an explicit `--build-installed-terminal` mode to
`tools/marmish_local_qualification.py`. It requires an exact full source SHA,
clean trusted checkout, and a resolved native tmux executable. Reuse existing
checkout, version/manifest, one-target installer rendering, local assets, PTY
quickstart, launcher, provenance and process checks. The new mode derives the
version from the produced manifest after checking the ordinary Cargo package
version and producer contract. It owns its package directory and excludes the
old assets, terminal, and live-relay mode arguments. Require a trusted
`--licenses` file previously collected for the exact candidate lock using
`tools/marmish_native_licenses.py` or the equally vetted package step. Record
Cargo.lock and license input SHA-256. The license collector can need
external dependency license downloads during separate input preparation.
The qualification command itself uses offline Cargo and no external relay or
asset requests. Its trusted-source assumption includes local Cargo configuration
and pre-populated locked caches. Python remains test tooling only.

## Package and install

Refuse root and any mismatch between native OS/machine and the pinned Rust
compiler host. Refuse Rosetta and cross compilation. Require tmux 3.3 or newer
on the exact stock lookup used by the product, with the same executable and
ancestor trust checks and a sanitized version in the report. Require the
current checkout Rust pin and enough free native build space before work.
Use one mode-0700 attempt directory outside the checkout with a fresh Cargo
target, no shared compiled objects, explicit two-job offline locked builds,
and a fixed build stamp matching the original and final clean HEAD. Do not
inherit target, compiler wrappers, cross-compilation overrides, Cargo flags,
TLS/relay variables, or build profiles. Preserve execution markers when set
without reporting their values. Keep the approved cache available through
Cargo home. Select release build payloads from their new target. Build
`marmish-release-tool` through the existing locked package build.

Copy the three executable payloads to the private package inputs. On macOS
set `MACOSX_DEPLOYMENT_TARGET=15.0`, ad hoc sign the copies and verify strict
signatures and ad hoc identity before hashing. Use the existing native ABI
inspection before packaging with the fixed `none/glibc=2.39` or
`adhoc/macos=15.0` contract. An incompatible native ABI is a typed block, even
if the host would run the binary. Use `marmish-release-tool package` unchanged.
Record final package and three verified installed payload SHA-256 values.
Render unchanged `scripts/install-native.sh` for the one target and run
existing local quickstart, repeat setup and stock foreground failure checks.
Use file credentials and the held explicit loopback URL. Exercise a
same-size tampered payload before install and repeat that refusal against the
fresh installation after snapshotting its manifest, launchers and profile
tree. In the second rejection compare the snapshot again. Do not read secret
state into the report.

## Exact private fixture

After local installation, copy `marmish`, `marmishd` and `marmish-worker` from
the verified version directory into an owned sibling shadow. Compare each
shadow to its checked manifest digest before and after the fixture. Compile
the same-SHA native libtest controller once, offline with Cargo JSON in the
private target. Require exactly one selected test executable from the
same workspace package, named `marmish_native`, marked test, located inside
the new target's debug deps. Copy that controller under the shadow's
`deps/` directory, because the fixture resolves all payloads as siblings of
that directory. Preserve the packaged install tree and macOS signatures.
Run this exact selector once with `--ignored --exact --test-threads=1`.

```text
mdk_adapter::terminal_route_proof_tests::terminal_command_fixture::terminal_installed_runtime_tests::foreground::terminal_installed_runtime_native_controls
```

The test and its mount compile in the Unix native libtest on Linux and macOS.
Individual termios field checks are platform-gated. The fixture uses the
actual installed CLI, packaged executable path validation and actual native
PTY worker/tmux, plus in-process test-owned issuer, transport and daemon
runtime with a fresh fixture CA trusted only inside that runtime. It observes
ordinary Command, literal Unicode output, resize, Ctrl-C, detach/resume of the
same shell and terminal restoration. The separate real stock daemon invocation
must return the expected typed `relay_connect` and remove its runtime.
The controls fixture does not exercise stock-to-stock private TLS readiness,
Observe/Takeover or every selection case of the broader plan. These may run
under their existing exact selectors as separately labelled qualification.

## Bounds, cleanup and evidence

Use the existing bounded process supervisor. Cap builds and qualification
with one 90-minute monotonic attempt deadline and cap installed plus runtime
checks with a 20-minute sub-deadline. Process-level timeout is limited by the
remaining budget. The test-only private runtime can accept a short temporary
root allocated by the harness. Its default stays unchanged. This allows the
harness a precise fallback after timeout/interruption: enumerate only
owner-held expected tmux sockets in this run's fixture tree, kill by exact
socket using the checked tmux path, verify sockets are gone, and remove the
attempt directories. Do not enumerate host sessions, scavenge by PID or remove
someone else's socket. Reap process groups, including installer PTY children,
before declaring cleanup. Bound capture and error output. Interrupts from
SIGINT/SIGTERM take the same cleanup path; uncatchable termination can leave
private artifacts and is not clean evidence.

Emit a single fixed-shape sanitized JSON report. Include target, source SHA,
Cargo.lock/license/package/installed-file digests, pinned toolchain and tmux
versions, bounded case/stage labels, exact executed count, durations and cleanup
outcome. Keep recognizable libtest one-case failures, unexpected count,
preflight or build blocks, timeouts and cleanup failures actionable without
passing through raw stdout/stderr, paths, relay data, key and profile details
or PTY contents. Recheck exact clean source before returning pass. Unit tests
cover new option isolation, strict cargo selection, fixed failure categories,
manifest/signing parameters and cleanup dispatch. An actual Linux AMD64 run
may qualify the interface if the exact source and build infrastructure are
available locally. Only an actual run on each ARM64 host can qualify that host.

No workflow is added or changed by this phase. An optional self-hosted matrix
depends on explicit protected-ref admission, uses one job at a time, and
requires separate approval before scheduling. This phase creates no release,
makes no Peer assertion, and adds no networking or TLS trust to installed
Marmish.
