# Native 0.2.1 legacy launcher upgrade: local validation

This is local Linux evidence for the reviewed installer increment, based on
`7c8dcc6` in the isolated installer worktree. It is not a hosted macOS/ARM64
qualification or proof that the public 0.2.0 bootstrap contains the fix.

The native installer recognizes exact owned `0.1.0-experimental.N` markers,
selectors and regular shell wrappers without executing old programs. Tests use
an independently captured v0.1 wrapper template. Both default and custom roots
are covered, including a HOME path with spaces and state nested under the old
runtime root. Old runtime/private-state sentinels and retained wrapper backups
remain unchanged. Managed native reinstall, selection and uninstall still pass.

The final author command was:

```sh
CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target \
  cargo test --offline --locked -p marmish-native --lib native_install::tests
```

Result: **17 passed, 0 failed**, test duration **1.97 seconds**. The cases include
transaction boundaries before moves, after either wrapper move, asset-copy and
selector/receipt boundaries; recovery after removing old marker/lock; occupied
or altered backups; partial recovery; one missing wrapper; foreign regular,
symlink, dangling-symlink and hardlinked launchers; missing/held locks; malformed
metadata and overlap refusal. A separate-process stderr check verifies the exact
JSON-escaped conflicting path, including shell metacharacters and a newline,
and the finite `unrelated_launcher` refusal with `--bin-dir` guidance.

The serialized JSON-plus-LF bound is checked before creating a transaction temp
file; boundary/overflow tests exercise the generic atomic writer. They do not
claim to construct every maximal legacy transaction. Pending transaction recovery
retains evidence and refuses unexpected replacement bytes.

The native package and its matching lock entry are 0.2.1. The generated public
bootstrap, workflows, dependencies and identity/profile formats are unchanged.
The published 0.2.0 bootstrap remains pinned to its existing assets until root
publishes a separately qualified matching update.

Independent completed-code reviews and independently executed checks are recorded
in [review r1](../../reviews/v0.2.1-native-legacy-upgrade-code-r1.md) and [approved review r2](../../reviews/v0.2.1-native-legacy-upgrade-code-r2.md).
The r2 reviewer independently passed 18 installer-related tests in 1.77 seconds
(the broader filter includes the held-directory helper test), workspace formatting,
and strict native all-target Clippy. The earlier author installed-checks-feature
all-target strict Clippy run also passed; the final review reran the default feature
graph. No hosted migration test or additional live protocol test was
run for this installer-only increment.

Root integration copied the reviewed Rust sources byte-for-byte onto the current
release branch. Ripwire's quality delta against `39e4027` is **not clean**:
33 findings, 11 gating. The report includes same-name `recover` comparison
against unchanged engine test code, new functions/tests labeled dead code,
short-horizon churn in the transaction integration, a 21-token membership
predicate clone, and shared owned-file read structure in `checked_metadata` and
the legacy `wrapper` verifier. The latter needs device/inode metadata from the
same open file used to compare exact wrapper bytes; replacing it with a byte-only
metadata helper plus a second path lookup would weaken that identity check.
These are recorded for review rather than suppressing findings or introducing a
broad file-validator refactor solely to alter a score. The transaction/ownership
review and executed tests are the behavioral evidence; this report is not a
claim that the quality gate passed.

The root integrated default-library suite then passed: **345 passed, 0 failed**,
360.99 seconds, using `cargo test --offline --locked -p marmish-native --lib`
with the shared target directory. The installer Rust source hashes match the
approved table in review r2. Additional actual integrated Claude confirmation
`12fc0152-8b55-469e-9eb0-49770b4e0175` approved the trust/persistence design and
recorded quality findings. This does not replace hosted macOS/ARM64 validation.
