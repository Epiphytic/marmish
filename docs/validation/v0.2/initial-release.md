# Native v0.2.0 initial release — 2026-11-03

**Published with full macOS testing still in progress.** The user authorized this
initial release with remaining fixes and validation assigned to v0.2.1. This is
not full all-platform command, provider or lifecycle qualification.

[Public v0.2.0 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.0)
contains Linux AMD64, Linux ARM64 and macOS ARM64 native Rust client, daemon,
worker, license and manifest assets: fifteen files total. Linux requires glibc
2.39+, macOS requires Apple Silicon and macOS 15+. macOS binaries are ad-hoc
signed, not notarized. Tor command qualification remains deferred to v0.3.

## Immutable source and publication

- Product source/tag target: `73ed2b41b64ecf614d245668e3ab3b2e5b753895`.
- Native runtime/build/dependency/product-script trees match baseline
  `347cf799ae5baa9e577ded1a00566c7a6b99b933`; only the reviewed release workflow
  and five amendment/review documents differ. New binaries embed the new source
  commit and are not byte-identical to older builds.
- [Packaging run 41664164076, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41664164076)
  passed all three native builds, signing where applicable, dependency/ABI
  inspection, license collection, packaging and installer rendering.
- Bootstrap-only commit `7e3e74411bed8657e8b8f5b599b36c2b702fe5bd` is a direct
  child of the product source and changes only `scripts/install.sh`. The generated
  shell is exactly the reviewed native template plus version and three manifest
  digest substitutions.
- Bootstrap SHA256: `46b4f369792f4f4ea36726c24414c5cca2ef977fec61712d821a1b81f985367a`.
- Both source branches and the immutable annotated `v0.2.0` tag were pushed to
  GitHub and Radicle. The tag dereferences to the product source on both mirrors.
  Radicle synchronized with Iris; actual Iris and GitHub installer bytes matched
  the bootstrap digest. Binary release assets are hosted by GitHub Releases;
  Radicle mirrors source, tag and installer.

All fifteen local asset sizes and SHA256 values matched their manifests. All
fifteen GitHub-uploaded asset sizes and server SHA256 digests matched those exact
files before publication. See [asset provenance](initial-release/assets-verified.json)
and [uploaded asset verification](initial-release/published-asset-verification.json).

The baseline run's default product tests and strict Clippy passed on all three
platforms, and its macOS signing passed. The caveated producer deliberately
skipped rerunning default verification on equivalent runtime source, the
feature-enabled installed tooling, producer quickstart and provider jobs. It did
not turn failed tests green. See [baseline step evidence](initial-release/baseline-checks.json)
and [new packaging step evidence](initial-release/packaging-checks.json).

## Actual HTTPS installation: six cases passed

[HTTPS run 41667214179, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41667214179)
passed its resolver, all six cells and rollup. Each cell used real HTTPS scripts
and public release downloads,
verified provenance, exercised interactive setup with disposable file-backed
credentials, checked status and repeated setup, and proved owned process,
profile lease, runtime and private-state cleanup.

| Target | Iris script | GitHub script |
|---|---|---|
| Linux AMD64 | PASS | PASS |
| Linux ARM64 | PASS | PASS |
| macOS ARM64 | PASS | PASS |

The two independent script sources are:

- [Radicle/Iris](https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh)
- [GitHub backup](https://raw.githubusercontent.com/Epiphytic/marmish/master/scripts/install.sh)

Root independently validated all six exact report schemas, source/run/attempt,
bootstrap and manifest bindings, success and cleanup flags, then reproduced the
hosted rollup byte-for-byte. See [context](initial-release/https-context.json),
[rollup](initial-release/https-rollup.json), [cell reports](initial-release/https-cells/)
and [URL observations](initial-release/public-sources.json). These file-backed
setup checks do not qualify real default-auto Keychain/Secret Service providers.

A separate normal-user Linux installation used the exact rendered script and
candidate files, verified all installed product hashes/modes and compiled
version/source/formats, and then repeated installation using actual public
GitHub downloads without an asset-directory override. Both installations exited
0; the latter activated generation 2 of the same v0.2.0 build. See
[installed provenance](initial-release/installed-provenance.json) and
[public-download installation](initial-release/public-download-install.json).

## Installed Linux live Marmot command check: passed

The installed release executable ran `command-check` with fresh disposable
client/server identities and an explicitly selected direct relay,
`wss://relay.us.whitenoise.chat/`. This relay is a test choice, never a product
default. Exit status was 0 and the independently checked finite report had
`ok=true`, no failure details, and all four samples with exact byte counts,
expected hashes and exit statuses.

| Sample | Observation | Elapsed | Output goodput |
|---|---|---:|---:|
| Running recovery | 27 stdout bytes, 25 stderr bytes, exit 7, one execution | 6.295 s | 4 B/s |
| Literal argv | 53 exact bytes including quotes, metacharacters, newline and non-UTF8 | 1.590 s | 33 B/s |
| Binary stdin echo | 16,384 exact binary bytes, exit 0 | 2.778 s | 5,896 B/s |
| Stderr/exit | 12 stderr bytes, zero stdout, exit 7 | 1.466 s | N/A |

Total driver time was **19.444 seconds**, including setup and cleanup. These are
single observations, not medians or pure network bandwidth. Goodput includes
command execution and asynchronous encrypted protocol/polling overhead; tiny
payload and recovery rates are not bulk-transfer benchmarks. Server-owner
restart observations were stop 29 ms, open 135 ms and activation 573 ms.

The recovery sample proved local identical attachment, existing identical submit,
local and remote conflicting-submit refusal, preserved output and exactly one
execution while the owner stopped and reopened. This is an in-process owner
restart while the worker runs, not an OS-process restart. The driver also
validated its retained test-only native probe obligations; those are not ordinary
publication ACK evidence. The report retains `purpose_ownership_unknown=true`;
this does not prove automatic deletion of probe rooms. Engine quiescence, joined
engine and MDK stops,
runtime cleanup and private-state deletion all passed; no state was retained.
See the [complete sanitized report](initial-release/command-check.json).

This check exercises the installed production command owner, engine and worker.
It does not replace the full public-CLI cross-platform client/server matrix,
recovery/policy/limits cohorts or cold/warm benchmark suite.

## Remaining work and honest limits

The baseline macOS feature-enabled tooling suite passed 426 tests and failed two
local benchmark child-process fixtures. Their cause remains unproven; the separate
approved diagnostic increment is excluded from this release. Beads `marmish-y5r`
tracks follow-up. Full public-CLI matrix/cohort, lifecycle and real provider
qualification remain open for v0.2.1; six passing installer cases do not close them.

The retained automatic v0.1 installer workflow still assumes embedded Python at
`scripts/install.sh`; the native cutover intentionally removes that wrapper.
Its post-cutover run is separate from the passing native HTTPS workflow.
Beads `marmish-21i` tracks retargeting legacy installer fixtures without losing the
Python protocol reference. No legacy CI result is treated as native acceptance.

Interactive PTYs/tmux, takeover/readers, privileged Unix-user mapping,
multi-device conformance, forwarding and file transfer remain future work.
Native profiles are fresh v0.2 state; v0.1 disk migration is not supported.
