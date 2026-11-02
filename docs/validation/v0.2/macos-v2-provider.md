# macOS v2 selected-keychain qualification

The scoped amendment adds an explicit namespaced selected-keychain constructor
and extends the existing owned disposable-keychain gate. The production helper,
default-keychain selection and native daemon are unchanged. The
[design](../../reviews/v0.2-native-macos-provider-design.md) and
[completed code](../../reviews/v0.2-native-macos-provider-code-r1.md) have actual
independent Claude Code Opus 5.5 approval in auto permission mode.

**Hosted provider behavior is pending, not passed.** The manual
`v02-provider.yml` workflow must run at the published amendment commit. Its report
proves only behavior on an explicitly selected, owned, disposable keychain:
four v2 identity/storage references, legacy isolation, exact secrets and labels,
duplicate/missing handling, lock denial, UI restoration and owned-state cleanup.
Restart and copied-executable replacement each report `ok` only when both the
identity and storage secrets load, or `locked_or_denied` only when both are
denied. Denial is not cross-process access proof. Default-keychain selection,
the production helper's ACL identity and interactive prompts remain unverified.
The storage-key checks do not by themselves prove SQLCipher database integration.

Root's Linux release checks passed: **8 vault unit tests + 3 helper tests**,
all-target vault/native Clippy with `-D warnings`, formatting, workflow actionlint
and diff checks. Claude independently type-checked the macOS vault and integration
test in a scratch target with Clippy; a preexisting Linux-only helper method
requires allowing dead code for that cross-check. This is neither macOS linking
nor runtime evidence. The dedicated provider gate uses normal release tests.

The static quality delta returned exit 2, with four gating findings: two clone
signals for the intentionally separate small provider constructors, and increased
length of the disposable-keychain assertions and supervising test. The selected
provider factory differs from the default provider factory; introducing a generic
factory to remove this tiny constructor similarity would obscure the boundary.
The test growth adds the specified v2 checks and cleanup-before-report sequence.
No baseline, suppression or production refactor was added; the static check is
not reported as passing.

## Source binding

The transport matrix run `41519670193` tests
`112bb3a330f9ef909599fbe1abbb435ed5cc06aa`. The provider workflow tests its own
subsequent commit; transport is not implicitly retested there. The approved scope
permits this separation only while the production files below and the entire
native crate are byte-identical. Against `112bb3a`, the only source changes are
the vault constructor pair and macOS integration test, plus the new manual
workflow and documentation.

| Protected source | SHA256, or Git tree ID for the entire native crate |
| --- | --- |
| `crates/marmish-native` | `dcdbeb145da8e07cadea87e73222b4255bf44ad1` (Git tree) |
| `crates/marmish-vault/src/macos.rs` | `451ef691e7085dc575bda91158c96b2334438ee94083deca6bb026467863d0ea` |
| `crates/marmish-vault/src/linux.rs` | `aa0ca941d9fff9c265e1f9c0967940befcf8f6c79a35ecf2e2eacfa14f8c2dac` |
| `crates/marmish-vault/src/helper.rs` | `0c5df15f12206a3f3c960a3b9df11e4af0bb73c619d85ee090d985e0e4feccb4` |
| `crates/marmish-vault/src/bin/marmish-keychain.rs` | `2239012bf6adbe98f9c8adb77fde61343aaf434c4ccb31fe42a360a0676b00a2` |

Reviewed amendment hashes:

| Source | SHA256 |
| --- | --- |
| `crates/marmish-vault/src/lib.rs` | `36a3599777910e3692f4417551f4c4e71cadf4bf9ba481cf2bf8d2724c8c7ad4` |
| `crates/marmish-vault/tests/support/native_macos.rs` | `ba175e80f7de3039ecd0a6e747ef906acc82baa43fa448fdc76d964dee296047` |
| `.github/workflows/v02-provider.yml` | `739bc9d923e0deed343d95dc5ad3fbe1b26315b81a6db9e56b44197f7d987943` |

No installer, remote command, default-keychain/helper integration, nine-pair
command matrix or v0.2 release acceptance is claimed by this amendment.
