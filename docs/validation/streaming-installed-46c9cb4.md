# Partial installed qualification: 46c9cb4

[Machine-readable pins and results](streaming-installed-46c9cb4.json).

Source `46c9cb46e382735a8678b17eca5e416f30b0c264`; candidate [42453390792/1](https://github.com/Epiphytic/marmish/actions/runs/42453390792) passed all three native platforms and final rendering. Candidate version 0.2.3 does not publish a new release or change the public installer.

The chain stopped at the third cohort, copy-matrix. Stream and rsync contain 27 passing hosted rollup observations; full qualification is false. The five later cohorts were not dispatched, and the final auditor was not run.

| Cohort | Run | Result | Passing rollup observations | CLI wall range (s) | Application goodput (B/s) |
| --- | --- | --- | ---: | --- | --- |
| stream-matrix | [42500831991](https://github.com/Epiphytic/marmish/actions/runs/42500831991) | success | 9 | 49.006619–58.060560 | 3527.351457–4179.027312 |
| rsync-matrix | [42501192017](https://github.com/Epiphytic/marmish/actions/runs/42501192017) | success | 18 | 37.540244–51.503708 | 636.226031–872.876588 |
| copy-matrix | [42501629829](https://github.com/Epiphytic/marmish/actions/runs/42501629829) | failure | 0 | not accepted/measured | not accepted/measured |

Stream uses 204800 validated application bytes per observation; rsync uses 32768. Wall time covers installed CLI lifetime including transport, command and output, excluding independent receipt and cleanup validation. Goodput is application bytes divided by that wall time. These are one sample per directed case, not RTT, wire throughput, compression ratios or percentile estimates. No copy performance result is accepted from the failed cohort.

Every cohort retained all 12 canonical identity/ready/client/server records; each is pinned by SHA256 and size. Source/candidate/context, identity/ready/result references, installation/bootstrap/manifest and product file metadata were independently cross-checked. All 45 installed-run artifact ZIPs (14 stream, 14 rsync, 17 copy) matched fresh GitHub API size and digest; each ZIP member matched retained extracted bytes. This is finite offline evidence inspection, not the full final auditor or a second physical-process/transport observation.

## Copy-matrix failure

Linux AMD64 emitted `invalid_or_failed` approximately 31.6 seconds after its supervised step began, followed by a generic supervisor failure. Its terminal fallback retains zero Pair records and six `cohort_incomplete`/`skipped` cells, with an empty server-ready pin map. The first failing phase/category is unknown; fallback absence does not prove commands never ran.

ARM64 and macOS each retain four complete exit0 Pair records targeting ARM64/macOS, plus two timeout/unqualified cells targeting AMD64. These eight raw records lack server receipt support and are not accepted copy matrix cells or speed measurements. All three servers are incomplete with zero receipts. Their client-stop, server-stop and stream-cleanup flags are serialized true; they do not independently establish fresh physical absence.

The rollup reports `FAIL`, `validation_or_platform_failure`, zero passed cells and no missing records. Normal AMD64 terminal artifact uploads exist and match diagnostic terminal bytes. That contradicts a simple supervisor-artifact-error → final-cleanup kill → new normal-terminal-upload narrative; the actual first cause and concurrent timing remain unresolved. No supervisor-killed-child cause is established.

## Native candidate

| Platform | Default native library | Installed-feature native library |
| --- | --- | --- |
| macos-arm64 | 632 passed / 3 ignored; 719.60s | 878 passed / 3 ignored; 699.00s |
| linux-arm64 | 633 passed / 3 ignored; 1025.44s | 879 passed / 3 ignored; 1104.94s |
| linux-amd64 | 633 passed / 3 ignored; 1951.57s | 879 passed / 3 ignored; 1971.20s |

Counts overlap and are not summed. The prior authority-withdrawal regression passed in both configurations on every platform. Native build, dependency/ABI, licensing, packaging and tooling steps passed; macOS ad-hoc signing and bootstrap rendering passed. All eight candidate ZIPs and all product/tooling file pins were independently verified. JSON retains exact suite times, jobs, log hashes and artifact provenance.

Ignored identities below describe these actual candidate invocations, not separate historical provider qualification. Ignored entries are not counted as executed passes. The five identities per platform span three test binaries: one engine-library entrypoint, three native-library entries and one native-vault integration provider gate. The table reports only the native library’s three ignored entries.

**macos-arm64**

- `live::tests::fd_probe_child`: exec-only descriptor probe, invoked by multithreaded live-spawn test.
- `copy_integration_tests::helper_child`: owned helper subprocess entrypoint.
- `copy_integration_tests::large::large_raw_zlib_actual_helper_four_way`: expensive >65MiB actual-helper four-way qualification; exact-name invocation only.
- `stream_runner::tests::large_duplex_integrity_with_slow_consumers_and_bounded_windows`: large 65 MiB duplex qualification; run explicitly.
- `macos::disposable_keychain_round_trip_lock_and_restart`: explicit native gate: creates only an owned disposable macOS keychain.

**linux-arm64**

- `live::tests::fd_probe_child`: exec-only descriptor probe, invoked by multithreaded live-spawn test.
- `copy_integration_tests::helper_child`: owned helper subprocess entrypoint.
- `copy_integration_tests::large::large_raw_zlib_actual_helper_four_way`: expensive >65MiB actual-helper four-way qualification; exact-name invocation only.
- `stream_runner::tests::large_duplex_integrity_with_slow_consumers_and_bounded_windows`: large 65 MiB duplex qualification; run explicitly.
- `linux::isolated_gnome_keyring_round_trip_and_lock`: explicit native gate: creates only an isolated temporary D-Bus/keyring session.

**linux-amd64**

- `live::tests::fd_probe_child`: exec-only descriptor probe, invoked by multithreaded live-spawn test.
- `copy_integration_tests::helper_child`: owned helper subprocess entrypoint.
- `copy_integration_tests::large::large_raw_zlib_actual_helper_four_way`: expensive >65MiB actual-helper four-way qualification; exact-name invocation only.
- `stream_runner::tests::large_duplex_integrity_with_slow_consumers_and_bounded_windows`: large 65 MiB duplex qualification; run explicitly.
- `linux::isolated_gnome_keyring_round_trip_and_lock`: explicit native gate: creates only an isolated temporary D-Bus/keyring session.

## Recovery and review status

A local host reboot erased private temporary coordination/auditor files. Committed source and public GitHub artifacts survived. The exact successful candidate was reused, and evidence/state moved to durable private storage. The original reviewed serial sequence resumed manually; no replacement coordinator was launched.

An earlier actual Claude Code coordinator-review invocation failed OAuth authentication before inference and provided no approval. Authentication subsequently recovered. The first corrective observability design review (session `5ddad58b-df7e-4420-8e3e-9d99a309c6c3`) returned NOT APPROVED and prompted revision; that is separate from review of this evidence. The historical authentication failure is not a current global blocker. Independent integrated evidence/code approval remains pending; this report does not claim it.

The five undispatched cohorts are edge-stream, edge-rsync, edge-copy, copy-interrupt-put and copy-interrupt-get. No final auditor ran on this failed chain. Prior failed/partial candidates and legacy CI failures remain retained and unwaived. This source-only qualification increment creates no new public release, tag or installer pin.
