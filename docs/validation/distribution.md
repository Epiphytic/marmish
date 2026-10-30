# Experimental distribution validation

## Published release and exact-archive acceptance

[v0.1.0-experimental.1](https://github.com/Epiphytic/marmish/releases/tag/v0.1.0-experimental.1)
is published from source `2a7e723eb8f980cda8a10f6d1e541d077cc15698`. All binaries
come from [workflow 41003542761](https://github.com/Epiphytic/marmish/actions/runs/41003542761),
attempt 1. The native macOS ARM64 and Linux ARM64 build/installation jobs passed.
Linux AMD64 compilation, daemon bootstrap, staged smoke, installation, help and
reinstallation passed, followed by a hosted live startup failure
([AMD64 job and logs](https://github.com/Epiphytic/marmish/actions/runs/41003542761/job/123894019825)). The workflow
conclusion is failure and its CI publication job was skipped; it is not a
successful hosted runtime run.

The hosted diagnostic reported `namespace_start_failed` at `identity_setup`,
with complete cleanup. The independent fixed user/network namespace probe
returned exit 1 and a known permission-denial result. Its read-only settings
were `apparmor_restrict_unprivileged_userns=1`, `unprivileged_userns_clone=1` and
`max_user_namespaces=58121`. These observations establish denial of the namespace
operation the product requires, not the specific policy responsible. No host
namespace or security policy was changed. See [the hosted evidence](distribution-hosted-2a7e723.json).

The approved conditional procedure used the exact downloaded Linux AMD64
archive on a local Linux AMD64 host, with acceptance and the installer from a
clean checkout of the same frozen source. Static doctor checks passed; local
glibc was 2.39, matching the archive's minimum, and the required namespace probe
succeeded. Full installed acceptance passed all 15 Marmot assertions in 198.025
seconds, with complete private-state cleanup. It covered literal special and
non-UTF8 argv, separate binary streams, additive authorization restrictions,
cancellation, retry/conflict handling, negative frames/invites, disconnect
continuation, restart recovery, replay rejection, fresh MLS sessions and actual
installed CLI execution. Real installation, help, idempotent reinstallation and
marker-owned uninstall also passed. See [the original passed report](distribution-installed-2a7e723.json)
and [Claude's final artifact/evidence approval](../reviews/distribution-artifact-evidence.json).
The preserved hosted failure remains separate from this local pass. The public
release's `installed-e2e.json` asset adds only `operator_provenance` to the
original passed report. Its SHA256 is
`03c76ad21a65bc72ecbcab7762d326a8f09762fd2774811689e3e62d48cb5662`.
That attribution is also copied into [the publication record](distribution-publication.json);
it does not change an acceptance claim or constitute host attestation.

Exact original archive SHA256 values:

| Target | SHA256 |
| --- | --- |
| Linux AMD64 | `c8af156ec02cc9fd679cfeb2040bc0d0862093e5c3d925ec28e6208153bf783f` |
| Linux ARM64 | `0b5a2c415561503410b34b3770b776f03edf4b6d4f28fbb611884736915255fe` |
| macOS ARM64 | `2b1ea2c6455d31a948bb51f0a71e8d47b13cd43f0397323ffeb5d46456369715` |

The original strict publisher aggregated those three same-run/source archives,
verified their payloads and the local report's source/archive/daemon bindings,
created an experimental draft, uploaded its assets and verified every uploaded
byte by downloading and hashing it before visibility. The mandatory immediate
release-body update explicitly names local Linux acceptance and the hosted
namespace failure category. Both Linux archives require glibc 2.39; macOS ARM64
requires macOS 15+. Neither ARM64 native installation result establishes confined
command execution; the earlier self-hosted runtime failures remain historical.
Intel macOS and Cargo application installation are not included.

Independent unauthenticated downloads of all eight public assets matched the
uploaded bytes; all seven `SHA256SUMS` entries passed. The Iris `head` installer
matched Git exactly, selected this prerelease without a version override and
successfully installed into a path containing spaces, a semicolon and a literal
`$HOME`. Both installed help commands, static doctor, idempotent public reinstall
and owned uninstall retaining versions passed. This public installation smoke
does not repeat or replace the separate exact-archive live acceptance above.

GitHub and the Radicle delegate namespace both expose release tag
`v0.1.0-experimental.1` at the exact source commit. The public
[Iris remote refs API](https://iris.radicle.network/api/v1/repos/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/remotes)
independently confirmed the delegate's tag and source branch. Canonical project
refs omit tags; the signed delegate namespace exposes them. The commit-pinned
Iris installer also returned HTTP 200 with exact Git bytes, SHA256
`331e48e6f80ff7a5bbd79f62179bc0a0c4b7c9d36e8efa497508b768fb3ed35e`.
See [the publication evidence](distribution-publication.json) for asset hashes,
installer checks and source/tag verification.

The sections below preserve the chronological implementation and failed-run
record. Earlier pending statements describe their recorded point in time.

## Reviewed implementation

The distribution design and independent Claude reviews are recorded in
[the phase contract](../phases/distribution.md),
[design approval](../reviews/distribution-plan-final.json) and
[initial code review](../reviews/distribution-code-initial.json).
The initial code review blocked the unsupported workflow context; implementation
also incorporates its tag-provenance, strict aggregate parsing, library-name,
error-reporting, glibc-version and failed-run evidence improvements. The subsequent
[integrated review](../reviews/distribution-code-approved.json) approved the code;
its additional draft-collision, archive-evidence binding and sanitized failure
reporting suggestions were also incorporated.

Local checks after those fixes: the complete tools discovery suite passed
327 tests with 7 skips; installer shell syntax and git whitespace checks passed.
The separate key-storage integration suite passed all 21 tests. Actual release
validation is recorded separately below when available. Local fixture tests are not native binary builds
or proof of Marmot command execution.

Both production dependency graphs were collected and validated during development:
Marmish resolves 162 packages and MDK resolves 553 on this Linux platform.
The native jobs recollect target-specific graphs. Notices preserve original
texts, pinned source identities and checksums, including SQLCipher, OpenSSL and
secp256k1; missing or unreviewed licensing fails packaging.
The existing manifests specify MIT; the added license uses Marmish contributors.

A bounded scan of 28 historical commits and 467 unique blobs found one
secret-pattern match: the documented scalar-1 Nostr test vector in
`tests/test_marmish_keys.py`. This pattern scan is not a proof that every possible
secret form is absent. Runtime state and private files remain ignored and the
exact staged publication is reviewed.

Ripwire quality checks identified two duplicated production helpers, which were
replaced with the existing build-environment and file-digest implementations.
Other reported items include unittest callbacks labeled unused, name collisions
with historical JSON fields and the existing e2e harness size. Archive validation
is intentionally explicit and bounded; strict parsing is shared by installation
and publication rather than independently reimplemented.

## Initial publication status (historical)

No native binary release has been validated or published at the time of this
implementation record. Linux AMD64 installed command execution is a publication
gate. Linux ARM64 and macOS ARM64 packaging success will not resolve their known
confinement/runtime validation blockers. Intel macOS is not a target.

Iris independently returned repository metadata and both lowercase `head` and
commit-pinned README bytes for source commit
`536933c228ea8f955002c665ad84f443b8f7ea50`. Both raw responses were HTTP 200
and matched Git exactly (SHA256
`85ab313079298890e22f8740cca5ee25000243f157233193453fb36102136f52`).
A supported restart of the existing user's Radicle node resolved stalled
replication; no identity, storage or configuration was replaced. New source
and installer publication must be verified separately after committing them.

## Runner assignment fallback

The initial self-hosted native workflow
[40999071717](https://github.com/Epiphytic/marmish/actions/runs/40999071717)
remained queued on all three targets beyond five minutes. The user was notified.
The original distribution dispatch
[40999081519](https://github.com/Epiphytic/marmish/actions/runs/40999081519)
was cancelled and verified completed/cancelled with zero jobs. Do not rerun that
superseded workflow revision. Existing self-hosted validation remains intact.

Claude approved the [hosted fallback design](../reviews/distribution-hosted-design-final.json)
and [implementation](../reviews/distribution-hosted-code.json). Standard native
GitHub-hosted VMs use Ubuntu 24.04 x86_64/aarch64 and macOS 15 aarch64. There are
no namespace-policy edits or ambient Keychain tests. Hosted ARM64 installation
is not confined runtime validation. The full local tools suite passed 331 tests
with 7 skips; Claude's focused distribution suite passed 69 tests.

A local Linux AMD64 build from reviewed source `4a78fb3` passed compilation,
all three pinned daemon bootstrap tests, staged provenance verification,
launcher smoke and static prerequisite diagnosis. Archive SHA256 was
`be1ec5c1c09e26b9e5aca6a89eca17f4d8a5281200caeae7227c14b2f541bf38`.
This development archive is not a published release; installed live acceptance
was still running when this fallback increment was committed.

## macOS canonical temporary paths

Hosted dispatch [41000261841](https://github.com/Epiphytic/marmish/actions/runs/41000261841)
assigned all three runners promptly. Its macOS offline tests exposed four
failures from `/var` versus `/private/var` temporary-directory aliases. Test
fixtures now resolve their roots; the license helper also canonicalizes its
root before containment checks. Installer symlink rejection remains unchanged.
An alias regression reproduced the failure before the fix and now passes,
including external dependency and escaping license-symlink rejection. Claude
approved the [design](../reviews/distribution-macos-path-design.json) and
[code](../reviews/distribution-macos-path-code.json): 71 focused tests passed.
The complete local tools suite passed 333 tests with 7 skips. Native macOS
validation requires the next dispatch; this evidence does not claim it passed.

## Installed live validation and safe failure diagnosis

The local installed development archive from source `4a78fb3` passed a direct
rerun of all 15 installed E2E assertions in 212.392 seconds, including actual
client/server command execution, literal/non-UTF8 argv, binary streams,
authorization denials, cancellation, disconnect continuation, restart recovery
and replay rejection. Cleanup completed. The harness was source `2b34b00`;
[the sanitized report](distribution-local-installed.json) binds both revisions
and the archive SHA256. This is not evidence for a different published archive.
The first local attempt reached its failure deadline; its generic report did
not preserve enough information for retrospective diagnosis.

Hosted dispatch `41000261841` completed: Linux ARM64 native build and installer
acceptance passed; Linux AMD64 compilation and the three daemon bootstrap tests
passed, but installed live acceptance failed immediately. macOS failed the
previously documented temporary-path fixtures. Publication was skipped.
Corrected dispatch `41000985026` was cancelled before job assignment so the next
run can include useful failure diagnostics. No release was published by either.

Claude approved the diagnostic design and implementation in the linked
`distribution-diagnostics-*` reviews. Failed acceptance now preserves only
finite stage/error/assertion/cleanup fields, with acceptance-owned source and
archive bindings. Tests cover malicious values and forged success; real
publication validation rejects the failure evidence. All 75 focused tests and
the complete 337-test tools suite passed locally (7 skips). The AST test's
initial complexity/nesting finding was fixed with focused collectors. Remaining
Ripwire findings are aggregate unittest class length, the required static error
vocabulary, and unittest/visitor callbacks it cannot see dynamically; these are
accepted with the focused review, rather than splitting classes to hide size.
Hosted startup failure remains unclassified until the next diagnostic run;
namespace policy is a hypothesis, not a validated conclusion.

## Cross-version test fingerprints and namespace observation

Dispatch `41002528506` fixed the earlier macOS temporary-path fixtures but
exposed a Python 3.14 difference in the new AST drift test: `ast.dump` omits
empty fields by default from Python 3.13. The exact failure was reproduced
locally on 3.14. A test-only helper enables `show_empty=True` on 3.13+ and
preserves every reviewed fingerprint and count. All 76 focused distribution
tests passed independently on Python 3.11.15, 3.12.3 and 3.14.7. The full tools
suite passed 338 tests with 7 skips. Claude approved the design and code in
[the AST/fallback review](../reviews/distribution-ast-fallback-design.json) and
[the compatibility/probe review](../reviews/distribution-ast-probe-design-code.json).

Claude also approved [the implemented observational workflow step](../reviews/distribution-probe-code.json).
It logs bounded numeric namespace settings and a fixed ten-second `unshare`
probe with finite permission-denial output. It changes no host setting, is
non-gating and does not replace the real live diagnostic. At the time of this
increment, the next dispatch had not exercised it and no
hosted namespace-policy cause was established. The conditional exact-archive
local-validation procedure was documented and reviewed but remained inactive
until its native-build/install and denial conditions were proven for one frozen
source and run. No release existed at that point. See the published-release
section above for subsequent evidence.
