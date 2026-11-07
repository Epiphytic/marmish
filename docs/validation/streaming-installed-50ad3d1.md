# Installed streaming chain at 50ad3d1: incomplete qualification

Source `50ad3d12abaa7698737bf23c68ffd7002c07112d`, candidate
[42416184935, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42416184935),
passed its three platform package jobs for Linux AMD64, Linux ARM64 and macOS
ARM64. The first three installed cohorts passed. The fourth, edge-stream,
failed on all three platforms and stopped the serial chain. **The complete
chain has zero qualified results.** These historical successes do not qualify
a corrected source revision, publish a release, or change an installer pin.

## Completed installed cohorts

Each directed platform pair includes same-platform pairs. Stream has one case
per pair; rsync and copy each have two directions per pair. All rows below are
one sample per directed case, not latency percentiles or independent repeated
benchmarks.

| Cohort | Workflow run, attempt 1 | Passed cells | Validated payload per case | Installed CLI wall time | Application goodput |
| --- | --- | ---: | ---: | ---: | ---: |
| Stream duplex | [42421449886](https://github.com/Epiphytic/marmish/actions/runs/42421449886) | 9/9 | 204,800 bytes | 43.503189729–46.082796929 s | 4,444.174695–4,707.700775 B/s |
| Rsync push/pull | [42421748964](https://github.com/Epiphytic/marmish/actions/runs/42421748964) | 18/18 | 32,768 file bytes | 29.591311485–59.242203083 s | 553.119200–1,107.352069 B/s |
| Copy put/get | [42422110987](https://github.com/Epiphytic/marmish/actions/runs/42422110987) | 18/18 | 65,536 file bytes | 21.551321503–51.844735417 s | 1,264.082061–3,040.927211 B/s |

Wall time is the monotonic installed CLI lifetime, including transport, command
execution and output. It excludes independent receipt and cleanup validation.
Application goodput is validated payload bytes divided by that wall time.
These measurements are neither network RTT nor relay wire throughput and do
not establish a compression ratio. Duplex payload sums 98,304 stdin bytes,
98,304 stdout bytes and 8,192 stderr bytes; file modes count verified file bytes
rather than protocol framing or fixture-control traffic.

The retained records were independently cross-checked offline, without
rerunning product commands or transport. Each successful cohort contains the
exact 12 canonical identity/server/client records bound to its context,
source, candidate and attempt. All expected directed pairs and server receipt
sets are present. Stream reports exact-byte checks and exit 7; rsync and copy
report exit 0. All three successful cohorts report complete client/server
results and true stream-cleanup, server-stop and client-stop flags.

For rsync, all 18 client manifests equal their matching independent server
receipts and deterministic nonce-derived expected inventories: 16 files,
including an empty file, plus one directory. File sizes and hashes, modes,
mtime and special-character names match. For copy, all 18 source/destination
manifests and server receipt bindings match; each receiver checkpoint reports
committed, with the complete 65,536-byte durable prefix, matching content hash,
stream and transfer bindings. Both file cohorts have 18 measured transfer CLI
operations and 36 separately recorded fixture-control CLI operations. Control
proofs and stream/session/open/group bindings match their server receipts.
These are checks of retained reports, not a new physical filesystem inspection.

## Failed edge-stream cohort and stopped chain

[42422474341, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/42422474341)
failed all three platform jobs and its rollup. All 12 expected records are
present and all three server-ready records exist. Each client result has
`complete=false`, `edges=null`, no pairs and no retained failed-cell entries.
Each server result has `complete=false`, `edges=null` and an empty receipt set.
The final failure rollup reports `status=FAIL`, `qualification=none`,
`passed_cells=0` and no missing records.

The macOS job failed at “Run owned installed server and bounded transfer
cells” with the generic `invalid_or_failed` diagnostic. No retained edge case
or per-case failure stage identifies the underlying failure. The Linux records
likewise contain no completed edge evidence. Empty receipts do not prove that
no remote command executed: the driver publishes the family evidence only
after that work succeeds. Known harness defects under investigation do not
prove a unique historical cause of these failures.

All three failed-cohort server reports still have true stream-cleanup,
server-stop and client-stop flags. Those flags describe reported cleanup and
are not independent observations of physical process absence. The four
remaining cohorts—edge-rsync, edge-copy, copy-interrupt-put and
copy-interrupt-get—were not dispatched. Their negative, cancellation,
detachment and resume acceptance criteria remain unqualified by this chain.

## Bounded exact-candidate local diagnostics

A separate Linux AMD64 same-host diagnostic installed the verified candidate
assets through its rendered installer, with disposable client/server identities,
file-backed keys, explicit White Noise relay and fixture-only execution grants.
The three public admission commands returned the expected results: missing
executable and denied executable each exited255 with matching NotFound/Denied
terminal journals and one new Open; unsecluded rsync bootstrap exited255 before
adding a server stream. Their CLI wall times were3.766s,3.646s and0.052s.
This probe did not reproduce every harness binding or idle assertion.

A separate held-command probe explicitly prepared the private `edge-gates`
directory. It observed `hello` with active=false, then active Preparing and
Running, before sending SIGINT to the owned CLI. The CLI exited130 with confirmed
terminal diagnostics; the server journal reported Cancelled/inactive and cleanup
status reported zero active, cancelling, uncertain, owned and unfinished runners.
Both owned daemons then stopped with exit0 and true cleanup flags; owned private
state was removed. This validates that bounded Ctrl-C case only, not the complete
signals family, another platform or the failed hosted attempt. The directory
prerequisite was present in this probe and missing from the hosted harness source.

The first signal-probe script had an incorrect stream-prefix-length assertion;
its owned cleanup ran. Correcting that diagnostic-only assertion produced the
successful result above. It is not evidence of a product fix or a retry of the
hosted qualifier. Admission log SHA256:
`b91d515f353d0dd7f18c5a5516e9be0c0260c41380cc431ff9dfc2d23196d3da`;
corrected signal log SHA256:
`3091b9ba5923b739b025766680269197cea32f7ec6a7870dcc74605290055964`.
Raw process output and private profiles are not published.

## Retained artifact fingerprints

SHA256 values below identify exact retained context and rollup JSON bytes;
they are not independently downloaded ZIP checksums or executable rehashes.
Private records, keys, runtime paths and raw output are intentionally omitted.

| Cohort | Context SHA256 | Rollup SHA256 |
| --- | --- | --- |
| Stream | `e053021f2cd18ad916751ad263b73be7f472a2a97bd6bb13879e59898bb729bc` | `981752dce07cb5377c78164a8ce4f86849d3f46e9a32b9ad3a15d855f72f7211` |
| Rsync | `bff08abc3a602c275f7a739c6f0250918638b5c74aec28f2c03c36abb6f17e6f` | `4b0a5e870bd29ab88b0cd926c3c76915f4409151613702afed8ce45cedbc491d` |
| Copy | `3d7ff52c68fc49c84594bd6d11709c09443664f432d45f2e66a94b9f970ad8d3` | `6e2c898ca5afaffa2eb95b8f8f4c35df80e94e722347bbdc821667298e5016a6` |
| Failed edge-stream | `ecd5c7c273825d9228928315672610aa8a424d99139f548b5aac246df0692f93` | `179a2e1258300a4428c748a4be698b39b1a0dfd01cda29411ca3fa217f72c0c0` |

The failed edge-stream `matrix-rollup.json` and `matrix-rollup.failure.json`
have identical bytes and the last SHA256 above. Workflow conclusions were
checked against retained status snapshots; this document does not claim a
fresh GitHub-status query or reproduce the pinned Rust rollup renderer.
