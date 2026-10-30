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
