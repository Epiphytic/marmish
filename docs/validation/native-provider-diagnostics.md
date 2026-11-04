# Native provider diagnostic qualification

Product source: `61f47606c5e7ba9048f196165a8025e7c2662839`, candidate
`41898839617`, attempt1. All package and file-backed quickstart jobs and both
Linux providers passed. The macOS outer qualifier failed after748ms with only
operation_failed; this does not identify a Keychain runtime failure.

The diagnostic-only ancestor retains version0.2.2 and exactly the twelve approved
paths. It reuses the old macOS product/bootstrap artifacts without rebuilding
or treating the failed whole candidate as production-qualified. Its output
separates product and diagnostic source bindings and cannot qualify publication.

Offline supervisor discovery and direct execution:30/30 PASS. Workflow static
contracts:3/3 PASS. Node tooling:58/58 PASS. Actionlint1.7.7 and diff whitespace
checks PASS. Claude code round1 found failure-category preservation B1; fixed
and round2 APPROVED, including sentinel redaction and real stop-error coverage.
The review's unrelated broad legacy tooling suite failures have not been
attributed to this increment and do not replace the scoped results.

Quality-delta reports finite supervisor stage complexity/length increases and
existing-function churn; these serve bounded diagnostic attribution and were
reviewed. Dynamic unittest entry points are not dead code. No runtime identities,
account details, raw subprocess output or private provider material are committed.

