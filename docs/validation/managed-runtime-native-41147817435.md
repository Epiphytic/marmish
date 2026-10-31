# Hosted packaging run 41147817435

[GitHub run](https://github.com/Epiphytic/marmish/actions/runs/41147817435),
source `b1454afbdd5bd02b1049cfc3e3afb0a09bf4af52` (master push).
The [sanitized aggregate](managed-runtime-native-41147817435.json) preserves all
three report objects unchanged, original artifact-file hashes, and public
job/step metadata. All three installer archive/controlling-terminal jobs passed.
All three managed jobs passed their offline contract step, then failed the
actual full-tools gate. All six jobs started between 00:01:17 and 00:01:19 UTC
on 2026-10-31; macOS completed with failure at 00:03:24 UTC. No timeout occurred
in these three managed reports. This remains failed matrix acceptance.

| Hosted target | Actual managed result | Full tools suite | Cleanup / monitored home |
| --- | --- | --- | --- |
| Ubuntu 24.04 AMD64 | `full_tools_suite_failed` | 505 tests, 8 skips, exit 1; two required namespace failures | complete / unchanged |
| Ubuntu 24.04 ARM64 | `full_tools_suite_failed` | 505 tests, 8 skips, exit 1; two required namespace failures | complete / unchanged |
| macOS 15 ARM64 | `full_tools_suite_failed` | 504 tests, 32 skips, exit 1; eight failure entries including class setup | complete / unchanged |

Both Linux targets failed the same mandatory actual unshare/Bubblewrap handoff
and approved bridge/decoy/capability/cleanup tests. Bare unshare, Bubblewrap,
and combined preparation probes all exited 1 with permission-denial hints.
`ip`/`sysctl` were present; observed AppArmor user-namespace restriction was 1.
These read-only observations do not establish an exclusive cause. Both reports
selected system Bubblewrap, not the authenticated private helper. Both native
help probes on each Linux target exited 0.

Unlike the earlier hosted runs, macOS returned a nonzero full-suite result
before the child deadline. Its report lists these source-defined failure entries:

- `test_marmish_lifetime.ZombieGroupProofTests`: `setUpClass`.
- `test_marmish_native_validation.FinalReportTests.test_report_emitted_only_after_successful_teardown`.
- `test_marmish_native_validation.FinalReportTests.test_report_secret_is_discarded`.
- `test_marmish_native_validation.FinalReportTests.test_setup_signal_restores_handlers_and_emits_one_failure`.
- `test_marmish_native_validation.FinalReportTests.test_teardown_failure_preserves_assertions_and_fails`.
- `test_marmish_transport_sandbox.BwrapSelectionTests.test_file_modes_symlinks_and_untrusted_ancestors_fail_closed`.
- `test_marmish_transport_sandbox.ValidationTests.test_hash_and_home_permissions_fail_closed`.
- `test_marmish_transport_sandbox.ValidationTests.test_home_symlink_is_rejected`.

These identifiers localize failures but do not include exception details or prove
root causes. No raw logs are included or published. A separate bounded private
inspection of the failed-step log returned only 862 bytes in five lines and no
traceback or exception markers. The harness had captured full-suite output
privately; retained CI output supplies no additional exception details for these
eight entries. The report identifiers above remain the measured facts, not
inferred exception reasons. Sanitized progress was available:
macOS reported 510 prefix hints, latest
`test_marmish_transport_sandbox.ValidationTests.test_home_symlink_is_rejected`;
both Linux reports had 507 prefix hints, latest naming the namespace failure.
The `started_count` values count matching prefixes, not completed tests or unique
starts: failure/summary text can also match. The suite result/count and progress
hints remain distinct evidence.

Both macOS native startup probes returned exit 1 with the exact expected
32-byte restricted-startup refusal marker and no signal. This proves only the
expected missing-config guard, not configured-daemon or confinement acceptance.
The earlier confined macOS SIGABRT remains a separate gate.

Before the failed full-suite gate, all targets passed verified downloads,
interrupted extraction/fresh retry, post-rehash archive substitution rejection,
two-slot interpreter/verifier setup, runtime audit, installed help/doctor/argv,
and hostile ambient/cwd isolation/restoration. Later enforced offline-retention
acceptance was not reached. Empty `unmet_acceptance` arrays do not override
`ok=false` or establish later gates. The experiments use existing experimental.2
archives with pinned uv 0.11.16 and CPython 3.13.13; they do not release a managed
installer. Linux still records a system-Python dependency for doctor/confinement;
Darwin makes no independent network-denial claim. Queued self-hosted jobs provide
no acceptance evidence; a successful measured native matrix remains required.

Original `managed-runtime-report.json` SHA256 checksums (artifact names):

- `managed-runtime-ubuntu-24.04` (6986 bytes): `1d48d0badd2883a951c46e10547f1f93b77e7c796a8e95e4c50889d93f766e0e`.
- `managed-runtime-ubuntu-24.04-arm` (6968 bytes): `710afd163796a25d37c3044f1a1050df6db914c584188280d192fb93cbd2d0b1`.
- `managed-runtime-macos-15` (7652 bytes): `17dddb264b03922cbfc939ceed3b8636d51fceb090aceca7faf18e22e4a1043f`.

Aggregate JSON SHA256: `07c089358415b100812d229c9b61f07408b631a7611771841377d9cc3b48124b`.

Independent Claude Code evidence review in auto permission mode, actual model
`claude-opus-5-5`, initially withheld approval for one incorrect completion
timestamp. The companion now uses the aggregate's macOS job completion time,
00:03:24 UTC, rather than the later root observation time. The reviewer verified
exact report objects, original sizes/hashes, aggregate hash, selected metadata
consistency, failure counts, cleanup, home metadata and privacy against local
originals. It made no network, native, administrative or test calls and did not
independently verify GitHub metadata or the private-log byte count. Follow-up
review by the same actual model APPROVED publication of these three paths,
confirming the correction and unchanged aggregate. No native success follows
from document review.
