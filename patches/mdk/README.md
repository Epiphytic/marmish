# Restricted native transport patch

`marmish-restricted.patch` applies only to pristine MDK revision
`37c248a11239e108afc9bd4ee62b8f60542012e3` from
`https://github.com/liamhelmer-bel/mdk.git`. `tools/build_mdk.py` fetches that
revision into a fresh temporary checkout and verifies the pinned Cargo.lock.
It never copies a sibling working tree. Linux builds remain unpatched.

On macOS the daemon requires a private restricted configuration before app
startup. The patch routes the primary, anonymous-directory, scoped-inspection
and per-account-publish SDK builders through one transport factory. Complete
URL membership is checked before forcing SOCKS; the entire connection future
is timed out. A nonce/digest acknowledgement proves configuration installation.
Raw HTTP export/media/directory clients are disabled in restricted mode, with
Seatbelt as the independent networking boundary.

`check_mdk_builders.py` checks the pinned digest of the complete reviewed
`marmot-app/src` Rust tree, rejecting alternate imports, factories and added
modules. `source-guard.json` binds that digest to the revision and patch. The
build guards and builds release code before copying the shared relay fixtures
outside that Rust tree and running focused Rust tests. Those tests cover exact
URL rejection, total SOCKS timeout, canonicalization and subprocess-isolated
configuration/acknowledgement validation. Darwin manifests record the fixture
digest and ad-hoc signing, and copied executables undergo strict signature
verification. Native compilation, Seatbelt probes and real Marmot tests
remain separate required validation; a successful cross-platform type check
alone does not establish macOS support.