Hosted diagnostic [41915532600/1](https://github.com/Epiphytic/marmish/actions/runs/41915532600)
started at 21:22:49 UTC on 2026-11-04, six seconds after creation. Supervisor
`5ccc646c3b5a85df73aef79dd14286c6b6d78f0c` validated the exact old product and
artifact bindings, passed offline contracts, then failed at `account_home_chown`
with `operation_failed`, exit code 1, elapsed 530 ms. The report records
`runner_keychain_unchanged=true` and `owned_account_removed=false`; product
execution was not reached. The provenance envelope records `guard=assets_pass`.

The negative group operand `uid:-2` is incompatible with Apple's unsigned
numeric [chown parser](https://github.com/apple-oss-distributions/file_cmds/blob/main/chown/chown.c).
An independently approved harness-only correction changes chown to the allocated
UID alone. New offline coverage asserts complete argv, private mode-0700 home
creation before ownership transfer, and unchanged account primary group.
Supervisor discovery and direct execution each pass 31/31; Node tooling passes
58/58; static workflow contracts pass 3/3. Actionlint 1.7.7 and diff whitespace
checks pass. No live accounts, Cargo builds, product changes, or private provider
material were used in this correction. Actual code review and a fresh hosted
provider run remain necessary; no macOS provider pass is claimed.


Hosted diagnostic [41916591226/1](https://github.com/Epiphytic/marmish/actions/runs/41916591226)
reached the child after the owner-only correction. It failed at outer
`qualification_validate` with `owned_account_qualification_failed`, elapsed
4123 ms; `owned_children_reaped=true`, `owned_state_removed=false`, and
`runner_keychain_unchanged=true`. This exposes no specific child cause and is
not a provider pass.

The independently approved finite-child-diagnostics increment adds failure-only
validated category/stage/status tuples, preserving the outer failure category
and successful schema. Offline supervisor discovery and direct execution each
pass 40/40, Node tooling 58/58, static workflow contracts 3/3, actionlint 1.7.7
and diff whitespace checks pass. No live accounts or product builds were run.
Actual code review and another exact-source hosted diagnostic remain pending.


Hosted diagnostic [41918539030/1](https://github.com/Epiphytic/marmish/actions/runs/41918539030)
identified `mac_default_verify`, `operation_failed`, child exit 1, elapsed
2718 ms. Preceding security setup operations succeeded; the child provider was
reaped and the runner Keychain stayed unchanged. Product execution had not
started. Apple preference persistence source supports the missing-parent
hypothesis described in the phase plan, without proving the runner's prior
filesystem state.

The independently approved, owned-account-only preference preparation remedy
passes supervisor discovery/direct 44/44, static workflow 3/3, Node tooling
58/58, actionlint 1.7.7 and whitespace checks. Directory fixtures are temporary;
no local account, runner HOME, Keychain or product build was used. A reviewed
hosted rerun remains required to establish successful default lookup and full
qualification. Successful directory preparation alone is not a provider pass.

Actual independent Claude code review APPROVED the owned Preferences prerequisite; see the existing [review record](../reviews/native-provider-diagnostics-code-r2.md). Reviewer independently passed 44 supervisor tests in discovery and direct modes and whitespace validation. A deterministic fixture chmod incorporates its useful nonblocking finding. Hosted qualification remains pending.

Hosted diagnostic [41920220640/1](https://github.com/Epiphytic/marmish/actions/runs/41920220640)
was created at 21:53:12 UTC and started at 21:53:17 UTC. The unchanged real default
Keychain query now succeeds; later `install` fails with `operation_failed`, child
exit 1, elapsed 1558 ms. Runner Keychain unchanged and owned children reaped are
true; owned state/account removed are false. This is progress, not complete
qualification or proof of the original missing-directory hypothesis. The source
utility-discovery mismatch is documented in the phase amendment; hosted remedy
validation remains required.

The discovery-only remedy passed supervisor discovery/direct 46/46, static
workflow 3/3, Node tooling 58/58, actionlint 1.7.7 and whitespace checks.
Actual independent Claude design and final code/integration reviews APPROVED;
see the existing review records. Runtime PATH remains the private exact utility
allowlist, excluding Python. Hosted installer/provider success remains pending.

Hosted [41921288533/1](https://github.com/Epiphytic/marmish/actions/runs/41921288533)
at supervisor 698571a passes install and initial provider probe, then fails at
quickstart: `provider_qualification_failed`, no checked child status, elapsed
6760 ms. Runner Keychain unchanged and owned children reaped remain true;
owned state/account removed remain false. The PTY driver already invokes the
0644 bootstrap via `/bin/sh`; executable-bit failure is not supported by source.
Complete finite failure diagnostics are being added before another hosted run;
no product fix or qualification pass is inferred.

Complete bounded PTY diagnostics passed supervisor discovery/direct 53/53 each,
static workflow 3/3, Node 58/58, actionlint 1.7.7 and whitespace. Actual Claude
design and final code/integration reviews approved; evidence is in existing
review records. Tests include mode-0644 scripts, true exit/signal, timeout without
fabricated status, optional-off behavior, hostile transcript/nested report
sanitization, outer copying and cleanup snapshot preservation. No live product,
accounts or Keychains were used. Next hosted run is still required to identify
the actual quickstart failure and establish provider qualification.

Hosted [41923076525/1](https://github.com/Epiphytic/marmish/actions/runs/41923076525)
at c175e08 exposes terminal_failure, all five prompts, true exit1,
setup_choices/native_setup_failed and unknown native token, elapsed7939ms.
Runner Keychain unchanged and owned children reaped remain true; owned state
and account removal remain false. Complete Identity vocabulary was absent from
the diagnostic parser; no specific Identity error was observed. Independently,
the old owned root produces runtime95+10=105 against strict104 and is therefore
a source-proven budget violation. The reviewed .mp- fixture root gives81+10=91
for all100reservedUIDs. No product socket bound or custody gate is changed.

Central targeted Rust test
`supervisor_failure_labels_match_python_ast_and_retain_cleanup` FAILED on the
current diagnostic ancestry (0 passed, 1 failed): Python AST adds
owned_preferences_directory while Rust REPORT_FAILURES omits it. The old
owned_account_prerequisite_failed fallback remains reachable through a nested
conditional and must remain in the mapping; recursive AST extraction must cover
that path. Actual log:
`/tmp/marmish-scj-provider-final-rust-contract.log`. The current prefix constant
and Identity enum do not change those failure-label sets. Parent coordinates a
separately reviewed installed-check mapping reconciliation in the automation
worktree after integration; no Rust edit or diagnostic-guard expansion hides
this failure. Full integrated acceptance remains pending.

Identity vocabulary/socket-budget increment passed supervisor discovery/direct
59/59 each, static workflow3/3, Node58/58, actionlint1.7.7 and whitespace.
Actual Claude code/integration review approved this bounded increment with the
central Rust failure explicitly open. No live product/account/Keychain was run.
Full acceptance awaits the separately reviewed combined-tree Rust correction
and actual hosted qualification; no category or provider success is inferred.
