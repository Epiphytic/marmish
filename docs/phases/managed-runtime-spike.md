# Increment 0: managed runtime packaging proof

Status: detailed design approved by independent Claude review; implementation in progress.
Beads: `marmish-x6z.1`; parent initiative: `marmish-x6z`.
Scope follows [the reviewed overall proposal](native-setup-unified-daemon.md).
This increment produces a disposable packaging harness and sanitized evidence,
not an installer release, role changes, service IPC or a Rust rewrite.
User/AGENTS authorization covers implementation and disposable trials after the
required Claude design approval; no additional permission request is needed.

## Existing boundary and deliverables
Both application and installer use Python. Current archives contain plain `tools/`
modules and matching native companions. Start with that archive, not a new wheel
or `uv tool install` package that the repository does not currently provide.
Production installer/verifier/launchers remain unchanged; scratch launchers use
a version-owned absolute interpreter, preserving `-I -B`, tools import path/argv.

Prove three native targets: Linux x86_64 and aarch64 (GNU Marmish companions),
and macOS aarch64. uv's Linux musl executable is only a bootstrap-tool choice;
it does not make Marmish's GNU native companions static or portable to musl.

## Candidate pins and trust anchor
Candidate uv: **0.11.16**, source commit
`135a3636789bb6886e0a7876f58cb4dbb270ba6d`.
Candidate ordinary GIL CPython: **3.13.13**, python-build-standalone **20260510**,
`install_only_stripped`, not free-threaded/debug/prerelease. These are candidates,
not claims of tested support. `stable_ast_dump` already handles the previous
harness issue. Run the full `tools` suite under 3.13.13 and record all failures;
failures block acceptance. Never silently change the runtime to pass.
The full-suite test environment additionally selects the existing Rust toolchain
needed by edition-2024 native test fixtures and includes system sbin tools.
This test-only compiler discovery does not relax the installed launcher or uv
environment allowlists; it does not install or update a toolchain.

| Target | uv release archive | SHA256 |
| --- | --- | --- |
| Linux x86_64 | `uv-x86_64-unknown-linux-musl.tar.gz` | `1bc4be1be0a000f893b0d1db97906cf392b63fa22fda9a0ecf33d0d4bbb4bc9a` |
| Linux aarch64 | `uv-aarch64-unknown-linux-musl.tar.gz` | `ac022d96411143b9a2dd75ea711fa8dd4cd14538bf248f2e5df3c10a80f7f6a4` |
| macOS aarch64 | `uv-aarch64-apple-darwin.tar.gz` | `2b25be1af546be330b340b0a76b99f989daa6d92678fdffb87438e661e9d88fb` |

| CPython metadata key | SHA256 |
| --- | --- |
| `cpython-3.13.13-linux-x86_64-gnu` | `bbe27549e475fe5f22d42a8e0d553dc79d80d8a00e05712599637857d287360e` |
| `cpython-3.13.13-linux-aarch64-gnu` | `67c837838c56a7d16187d1be9fad326a617e0b1ee2687e1a0dda0c85053dac33` |
| `cpython-3.13.13-darwin-aarch64-none` | `16d2332d950178968534e65fe09f01f876d13af1147176fd0c77a74c9e4d1a4b` |

