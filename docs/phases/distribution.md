# Prototype distribution and publication

## Scope and phase boundaries

The user authorizes public release assets, a public Radicle mirror, and a simple
installer. This initiative uses the script option, rather than publishing crates
which do not install the application. The Python frontends currently have no
Cargo CLI package, and downstream Cargo does not inherit the workspace's pinned
SQLCipher patch. Cargo publication remains future work; the Actions registry
token must not be consumed by an unrelated or incomplete crate.

1. Approve this design, establish the public Radicle repository using the existing
   identity, and verify Iris replication. Do not replace an unavailable signer.
2. Implement complete native runtime bundles and checked user-local installation,
   validate them offline and through installed commands, and obtain Claude code
   review. Commit and push the reviewed increment to GitHub and Radicle.
3. Add reviewed release automation, build three native archives from one commit,
   publish an explicitly experimental prerelease and verify installation from
   Iris and published assets. Record each platform's actual checks and limitations.

The requested initial targets are Linux x86_64, Linux aarch64, and macOS aarch64,
matching available native runners. Intel macOS is not included or claimed.
Linux ARM64 confinement is currently blocked by runner namespace policy; macOS
confinement has an unresolved broker SIGABRT. Packaging/build success cannot
upgrade these platforms to runtime-validated support. Linux x86_64 installed
end-to-end command execution is a required gate. Existing native validation
gates remain intact; build-only packaging must not disable runtime confinement.

## Runtime package interface

Each versioned tar.gz archive contains one version/target directory with bin/
launchers, tools/ Python runtime modules, libexec/marmish/ native executables
(wnd, its manifest, worker, bridge, keychain and Darwin helpers), patches/mdk/
provenance inputs, the relay canonicalization fixture needed by verification,
project LICENSE, dependency notices, and release.json. Never include fixture
executables, secret identities, command state, caches or CI credentials.

Use a deterministic explicit file allowlist. release.json records format version,
application version, target, source commit, file digests, runtime prerequisites
and experimental validation limitations. Verify daemon provenance using the
existing verifier. Build with locked Rust dependencies, the pinned clean MDK
builder and its exact bootstrap tests. Record native shared-library dependencies.
Collect upstream license texts/notices from the resolved build dependencies;
do not infer redistribution compliance from a crate's name alone.

The installed launchers locate assets relative to themselves and invoke Python
3.11+ using argv arrays, including prefixes containing spaces. Supply bundled
wnd/hash/manifest and server bridge/worker defaults without changing explicit
user options. Keep relay, identity, authorization and cwd choices explicit.
Native keychain helper discovery uses the installed executable. Do not add root,
privilege, public-relay defaults or insecure fallback to make a demo succeed.

## Installer contract and trust boundary

scripts/install.sh is a small POSIX entrypoint using Python 3.11+ for bounded HTTPS
downloads, SHA256 verification, strict archive validation and staged installation.
Its companion installer logic must be self-contained or fetched from the same
immutable source revision; no unverified mutable secondary script execution.
Default entry URL is https://iris.radicle.network/raw/<RID>/head/scripts/install.sh;
documentation also gives a commit-pinned URL. Iris accepts head or a full commit
OID, not an arbitrary branch/tag in that segment. Release binaries stay in GitHub
Releases because Iris raw blobs have a 10 MiB limit.

Resolve the latest experimental release through the GitHub API, or accept an
explicit version. Download that release's target archive and SHA256SUMS via HTTPS.
Checksums detect corruption; they are not independent signatures or reproducible
build proof. No arbitrary download URL configuration is required for production.
Bound response and extraction sizes, reject traversal, absolute names, symlinks,
hardlinks, duplicate paths, special files, mismatched target/version and unexpected
members; verify every payload against release.json. Preserve exact executable
and private-directory modes. Downloads or checks must finish before publication.

