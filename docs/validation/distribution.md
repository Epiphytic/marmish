# Experimental distribution validation

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

## Publication status

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
