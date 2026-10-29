# Maintained native MDK patches

`marmish-restricted.patch` applies only to pristine MDK revision
`37c248a11239e108afc9bd4ee62b8f60542012e3` from
`https://github.com/liamhelmer-bel/mdk.git`. `tools/build_mdk.py` fetches that
revision into a fresh temporary checkout and verifies the pinned Cargo.lock.
It never copies a sibling working tree. Both platforms apply
`marmish-bootstrap.patch`; macOS then applies `marmish-restricted.patch`.

The bootstrap patch adds `--secret-store marmish-stdin`, a strict one-shot
anonymous stdin frame and an immutable account store for exactly the supplied
Nostr secret/public identity. A mutation-free `MarmishCapabilities` handshake
precedes operational requests and startup reconciliation. Export, replacement
and identity creation are rejected in this mode. Account setup uses the existing
private Login import and setup journal. No host keychain service becomes
accessible to the confined daemon. File mode remains explicit and supported.
Native launch requires a v2 manifest binding the bootstrap patch, source guard
and exact executed bootstrap tests on both platforms.

On macOS the daemon requires a private restricted configuration before app
startup. The patch routes the primary, anonymous-directory, scoped-inspection
and per-account-publish SDK builders through one transport factory. Complete
URL membership is checked before forcing SOCKS; the entire connection future
is timed out. A nonce/digest acknowledgement proves configuration installation.
Raw HTTP export/media/directory clients are disabled in restricted mode, with
Seatbelt as the independent networking boundary.

`check_mdk_builders.py` checks the pinned digest of the complete reviewed
`cli/src`, `marmot-account/src` and `marmot-app/src` trees, including every
regular source/resource file, their three Cargo manifests and Cargo.lock.
Symlinks and special files are rejected. `source-guard.json` v2 binds separate
Linux and macOS digests to the revision and both maintained patches. The
build guards and builds release code before copying the shared relay fixtures
outside those guarded source trees and running focused Rust tests. Those tests cover exact
URL rejection, total SOCKS timeout, canonicalization and subprocess-isolated
configuration/acknowledgement validation. Darwin manifests record the fixture
digest and ad-hoc signing, and copied executables undergo strict signature
verification. Native compilation, Seatbelt probes and real Marmot tests
remain separate required validation; a successful cross-platform type check
alone does not establish macOS support.