Default to a private user-owned prefix under ~/.local/share/marmish, with launchers
in ~/.local/bin, and support an explicit absolute --prefix. Refuse effective root,
unsafe existing destinations and replacing unrelated files. Use immutable
version/target directories and an atomic current selector; refuse version
collisions with different bytes. Reinstallation is idempotent. Never modify
identity/catalog/keychain state or shell profiles. Clearly print PATH guidance.
Failures must preserve the previous installation and clean only owned staging.

## Release and mirror automation

Use trusted manual workflow dispatch from master with a strict version input.
Build on the native epiphytic-ci Linux X64 bel-ci, Linux ARM64 laptop-ci and
macOS ARM64 laptop-ci runners; laptop builds serialize. Jobs use clean checkout,
isolated source/staging locations, verified trusted build caches, bounded execution and least permissions.
Do not pass publication credentials to compilers, tests or build subprocesses.
Publish only after all three archives and checks pass. Publication uses an
explicit experimental prerelease tag and checksums; do not overwrite an existing
release/version. Include clear per-platform build/runtime status in release notes.

Radicle initialization uses master (the actual repository default), public
visibility, existing signing identity and existing node. Record RID and add rad
remote. Explicitly synchronize to Iris, verify repository metadata and exact raw
committed bytes before claiming availability. GitHub and Radicle are both source
homes; README lists both. AGENTS requires pushing changes and release tags to
both and verifying remote refs/seed visibility; if one fails, record and report
the partial publication rather than declaring synchronization. No secret node
keys, passphrases or tokens enter git, logs, archives or documentation.

## Acceptance checks

Offline tests cover package closure and provenance, launcher argv/override
handling, spaces and special characters in install paths, version/target parsing,
checksum corruption, archive traversal/links/duplicates/size limits, unsafe
prefixes, idempotence and rollback. Installer help and both installed command
helps run from a fresh prefix without the source checkout on PYTHONPATH.

Native jobs build guarded MDK, production Rust executables, licensing records and
the exact complete archives; inspect dynamic dependencies and run installed
offline smoke checks. On Linux x86_64, execute disposable client/server commands
through Marmot with the installed distribution, explicit relays and byte/special
character assertions. Preserve owned-state cleanup and sanitized evidence.
Fetch the installer from Iris and verify bytes against git; fetch published
assets and verify SHA256 and a fresh installation. Store independent Claude
plan/code reviews and validation evidence under docs/reviews and docs/validation.
Use Beads for execution tracking and review dispositions.

## Reviewed contract refinements

These refinements supersede any less-specific wording above.

- The version-root tree is exactly bin/{marmish,marmishd}, tools/ runtime Python
  modules, libexec/marmish/{wnd,wnd.manifest.json,marmish-worker,
  marmish-engine-bridge,marmish-keychain}, patches/mdk/{marmish-bootstrap.patch,
  marmish-restricted.patch,source-guard.json},
  tests/fixtures/relay-canonicalization.json, release.json, LICENSE and licenses/.
  Darwin adds libexec/marmish/{marmish-lifetime,marmish-probe}. The package builder
  runs verify_manifest against the staged tree, not just the original build.
- Add an installed-layout resolver in marmish_app.py: parser defaults replace
  required=True only for a recognized installed tree. Resolve the version root
  once to its immutable physical path. Native helper defaults are absolute; no
  PATH fallback is used for the default installed helper. Explicit options retain
  argparse semantics, including --opt=value and abbreviations. Test the remote
  command divider and a decoy helper earlier in PATH.
- Bundle bin launchers invoke python3 -I -B with an explicit controlled tools
  import directory and argv arrays. Do not import from CWD/PYTHONPATH, and never
  write bytecode into the installation. Prefix bin launchers have a marker and
  safely quoted immutable absolute version-bin paths. Installation stages both
  launchers, validates existing marker ownership and can roll back replacements.
  Existing processes retain immutable old paths; keep old versions, do not delete
  running executables. No automatic state migration or downgrade is performed;
  profile/schema incompatibility fails according to existing frontends.
