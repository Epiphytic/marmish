# Store fixture frontend ownership review

Bead `marmish-d97.1`; base
`ae309a58bee3917bd2a90f643f1ec7da61d51f50`.
See [the phase](../phases/native-fixture-frontend.md).

Claude Opus 5.5, session `41bfc004-6675-4b7a-a602-80ed4d8564ac`,
reviewed design and completed code in auto mode. The first design review
required preserving the wrong-master test's early guard release. The revised
design resolved that finding, and the exact-base amendment was **APPROVE**
before implementation. Final design SHA-256:
`f20fe58c87a73535ba87928d6b336785615a1a3637278d18598bd24c40974747`;
approval response SHA-256:
`5ced048c1e6a0d1c8b05e918f30189a35e8d0c3fd4f42a79b90d1b1818570339`.
Completed-code verdict: **APPROVE**, no blockers; response SHA-256:
`d74c191a2994c0f9e319f53d7d009620fda296f04f0bdfe1b0165f3d748282ca`.

Reviewed source hashes:

| File | SHA-256 |
| --- | --- |
| `crates/marmish-engine/src/store/tests.rs` | `0839b588240d36b1ff78187ab6ec8bd44df55b9023f6fca0ab24d95151e15016` |
| `crates/marmish-engine/src/launcher/tests.rs` | `d93d74610400b0a8a906b44cd128c7be37afa6e45f2741eb6e3c3b6590a36ce2` |

The new ownership regression, selected alone against the unchanged fixture,
failed at its first ownership assertion (one failure, zero ignored), then
passed after the correction. Store tests passed 48/48 and launcher tests
17/17, with zero ignored in either selected inventory. These runs include
the original denial/token test and the existing isolated lock test with its
new descriptor-alias control. No original assertion or test budget was removed.

The reviewer independently ran `cargo +1.97.1 test --locked -p marmish-engine
--lib` under default test threads: **145 passed, zero failed, one ignored**,
in 20.55 seconds. The ignored test is the existing exec-only descriptor probe
helper; it is not counted as passed. Source hashes matched before and after.
Correct workspace-edition rustfmt and `git diff --check` passed. An initial
format check used the wrong edition, made no edits, and was corrected.
The quality delta exited zero; the reviewer accepted minor fixture churn,
six additional alias-control lines and the new test's unused-symbol signal
without suppression.

This increment changes two test files and no production locking, timeout,
retry or thread-count behavior. Descriptor duplication proves the mechanism
that can retain an OFD/flock lock, not the original holder. Existing explicit
engine-drop/reopen tests retain their semantics and possible inherited-alias
exposure. Neither one passing run nor this correction establishes a global
race-free claim. Broader native validation and fresh source-bound release
qualification remain separate.
