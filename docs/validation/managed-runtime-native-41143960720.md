# Hosted packaging run 41143960720

[GitHub run](https://github.com/Epiphytic/marmish/actions/runs/41143960720),
source `983a6784206a65b9548d6c6af0b9760503905eb1` (master push).
The [sanitized aggregate](managed-runtime-native-41143960720.json) preserves all
three report objects unchanged, original artifact-file hashes, and selected public
job/step metadata. All three installer archive/controlling-terminal jobs passed.
All three managed jobs passed their offline contract step, then failed actual
proof. All six jobs started between 23:21:19 and 23:21:26 UTC on 2026-10-30;
the final macOS job completed with failure at 23:37:08 UTC. No acceptance gate is
waived by these results.

| Hosted target | Actual managed result | Full-suite report facts | Cleanup / monitored home |
| --- | --- | --- | --- |
| Ubuntu 24.04 AMD64 | `full_tools_suite_failed` | count 495, skips 7, exit 1; same two required namespace failures | complete / unchanged |
| Ubuntu 24.04 ARM64 | `full_tools_suite_failed` | count 495, skips 7, exit 1; same two required namespace failures | complete / unchanged |
| macOS 15 ARM64 | `child_timeout`, stage `full_tools` | count field 494, skips field 30, exit null; no successful suite completion | complete / unchanged |

On both Linux targets the mandatory actual unshare/Bubblewrap handoff and
approved bridge/decoy/capability/cleanup tests failed. Bare unshare, Bubblewrap,
and combined preparation probes all exited 1 with permission-denial hints.
`ip`/`sysctl` were present; observed AppArmor user-namespace restriction was 1.
Those read-only facts do not establish exclusive causality. System Bubblewrap
was selected, not the authenticated private helper. Both native-help probes on
each target exited 0.

The new sanitized progress capture was available on all three targets. On macOS
its `last_started_test` was:

```
test_mdk_smoke.EntrypointTests.test_main_success_finally_restores_handlers_umask_and_state
```

The method field was `test_main_success_finally_restores_handlers_umask_and_state`.
This is a source-defined prefix hint from bounded captured output, not proof
that this test caused the timeout or was the last unique test begun. The macOS
report has `started_count=508`; both Linux reports have `started_count=497` and
a last hint naming the namespace failure. These counters are matched verbose
prefixes, not completed tests or unique starts: matching summary/failure text
can add hints. They must not override the separately reported suite outcome.
Likewise macOS's count/skips fields do not establish completion because its
child timed out and its exit field is null. The approved 900-second full-suite
child deadline remains a failing gate, not a successful partial suite.

Both macOS native startup probes returned exit 1 with the exact expected
32-byte restricted-startup guard marker and no signal. This is a successful
missing-config refusal check, not configured-daemon or confinement acceptance.
The earlier confined macOS SIGABRT remains independent; this report provides no
crash attribution for the full-suite timeout. Only sanitized JSON artifacts and
public job metadata were fetched; no raw logs are included.

Before the failing full-suite gates, all three targets passed verified download
checks, interrupted extraction/fresh retry, post-rehash archive substitution
rejection, two-slot interpreter/verifier setup, runtime audit, installed
help/doctor/argv checks, and hostile ambient/cwd isolation/restoration. Later
enforced offline-retention acceptance was not reached. Empty `unmet_acceptance`
arrays cannot override `ok=false` or establish later gates. These are packaging
experiments using existing experimental.2 archives, pinned uv 0.11.16 and CPython
3.13.13, not a released managed installer. Linux still requires system Python
for doctor/confinement; Darwin makes no independent network-denial claim.
Queued or unassigned self-hosted jobs establish no acceptance, and a complete
successful native matrix is still required.

Original `managed-runtime-report.json` SHA256 checksums (artifact names):

- `managed-runtime-ubuntu-24.04` (6986 bytes): `acc288fba7d89443c59cdf31db5af1de1fa8a27e96e09784f87afcd4fda7391d`.
- `managed-runtime-ubuntu-24.04-arm` (6968 bytes): `3870645bd585620bf0218af8ffe12186984d1ad160ad8cf8e351e806dcc3ed3a`.
- `managed-runtime-macos-15` (6157 bytes): `b7d75aff53c9b79d4b1f8d3b013d415f77de36451b555742908df8c3b9eb3bb0`.

Aggregate JSON SHA256: `aa10b0b2c35aee01f098a0ca62df1e67703d05e78a0116778ed74b51e1277619`.

Independent Claude Code review in auto permission mode, actual model
`claude-opus-5-5`, verified the exact three report objects, original sizes and
hashes, aggregate hash, counts, selected metadata consistency and privacy.
An initial publication objection concerned wording in the separately reviewed
crash-design record; it was corrected and the follow-up explicitly APPROVED
publication of all four design/evidence documents. The reviewer used local
originals and did not independently fetch GitHub metadata, run tests, native
commands or network checks. No native success or production installer approval
follows from this evidence review.
