# Shared canonical credential helper: offline validation

Task `marmish-far`; isolated worktree based on `49d1314d8b1720d3554bf5945542bfc95f1e616b`. This is a corrective v0.2.1 increment. Its base still reports native package v0.2.0; the parent owns the independent version integration.

Actual Linux checks (2026-11-03), `CARGO_TARGET_DIR=/home/openclaw/repos/marmish-v02/target`:

- `cargo test --offline --locked -p marmish-native --features installed-checks --lib native_layout::tests -- --nocapture`: 4 passed, 23.55s compile and under 0.01s tests.
- Same command with filter `installed_frontends_retain_one_same_version_credential_helper`: 1 passed, 0.29s tests. The actual managed installer fixture resolves all three frontends and preserves the canonical version helper and worker after uninstall.
- `cargo clippy --offline --locked -p marmish-native --features installed-checks --all-targets -- -D warnings`: passed, 11.13s.
- `python3 -m unittest discover -s tools -p test_marmish_native_keychain_check.py`: 17 passed, 0.068s. The new offline regression pins actual marmishd init argv and rejects identity mismatch/provider refusal; it does not contact Keychain.
- Direct Rust formatting and `git diff --check`: passed.

The first independent code review found that executable validation canonicalizes ordinary helper symlinks. The implemented equality check rejects a helper resolving away from canonical parent/marmish. The existing negative test passes against the corrected implementation. Managed installation validation already rejects symlinked manifest files.

Ripwire's Rust quality delta has no gating regressions; five test functions are classified as newly dead by its static lens despite actual Cargo execution. Python quality delta flags class/test-class size and 99-token similarity between daemon_reload and lifecycle upgrade_reload. The separate methods preserve their different deadlines and finite error mappings and prevent modifying inherited lifecycle behavior; no metric suppression or generic abstraction was introduced.

No actual macOS provider, pre-fix control, full daemon startup or transport validation was performed here. The ordinary provider qualification now requires both named identities to load through actual marmishd init before success and after unlock. This proves frontend credential loading only when that real gate runs. A separate parent-owned diagnostic must exercise no-subcommand marmishd startup and compare the actual pre-fix control. Source inconsistency is established; the reported Keychain denial cause remains unconfirmed. No UI prompt, ACL migration, backend fallback or cross-version ad-hoc Keychain access is promised.

Independent [actual code review r2](../../reviews/v0.2-canonical-helper-code-r2.md) approved the offline increment and independently repeated layout4, managed1, Python17 and formatting/diff checks. It retained the explicit macOS limitations above.

## Integrated verification

Actual independent Claude auto review session `4db3527d-ddf4-4e49-b7a0-d97720091a77` approved integration atop389a2c8. Reviewer reproduced layout4/4, managed-helper1/1, Python provider19/19 and fmt/diff checks. The higher Python count includes the previously integrated patch-version lifecycle tests. macOS provider and full daemon startup remain pending.

Root Ripwire quality-delta at389a2c8+dirty reports four gating findings:99-token similarity between distinct daemon_reload/upgrade_reload operations; Qualification125→135lines; test class93→107lines; lifecycle recent churn. These are bounded reviewed test-tooling increments, not a clean quality gate. The distinct reload operations have different timeouts and finite failure classifications; a generic abstraction would couple separate acceptance contracts. Native Rust test functions are incorrectly classed as dead by the static graph despite their actual execution. Change-check reports broad name-based whole-file reach, not cfg-aware product impact. No suppressions or baselines were added.
