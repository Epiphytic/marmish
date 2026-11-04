# Native automatic release validation

Beads: marmish-scj. Hosted acceptance and v0.2.3 publication remain pending.
The installer still selects v0.2.2 until independently verified public assets and
the exact direct-child bootstrap exist.

Integrated offline checks after diagnostic ancestor d39a912 and round-1 fixes:
113 Node tests, Python HTTPS 24, automation workflow 4, provider supervisor 31
and diagnostic workflow 3 all PASS. Actionlint 1.7.7 passes candidate, publisher
and diagnostic workflows. Git whitespace checks pass. The actual old macOS
product API archive (13360975096) passes the new builtin ZIP digest/member
validation and exact signed manifest validation; this is parser compatibility
evidence, not a v0.2.3 candidate result.

Claude round 1 found two blockers (draft asset URLs and rerun artifact inventory),
both corrected with regression tests. Integrated round 2 is APPROVED by actual Claude Code; its report is retained
under docs/reviews/. The reviewer also reproduced thirteen broad legacy tooling
failures/errors on pristine HEAD: no new failures in the scoped increment.
These baseline failures remain separate from hosted release qualification.

Ripwire quality-delta exits 2 with eleven gates. Its same-name api and fixture
complexity/error-masking changes point to unchanged Python/Rust functions; the
new JavaScript names collide in this report. The bounded-file/environment
duplicates intentionally preserve builtin-only dependencies across the write
permission boundary. Small closed-schema and bounded-page checks have different
error contracts; moving them into existing npm-dependent modules would cross
that boundary. checkRun churn implements explicit event policy and preserves
old diagnostic restrictions. unittest entry points are dynamically discovered.
New publisher orchestration and ZIP-parser complexity remain explicit reviewed
risks, covered by finite state, digest, race, partial-retry and malformed ZIP
tests. No quality baseline was changed to suppress results.

Final integration on diagnostic ancestor 2d9ecd2 passes 113 Node, 44 supervisor, 24 HTTPS and four automatic-workflow Python tests plus actionlint for candidate/publisher/diagnostic workflows and git diff whitespace checks. Actual Claude independently approved the integration and also ran the three diagnostic static tests; see code review round2 appendix. Hosted diagnostic41920220640/1 passed default-keychain verification but failed later at install with exit1; it is not a qualified candidate.

Reviewed diagnostic ancestors 698571a and c175e08 are integrated without conflicts; only the approved checkRun event delta remains dirty in workflow.mjs and its tests. Actual independent Claude approved the shared-PTY diagnostic code and integration in session9614455e (see [diagnostic review](../reviews/native-provider-diagnostics-code-r2.md)), including dynamic helper/supervisor sourceDigest, exact13-path harness allowlist and unchanged success schema/gates. Final integrated checks pass 113 Node, 53 supervisor, 24 HTTPS and four automation-workflow tests. Pinned-product diagnostic41923076525/1 started in five seconds and failed after all five prompts with native_setup_failed; the original parser did not recognize the native category. The reviewed shorter owned-root prefix fixes a source-proven socket-path budget violation without claiming that category was observed.

Installed-provider final-SOURCE label reconciliation: the original targeted AST
parity failure was a test-extractor blind spot as well as a missing mapping.
Actual Claude rejected removal of the still-reachable prerequisite fallback;
corrected design approval retained it and strengthened conditional extraction.
The installed-checks adapter now additionally maps `owned_preferences_directory`
to `PrerequisiteFailed`. Exact label equality, mapping uniqueness, unknown-label
rejection and per-label independent cleanup checks remain intact.

On Linux in the automation worktree, coordinated shared-target commands pass:
`cargo test -p marmish-native --all-features --lib --locked
supervisor_failure_labels_match_python_ast_and_retain_cleanup` (1 passed), then
`cargo test -p marmish-native --all-features --lib --locked
lifecycle_provider::tests` (9 passed). These validate source parity and local
adapter/process-control fixtures, not live macOS account/keychain qualification.
The diagnostic-only ancestor worktree was not modified by this repair.

Actual Claude independently approved this narrow code change in session
`25d7702f-07c0-41fb-8407-bcd62ab638e6` and independently reran all nine
`lifecycle_provider::tests`: 9 passed, 0 failed. No live macOS qualification
is inferred; hosted release gates remain mandatory. The corrected design
approval session is `908a53a3-19d1-41b3-bc68-86c9342c2dbf`.

Final diagnostic ancestor699f53c is integrated. Hosted run [41925409283/1](https://github.com/Epiphytic/marmish/actions/runs/41925409283) started six seconds after creation and PASSED on macOS15 with pinned61f4760 product assets. Its finite report confirms automatic Keychain custody, two distinct named identities/four items, fresh and unlocked reload, locked refusal, no plaintext fallback, all owned children/state/account removed, and unchanged runner Keychain/interpreter. This tests provider custody and cleanup; network activation is false. It is not a v0.2.3 candidate or live remote-command result. Exact finite historical envelopes are retained in [diagnostic reports](native-provider-diagnostic-runs.json).

Combined-tree offline checks after699f53c pass113 Node,59 supervisor,24 HTTPS, four automatic-workflow tests, nine Rust installed-provider tests and actionlint1.7.7 for all three changed candidate/publisher/diagnostic workflows. The original central Rust parity failure is resolved by the separately reviewed mapping/extractor repair. Iris serves byte-identical699f53c supervisor source; both GitHub and Radicle source branches are current at that diagnostic ancestor.

The final all-feature native library suite completed successfully after combined
integration: `cargo test -p marmish-native --all-features --lib --locked`,
483 passed, 0 failed, 361.41 seconds. This discharges the explicit full-suite
condition in final Claude integration approval session
`8c573880-8c4e-42be-90c1-dc295e6bb58b`. The reviewer independently ran59
supervisor,24 HTTPS,4 workflow and113 Node tests and confirmed exact93-label
AST/table parity. No hosted v0.2.3 qualification is inferred from local success.
Prepublication checks found no v0.2.3 tag on either mirror and no matching GitHub
release, including visible drafts; master remains699f53c, unprotected with zero
rulesets. The35-file explicitly staged increment contains source, configuration,
tests and sanitized evidence; existing ignore rules exclude private runtime
state, keys, databases, logs, environment credentials and build outputs.