- A read-only --doctor reports interpreter and platform prerequisites. Linux
  checks include /usr/bin/python3 (also >=3.11), bwrap, unshare, /usr/sbin/ip,
  /usr/sbin/sysctl, /etc/ssl/certs and /etc/nsswitch.conf. Darwin requires macOS15+,
  sandbox-exec, codesign and ifconfig. Static prerequisite findings do not prove
  confinement works. Missing optional keychain service is not probed by help or
  doctor; the runtime retains its existing auto-provider rules.
- scripts/install.sh embeds all installer Python; no second executable fetch.
  List releases and choose the first non-draft matching experimental version,
  rather than /releases/latest (which omits prereleases). Explicit --version is
  supported. Download via HTTPS only with bounded time/size and redirect hosts
  api.github.com, github.com, release-assets.githubusercontent.com and
  objects.githubusercontent.com. Ignore ambient proxy configuration; provide
  fixed actionable network/rate-limit errors without leaking response secrets.
  Mutable Iris head means trusting the current served script. Also document the
  immutable source-commit URL. Same-release checksums are integrity checks only.
- Use a bounded gzip decoder and strict USTAR reader/manual extraction. Enforce
  member/count/decompressed-size/path-length limits and an ASCII canonical file
  allowlist (so case/Unicode collisions cannot be smuggled in). Reject PAX/GNU
  extensions, sparse entries, links, unsafe mode bits, duplicate members and
  nonzero trailing tar/gzip data. Create regular files exclusively in a newly
  owned staging tree, reject all existing symlink prefix components and never
  extract through externally supplied directories. Validate release.json fields,
  exact payload set, all digests and expected modes. No tarfile.extractall.
- Provide --uninstall for marker-owned launchers/selector only, retaining old
  version trees and all user state. No arbitrary recursive deletion is required.
- Record minimum glibc required by the actual ELF symbols in each Linux build,
  and check it on installation. Do not advertise a distro floor below the measured
  requirement. Record external shared libraries and their availability; SQLCipher
  and OpenSSL remain vendored. macOS builds are ad-hoc signed, not notarized;
  upgrades may prompt for Keychain access again or fail closed if access is
  denied. Do not bypass Gatekeeper, disable quarantine or change keychain policy.
  Verify code signatures after extraction on the native build runner.
- License collection covers both exact dependency graphs/lockfiles: Marmish and
  patched pinned MDK. Collect full root and vendored LICENSE/COPYING/NOTICE/
  COPYRIGHT texts from resolved source trees, recording package/source/digests.
  Explicitly include SQLCipher, nested OpenSSL and secp256k1 notices, plus MDK's
  original MIT license (2024–2026 Internet Privacy Foundation). Retain MPL source
  availability and original pinned source/revision references where applicable.
  Missing license texts/unknown metadata fail packaging until reviewed; do not
  silently ignore copyleft obligations. The optional attribution question received
  no preference during implementation; use Marmish contributors as communicated,
  preserving the repository's existing MIT declaration.
- Before public Radicle initialization, scan all reachable Git history for
  credential patterns and audit tracked private-state paths and publication
  inputs. GitHub is already public; existing author email, public repository
  history and tracked sanitized Beads/review evidence are within the user's
  explicit mirror-publication request. Do not demand a second authorization to
  mirror that existing public history, and do not rewrite history. A discovered
  actual credential is a concrete blocker requiring remediation.
- Update AGENTS.md, README and docs/agent-spec.md as deliverables. Existing
  general statements withholding release authority are superseded for this
  initiative by the user's explicit release and public Radicle request recorded
  in Beads marmish-ic3 (2026-10-29). Future unrelated releases retain their normal
  authorization requirements. Build jobs receive no publication credentials. Shared native toolchain/Cargo caches are restored only in trusted master jobs serialized with native validation; source and staging remain isolated and locked build identities are checked.
- Publish requires build/provenance/offline installation success on all three
  targets and installed Linux x86_64 real-Marmot execution. Do not require the
  known-blocked ARM/macOS runtime gates to pass as packaging checks or suggest
  they passed. release.json, installer output and release notes name experimental
  status and the exact target runtime limitation. No Intel macOS artifact is
  implied by macOS ARM64 support.