uv hashes were read from the official [tagged release API](https://api.github.com/repos/astral-sh/uv/releases/tags/0.11.16);
CPython hashes/URLs from [pinned uv metadata](https://github.com/astral-sh/uv/blob/135a3636789bb6886e0a7876f58cb4dbb270ba6d/crates/uv-python/download-metadata.json).
Before any artifact execution, freeze those exact URLs, sizes, hashes, target
keys, source commit and the selected Marmish release/tag/hash in reviewed fixture
data; compare CPython hashes with that PBS release's checksum asset and uv hashes
with its checksum assets. Missing/disagreeing evidence blocks the trial.
Freeze the concrete Marmish archive identity before beginning approved trials.
The pinned upstream metadata source is 2,752,303 bytes. Its one-time read-only
freeze research has a separate 16MiB bound and records its exact size/hash;
the runtime consumes only the reviewed one-entry local metadata, capped at
2MiB. The harness never downloads the larger source metadata during trials.

The bootstrap trust root is the reviewed Marmish script/release-data revision
obtained through authenticated HTTPS from the documented GitHub or Iris source.
Embedded hashes bind subsequent artifacts to those reviewed bytes; checksums
obtained from the same publisher are not independent publisher authentication.
Do not claim signatures that do not exist. Mutable `head` bootstrap trusts the
source host/current project publication; a commit-pinned URL is reproducible.
The spike uses a fixed reviewed commit, never `latest` or a fetched executable
installer script. Increment 1 must preserve and document this trust boundary.

## Install contract to prove after approval
1. Realpath the platform temporary root first (`/tmp` is a symlink on macOS),
   then create an owned mode0700 ASCII scratch directory below it. Reject symlinks
   below that root, foreign ownership and existing interpreter directories. Use
   a fresh exclusively created final version slot; uv reuses existing installs
   without rehashing. No root install. Never rename or move a completed slot.
2. Use fixed system tool paths and an allowlisted subprocess environment: scratch
   HOME/XDG/TMPDIR, necessary locale; no ambient `UV_*`, `PIP_*`, `PYTHON*`, loader
   injection, project/index configuration or user cache. Explicitly leave
   `UV_PYTHON_CACHE_DIR` unset (its separate archive cache bypasses `--no-cache`).
   Set `--no-config` throughout. Snapshot real `~/.cache/uv`, `~/.local/share/uv`,
   `~/.config/uv`, `~/.local/bin` before/after and require no change.
3. Use curl `--proto =https`, no `-L`: manually follow at most five redirects,
   reading `%{redirect_url}` and checking each URL's scheme/host against the
   reviewed allowlist before fetching. Set connect/total timeouts. Cap uv at
   64MiB, CPython 128MiB, release archive 512MiB and metadata 2MiB: combine
   `--max-filesize`, a bounded file-size resource limit and post-download checks.
   Verify SHA256 with Linux `sha256sum` or macOS `shasum -a 256` before extracting
   or executing; reject wrong host, size, checksum or target.
4. Extract only the expected uv binary member after verification, into the owned
   directory; never unpack into arbitrary destinations. Generate owned local
   metadata with exactly one pinned upstream entry, changing only the archive
   URL to its prefetched local `file://` path. Use a fixed ASCII filename/path;
   the harness rejects missing/null/non-64-hex SHA256 and unexpected entries.
   uv itself permits absent hashes: this mandatory-hash gate belongs to us.
5. Make the archive read-only in the 0700 directory and rehash immediately before
   uv reads it. Invoke the supported interface (quote each path argument):

   ```text
   uv --no-config --offline --no-cache python install --managed-python --no-bin \
     --no-registry --install-dir ABS_VERSION/python \
     --python-downloads-json-url ABS_STAGE/downloads.json EXACT_METADATA_KEY
   ```

   [Pinned source](https://github.com/astral-sh/uv/blob/135a3636789bb6886e0a7876f58cb4dbb270ba6d/crates/uv-python/src/downloads.rs)
   supports local metadata and `file://` archives. Our pre-extraction hash is
   primary; uv's second check completes after temporary extraction and before
   installation. Keep `UV_PYTHON_CACHE_DIR` unset, cap expanded runtime at 1GiB
   using bounded scratch capacity/monitoring, and clean only owned scratch.
6. Prove each request selects the single metadata entry on its native target.
   Discover the resulting interpreter; never derive its directory from metadata
   keys (`darwin` requests become `macos` paths). Verify implementation, patch,
   architecture, executable and base prefix. Resolve all minor-version links
   and interpreter paths inside the version slot. Use the base interpreter
   directly: plain stdlib tools need no venv, pip, package index or source build.
7. Extract the existing installer's marked Python heredoc byte-exactly into an
   owned scratch file, asserting unique begin/end markers and no altered bytes;
   execute it with the managed interpreter's `-I -B`. Retain manifest/member/
   path/size/native-target checks. Place verified plain tools and companions in
   the version slot. Preserve native hashes, licenses and macOS signatures.
8. Generate the scratch launcher through `marmish_release.launcher()`; assert it
   differs only in its interpreter token, replaced by the quoted absolute managed
   executable. Preserve `-I -B`, imports and argv. No uv/download/PATH Python at
   startup. Each retained version owns its base interpreter and minor links;
   removing one never removes another's runtime. Never mutate signed helpers.

## Validation and decision gates
Reuse `.github/workflows/installer-validation.yml`, which already tests the
actual installer and fake/offline quickstart on native ubuntu24 x64/arm64 and
macos15 through push/PR/manual triggers. Preserve those gates; extend the matrix
or add a path-gated managed-packaging job for this increment's frozen assets.
Every native result is required. Increment 1 extends actual `scripts/install.sh`
CI to exercise managed bootstrap positive/negative installation, not just this
spike. Record hashes, versions, base paths, native dependencies and CI run links.
Use disposable directories; no user identities, Keychain changes or live relay
traffic. No claims about Marmot E2E or confinement follow from packaging smoke.

- Positive: full `tools` suite; installed help/doctor/native executable smoke and manifest provenance;
  tools imports, argv fidelity, spaced prefix, matching helper hashes; system
  Python absent from PATH and failing python3 sentinel unused. The
  [completion amendment](managed-runtime-spike-completion.md) corrects the macOS native-help gate.
- Isolation: hostile UV/PIP/PYTHON variables, uv config in cwd/parents/user home,
  unexpected current directory and preexisting installation cannot change choice.
- Integrity: wrong uv/CPython/Marmish digest, missing mandatory hash, truncated or
  oversize archive, wrong architecture, traversal/link member, unexpected uv
  binary member and interrupted extraction fail closed without launcher exposure.
- uv-specific: deliberately bad CPython metadata digest rejects a valid archive;
  substituted local archive fails; existing install is rejected by the harness;
  missing hash rejected by harness; absent local bytes cannot trigger fallback.
- Offline retention: remove uv/download/cache directories; launch two retained
  versions, remove one and relaunch the other. Linux independently denies network
  with a network namespace/bwrap. macOS records uv `--offline` only, without
  independent network enforcement; do not label it an enforced-denial pass.
  Capture failure/cleanup boundaries; no silent fallback or unchanged-home claim
  without the snapshots. Missing required native CI results block acceptance.

Exit: Claude reviews integrated harness/evidence; all packaging gates pass on
all three targets, or precise platform blockers are recorded and the increment
remains incomplete. A native confinement SIGABRT remains a separate blocker.
If this uv interface cannot enforce the contract, stop and review a verified
PBS interpreter bundled in the Marmish archive; never weaken digest checks.
Only then design increment 1's production bootstrap/setup integration.

Interface clarification from actual trials: the published `wnd` does not expose
`--version`; inspect its supported help interface and use native `--help` execution
plus verified manifest/source/hash provenance. The [completion amendment](managed-runtime-spike-completion.md)
replaces native help on macOS with its artifact-bound restricted guard refusal.
Do not claim a nonexistent version
probe or initialize daemon identities in this packaging-only increment.
Real-home snapshots compare lstat device/inode/mode/owner/size/mtime/ctime and
symlink targets, without reading preexisting user cache contents; concurrent
changes are inconclusive and cannot be attributed to the owned test process.
