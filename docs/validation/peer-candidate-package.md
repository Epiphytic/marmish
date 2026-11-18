# Private Peer recipient candidate package validation

Source `b9ce82c8cd5eb80346ce2e74e52ec953246c44c9` was built on Linux
AMD64 (glibc 2.39, Rust 1.97.1, upstream MDK `bdf9833e` from the checked
lock). Both stock and private artifacts came from the same clean commit.
The build boundary follows the
[candidate build and read-surface design](../phases/peer-candidate-read-surface.md).

## Reproduction

Build stock and candidate with different target directories. Set the
candidate stamp explicitly from the clean source checkout and target only
AMD64 Linux GNU.

```bash
test -z "$(git status --porcelain)"
export MARMISH_BUILD_COMMIT="$(git rev-parse HEAD)"
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
CARGO_TARGET_DIR="$stock_target" \
  cargo build --locked --offline -p marmish-native --bins -j2
export CARGO_TARGET_DIR="$private_target"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'
cargo build --locked --offline -p marmish-native --bins \
  --target x86_64-unknown-linux-gnu -j2
for scope in native_manifest::tests:: native_release::tests:: native_install::tests::; do
  cargo test --locked --offline -p marmish-native --lib \
    --target x86_64-unknown-linux-gnu -j2 "$scope"
done
cargo clippy --locked --offline -p marmish-native --all-targets \
  --target x86_64-unknown-linux-gnu -j2 -- -D warnings
```

For the stock tests unset the candidate Rust flags, use the stock target
directory and run the same three selectors with the host target. The stock
`--features installed-checks` check uses `--all-targets -- -D warnings`
for Clippy, plus the `native_manifest::tests::` tests. The standalone
build-script matrix runs the real `build.rs` with Cargo-style environment
values and refuses the missing, zero, short or uppercase commit, Linux
ARM64, macOS ARM64, Linux musl and `installed-checks` cases. The valid case
emits the candidate compile witness. This checks build admission on one
host.

The Linux AMD64 artifact check copied all three executables from each
build. It authenticated each manifest locally using SHA-256 and invoked
the corresponding packaged CLI's native installer with explicit source,
manifest, pin, prefix, launcher and state paths in disposable directories.
For the private package use the candidate-built `marmish-release-tool`
and add `--peer-receiver-candidate` to `package`. The private stamp and
manifest class supply no authority to open a Peer route. Public release
and distribution tooling stays stock-only.

## Results

| Check | Result |
| --- | --- |
| Stock manifest, packager and installer | 34 passed (4, 6 and 24) |
| Private candidate manifest, packager and installer | 32 passed (4, 4 and 24) |
| Stock manifest with `installed-checks` | 4 passed |
| Standalone build-script admission | 9 cases passed |
| Strict all-target Clippy in both builds, formatting and diff checks | Passed |
| Real executable package/install matrix | Passed in 116.65 seconds |

The executable matrix installed both artifact classes, checked all six
real release-info responses against the exact build, and rejected six
mixed-binary packages. The two packagers and the two native installers
refused the opposite class. Direct install and uninstall requests against
occupied prefixes refused the opposite class before a transaction and
preserved the existing bytes and launchers in both directions. The public
bootstrap rejected the private manifest as `manifest_grammar` and the
candidate release tool refused both public render and tooling commands.
One same-size worker mutation failed with `install_digest` and left no
installation prefix.

This increment changes build and installer classification only. The
private candidate still lacks the composed installed Peer owner and its
List/Show surface. No Peer socket, admission, approval, relay, command or
heartbeat behavior was exercised by these checks. Both installers were
given isolated homes and prefixes for positive installation.
