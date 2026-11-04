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
