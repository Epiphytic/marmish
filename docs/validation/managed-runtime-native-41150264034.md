# Hosted packaging run 41150264034

[GitHub run](https://github.com/Epiphytic/marmish/actions/runs/41150264034), source
`e5a636d1026978b5c539c898795e554d32118eac` (master push).
The [sanitized aggregate](managed-runtime-native-41150264034.json) preserves all three
report objects unchanged, original artifact-file hashes and public job/step
metadata. All three installer archive/controlling-terminal jobs passed. All
three managed jobs passed their offline contract step. macOS then passed its
actual managed proof; both Linux proofs failed. The overall run concluded failure.
No raw logs were fetched, included or published.

| Hosted target | Actual managed result | Full tools suite | Cleanup / monitored home |
| --- | --- | --- | --- |
| Ubuntu 24.04 AMD64 | `full_tools_suite_failed` | 513 tests, 8 skips, exit 1; two required namespace failures | complete / unchanged |
| Ubuntu 24.04 ARM64 | `full_tools_suite_failed` | 513 tests, 8 skips, exit 1; two required namespace failures | complete / unchanged |
| macOS 15 ARM64 | `ok=true`, all ten assertions | 513 tests, 32 skips, exit 0 | complete / unchanged |

The managed jobs started at 00:27:33 UTC (Linux AMD64), 00:27:35 UTC (Linux ARM64)
and 00:27:34 UTC (macOS) on 2026-10-31. Their completion times were 00:28:41,
00:28:57 and 00:30:04 UTC respectively. None of these reports timed out.

Both Linux reports name the same mandatory failures:

- `test_marmish_bootstrap.LinuxHandoffTests.test_actual_unshare_bwrap_inner_chain_and_public_configuration`.
- `test_marmish_transport_sandbox.NamespaceTests.test_approved_bridge_decoy_denial_capabilities_and_cleanup`.

Both selected system Bubblewrap. Bare unshare, Bubblewrap and combined
preparation probes exited 1 with permission-denial hints. `ip` and `sysctl` were
present; AppArmor user-namespace restriction was 1. These read-only facts do not
establish an exclusive cause. No host policy was changed and no required Linux
namespace test was waived. Both Linux native help probes exited 0. Verified
downloads, interrupted extraction/fresh retry, archive-substitution rejection,
two-slot interpreter/verifier setup, runtime audit, help/doctor/literal argv and
hostile ambient/cwd isolation passed before the failed full-suite gate. Linux
offline-retention acceptance was not reached. Empty `unmet_acceptance` arrays do
not override `ok=false` or establish later gates.

macOS passed all ten assertions, including the full managed-interpreter suite
and offline retention after removing uv/download caches and one version. Its
`offline_network` is explicitly `uv_offline_only_not_independently_enforced`;
there is no independent Darwin network-denial proof. Both native startup probes
returned the exact expected 32-byte missing-config refusal marker, exit 1 and
no signal. This proves only that restricted-startup guard, not configured-daemon
or confinement acceptance. The earlier configured macOS sandbox SIGABRT remains
a separate unresolved gate. No failure identifiers occur in this macOS suite.

Sanitized progress was available: macOS recorded 513 matching prefixes, latest
`test_mdk_smoke.ValidationTests.test_send_publication_is_count_not_boolean`;
both Linux reports recorded 515 prefixes, latest naming the namespace failure.
`started_count` counts matching verbose prefixes rather than completed tests or
unique starts; failure/summary text may match. Latest hints do not establish
causality. Suite counts and exit outcomes are separate measured facts.

These experiments use existing experimental.2 archives, pinned uv 0.11.16 and
CPython 3.13.13; they do not release a managed installer. Linux retains its
system-Python dependency for doctor/confinement. Local Trial19 and this macOS
pass do not satisfy the required three-target native matrix or authenticated
self-hosted-helper proof. Queued self-hosted jobs supply no acceptance evidence.

Original `managed-runtime-report.json` SHA256 checksums (artifact names):

- `managed-runtime-ubuntu-24.04` (6986 bytes): `30626cad31ffa57e94176c68e7f9a4cbc82f13fbfe63559db9090555981fcfe7`.
- `managed-runtime-ubuntu-24.04-arm` (6968 bytes): `664eb114def4b89ca7cf221d0bccf6baa2d1b437d89211b82aacdf39a49b424b`.
- `managed-runtime-macos-15` (6305 bytes): `589f841206b7286efce46507fa24fc460c69c9c1956d5711d37846e3212d7899`.

Aggregate JSON SHA256: `8511f87e48b86abde137bc0c934ac582314134870cddd3327391223b49082b84`.

Independent Claude Code evidence review, actual `claude-opus-5-5` in auto mode,
APPROVED publication of the six final evidence/status documents. It verified
all three exact original report objects, sizes and checksums, aggregate hash,
selected job-metadata consistency, local Trial19 bytes/hash, source hashes,
counts, acceptance limits and privacy. It made no independent GitHub/Iris or
network check and ran no tests, native or administrative commands. The SDK
compile pass is an inference from the complete suite count, unchanged skip
count and source skip conditions; the report does not list individual skips.
No configured-daemon acceptance or implementation authority follows from this
publication review. Raw local review: /tmp/marmish-native-41150264034-evidence.raw.json.
