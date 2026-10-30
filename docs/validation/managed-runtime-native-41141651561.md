# Hosted packaging run 41141651561

[GitHub run](https://github.com/Epiphytic/marmish/actions/runs/41141651561),
source `3b3ef8c35a9a65f833d0b465c9c71f778a57b4ab` (master push).
The [sanitized aggregate](managed-runtime-native-41141651561.json) preserves all
three report objects without changing their fields, alongside job metadata and
original artifact-file checksums. All six jobs started between 22:59:30 and
22:59:33 UTC on 2026-10-30. All three installer archive/controlling-terminal jobs
passed. All three managed-packaging jobs passed offline contract tests but failed
the actual proof. This is failed acceptance evidence, not a successful matrix.

| Hosted target | Actual managed result | Full tools suite | Cleanup / monitored home |
| --- | --- | --- | --- |
| Ubuntu 24.04 AMD64 | `full_tools_suite_failed` | 478 tests, 7 skips, two required namespace failures | complete / unchanged |
| Ubuntu 24.04 ARM64 | `full_tools_suite_failed` | 478 tests, 7 skips, two required namespace failures | complete / unchanged |
| macOS 15 ARM64 (15.8.2) | `child_timeout`, stage `full_tools` | 900-second child deadline; no completed suite count | complete / unchanged |

Both Linux suites failed the same mandatory tests:
`LinuxHandoffTests.test_actual_unshare_bwrap_inner_chain_and_public_configuration`
and `NamespaceTests.test_approved_bridge_decoy_denial_capabilities_and_cleanup`.
Bare unshare and combined preparation returned operation-not-permitted hints;
direct Bubblewrap returned permission-denied hints. All exited 1 without a
signal, with `ip` and `sysctl` present. The observed AppArmor restriction was 1,
`unprivileged_userns_clone` was 1, and user namespace limits were positive; these
read-only facts do not identify an exclusive cause. This run selected system
Bubblewrap, not the authenticated private helper. Both native-help probes on
each Linux target exited 0.

On macOS both artifact-bound native startup probes returned exit 1, exactly
matched the expected 32-byte restricted-startup refusal marker, and had no
signal. This is the expected missing-config guard, not configured daemon or
confinement acceptance. The packaging proof reached its full-suite child deadline
and killed/reaped the owned child group; the job completed with failure at
23:15:02 UTC. The sanitized report identifies neither the slow test nor its
cause. No raw logs were collected. The previously observed confined macOS
SIGABRT remains a separate unresolved gate.

All targets passed verified download checks, interrupted extraction/fresh retry,
post-rehash archive substitution rejection, two-slot interpreter/verifier setup,
allowlisted environments, installed help/doctor and literal argv, runtime audit,
and hostile ambient/cwd isolation/restoration before the full-suite gate. The
failed full suites prevented later enforced offline-retention acceptance. Empty
`unmet_acceptance` arrays do not override `ok=false` or establish later gates.
These tests use the existing experimental.2 archives with pinned uv 0.11.16 and
CPython 3.13.13; they do not update the installer or publish a managed runtime.
Linux still records a dependency on `/usr/bin/python3` for doctor/confinement.
Darwin makes no independent network-denial claim.

The self-hosted workflow requires its own complete actual matrix and runner-group
policy evidence. Queued or unassigned jobs provide no acceptance evidence. Local
packaging passes do not replace these failed hosted outcomes.

Original `managed-runtime-report.json` SHA256 checksums (artifact names):

- `managed-runtime-ubuntu-24.04` (6636 bytes): `181ab0b8b0c26254eff782a5c386d73a366fbf55792205a67d8b2e664e4ed831`.
- `managed-runtime-ubuntu-24.04-arm` (6618 bytes): `94fab11da8f5043f758877c8ee74f9af3b3faf1eb3f73e642b86770ea4b54c92`.
- `managed-runtime-macos-15` (5621 bytes): `483c18fc3904c3351ee03704dcde83366dbb789650633722ab15bb48bf562b2b`.

Aggregate JSON SHA256: `a4c83ecff741a5ed590dabf3a3a7c35153b3ad87c3a2a0b8754d588f129f590d`.
