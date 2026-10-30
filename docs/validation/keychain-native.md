# K3 native keychain acceptance

Production implementation: K2 `ed4271c`, metadata correction `9054f5a`;
validation harness based on `f1f3616` with its exact file digests in each report.
The [approved acceptance design](../phases/keychain-native-validation.md)
separates Linux runtime acceptance from the remaining native macOS gates.

The Linux fixture owns a private session bus without service activation,
GNOME Keyring process and credential directory. It never queries or changes the
ambient desktop service. Each scenario uses fresh disposable identities, one
catalog, distinct names and explicitly selected White Noise relay URLs. The
server SQLCipher entry is independent of all Nostr entries.

Offline harness checks passed 41 tests, including process identity races,
actual dummy executable stdin/environment checks, strict scans, CLI/import
context identity, locked frontend refusals, migration quiescence before
fingerprinting, and preserving credentials when cleanup cannot prove safety.
A preflight attempt failed before transport because the fixture PATH omitted
system administration tools. All owned state was cleaned; the fixture now uses
the same fixed system PATH as the frontends.

The runtime scenarios validate fresh auto-native profiles and explicit migration
from files separately. Reports distinguish completed assertions from failures;
no relay publication alone counts as command execution. Migration fingerprints
are taken only after an authenticated engine guard proves workers and I/O
quiescent, with both frontends stopped.

macOS provider-only evidence is recorded in the
[K1 report](keychain-vault.md). The dedicated default-keychain account/VM context,
injected daemon inherited pipe and Seatbelt runtime acceptance remain pending.
Neither Linux success nor a cross-check establishes those gates.

The fresh auto-native Linux scenario
[passed all 20 assertions](keychain-native-e2e-initial.json) in 235.967 seconds.
It exercised four distinct Nostr entries (three named profiles and one unnamed)
and one separate SQLCipher entry. Exact catalog/provider inventories matched.
Special argv bytes, binary streams, additive restrictions, cancellation,
retry/replay denial and CLI execution passed through actual Marmot rooms.
After keychain lock and both frontends exiting, the already-running command
finished; both locked reopens failed, and unlocking recovered the exact output
without a duplicate marker. Eight owned processes were inspected, including a
live daemon and worker: no key forms in argv/environment, bootstrap fd0 closed,
and no host keychain discovery environment in those two processes. At-rest scans
found no known raw/hex/base64/nsec secret forms outside provider storage.
Local cleanup completed; successful wrapper exit also proves provider process
identity checks, termination and owned directory deletion completed.

The initial migration scenario also passed 20 assertions in 226.521 seconds,
including active-worker refusal, exact reserved-reference retry, preserved
identities/rooms/ciphertext and recovered output. These initial reports predate
the K3 review additions: final single-record teardown reporting, both endpoint
checks after restart and native process auditing after migration. They remain
initial evidence, not a claim those added checks already ran.

Review dispositions: the active-worker guard legitimately returns
`recovery_uncertain` for any retained non-Exited record, before job-lock checks;
`busy` covers live identities/lock contention. Both are refusal, neither permits
migration or regeneration. The owned collection is restart-recovered with its
original password after lock; this is an isolated fixture mechanism, not an
ambient desktop unlock feature. The final snapshot is narrowed to the engine
database and retained job files; MDK WAL/SHM housekeeping is excluded.

The operator bounds the combined acceptance workload to 2400 seconds, including
stage cleanup; each scenario separately enforces its 900-second runtime alarm.
The code does not implement a shared cross-process budget.

The follow-up [Claude code review](../reviews/keychain-k3-code-followup.json)
approved the harness with no blockers after the four medium findings were
resolved. Its remaining limits are explicit: locked restart checks exercise both
profiles rather than a complete server engine start, and the combined budget is
operator controlled. The final wrapper also reports provider teardown status
explicitly; focused tests cover successful, failed and retained teardown.

The final quality-delta check exits 2 with ten major findings: growth and
complexity in the acceptance harness, shared test setup, and recent-edit churn.
New-symbol findings also include bounded cleanup exception handling and dynamic
unittest/context-manager reachability false positives.
The integrated review judged the sequential scenario orchestration appropriate;
it does not recommend splitting it merely to reduce metric counts. No metric
baseline or suppression was introduced. These are validation-only additions,
not an expansion of the production command protocol. A shared generic test
fixture for the small setup clone would obscure the separate entrypoint
contracts; bounded cleanup exceptions retain fixed failure outcomes rather than
turning incomplete teardown into success.

