# Deterministic copy safety followup

Actual Claude Code Sonnet 5.5, auto mode, session
`75ea182a-0643-48b2-84b0-ee9f510c9832`: design **APPROVE WITH CHANGES**,
54.618 seconds, ten turns; revised implementation **APPROVE**, 32.983 seconds,
three turns. Review was read-only, without Cargo or live execution.

Five new test-only functions reuse existing filesystem fixtures. They check each
parent component as a resolving symlink plus a dangling variant; missing parent
and absent Resume; read-only admission and namespace creation; a same-content
destination appearing after prepare; and duplicate Create plus nine changed
resume identity/manifest fields. Refusals must preserve sentinel/staging bytes
and permit the original manifest to resume and commit.

Useful design corrections incorporated: relative operands and absolute owned
symlink targets, exact refusal categories, permission-enforcement probe, RAII
permission restoration, repeated no-clobber conflict, exact retained prefix and
fresh receipt recovery. Missing Resume may create only the shared admission
lock; it is not described as leaving an empty directory.

Author execution on Linux AMD64 as UID 1001: the full filesystem cohort
**33 passed, 0 failed**, 25.58 seconds, with no permission bypass skip. Strict
native all-target Clippy **passed**, 5.19 seconds. Hosted candidate tests also
set `MARMISH_REQUIRE_NONROOT_PERMISSION_TESTS=1`: actual permission bypass must
fail rather than count as platform coverage. Hosted execution remains pending.
There are no production changes and no claim of exhaustive race/fault coverage.

Quality delta against `52ac75a` was not globally clean: 39 rows and eight gates
include concurrently authored context tests outside this increment. The seven
`copy_files.rs` rows contain no gating regression: one new test complexity 17,
five test-entry dead-code reports and ambient test-module churn. These static
results do not replace the behavioral tests or erase earlier production debt.