The post-review fresh-native run also
[passed 20 assertions](keychain-native-e2e-reviewed.json) in 226.58 seconds.
It checked both endpoint identities after restart, eight server-tree processes
(including one live worker, daemon and bridge), four client-tree processes
(including its daemon), and a separate live helper using private pipes.
The known secret scans of processes, private stderr and final report passed.
The wrapper exited zero after teardown. This report predates the additive
explicit `provider_cleanup` field and the corrected MDK metadata build. Its
implementation binding is `ed4271c`; field-bearing runs below bind `9054f5a`.
The later `f1f3616` increment changes macOS diagnostics, CI and docs, and does
not change the Linux command path.

Two concurrent attempts with the corrected daemon failed at `negative_frames`
with `message_not_published`, after eight fresh-native and seven file-mode
assertions. Both final reports confirm local and provider cleanup complete;
[the fresh failure](keychain-native-e2e-parallel-failed.json) and
[migration failure](keychain-migration-e2e-parallel-failed.json) are retained.
Their cause is unproven. Further attempts are sequential to remove concurrent
relay load as a variable; no acceptance assertions or publish checks were relaxed.

The final sequential migration run with the corrected pinned daemon
[passed 22 assertions](keychain-migration-e2e-reviewed-before-audit-fix.json) in 235.6 seconds.
This includes the post-migration gated native command and actual worker, both
daemon trees, bridge and helper audits added during review. Both endpoint
capability/identity checks passed after migration. Local and provider cleanup
are explicitly complete in the single final report. This successful sequential
run does not establish the cause of the retained parallel publication failures.

Before the final optional-child audit race correction, sequential fresh-native
acceptance [passed 20 assertions](keychain-native-e2e-final-before-audit-fix.json)
in 228.249 seconds. The completed-code review read both entire new test files
and ran the focused suites. It approved the harness and identified a useful
nonblocking race: an observed leak must not be discarded if the inspected child
exits immediately afterwards. The correction permits skipping only process
identity/absence failures; a new regression test checks three security findings
and both legitimate exited-child cases. These pre-correction reports remain
bound to their own source hashes, without attributing the later change to them.

The [complete code review](../reviews/keychain-k3-code-complete.json) inspected
the full harness and both entire test files; the
[audit-fix follow-up](../reviews/keychain-k3-audit-fix-approved.json) approved
the narrow race correction and ran 41 focused tests plus an independent
root/child, present/absent process probe. The full offline Python suite passes
247 tests with five Darwin-specific skips on Linux. Code reviews do not claim
independent verification of the separately reported live executions.

With the process-exit audit correction, the final migration scenario
[passed 22 assertions](keychain-migration-e2e-reviewed.json) in 221.908 seconds.
It binds the final harness digest and records complete local/provider cleanup.

The final fresh auto-native scenario
[passed 20 assertions](keychain-native-e2e-final.json) in 235.67 seconds.
Both final reports bind harness digest
`b5ed4f8c9db595ffc5a2e01766e6b3019bd12ecd9de0b18721753fe9e3ed6680` and the same
corrected daemon SHA-256
`577ae1543c6ff285379ce4c19893360ec896ef544f8f5b50296d2f980fdaec9f`.
They include the reviewed process-exit race fix, complete local/provider teardown,
and exact source-file and executable digests. No runtime source changed between
these two runs. The documentation-only `c38ab2b` commit does not affect their
`f1f3616` production binding. The summed duration of all nine recorded scenarios,
including failed attempts and local engine cleanup, is 1884.735 seconds;
owned-provider setup/teardown and the preflight refusal are not separately
measured in those reports. The operator-controlled 2400-second workload budget
is not an instrumented aggregate wall-time result. Review time is outside that
live workload.

Linux native storage and explicit migration are validated. Native macOS
default-provider/frontend and confined daemon acceptance remain pending in
Beads `marmish-d84.4`; the latest
[runner report](native-run-40978430684.json) separately records provider success
and the unresolved shim cleanup failure.

The [final evidence review](../reviews/keychain-k3-evidence-approved.json)
recomputed the four file hashes and composite digest against both final reports,
confirmed matching daemon provenance, historical/failed-report distinctions and
macOS pending gates, and approved their consistency. It did not independently
execute the live scenarios.
