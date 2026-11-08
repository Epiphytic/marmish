# Partial installed qualification: 2f98802

[Machine-readable pins and results](streaming-installed-2f98802.json).

Source `2f98802d25e2a6ec9ebd995adfef3504169175b7`; candidate [42435626121/1](https://github.com/Epiphytic/marmish/actions/runs/42435626121) passed all three platform packages and render. Version 0.2.3 is a candidate, not a newly published release.

The chain stopped at edge-copy. Five passing cohorts contain 90 validated rollup observations; qualification remains false. Neither interruption cohort nor the final auditor ran.

| Cohort | Run | Result | Observations | Wall range(s) | Application goodput(B/s) |
|---|---:|---|---:|---:|---:|
| stream-matrix | [42442857168](https://github.com/Epiphytic/marmish/actions/runs/42442857168) | success | 9 | 47.565601205–50.884936821 | 4024.766715–4305.632533 |
| rsync-matrix | [42443132988](https://github.com/Epiphytic/marmish/actions/runs/42443132988) | success | 18 | 36.246406875–45.099938108 | 726.564190–904.034436 |
| copy-matrix | [42443528275](https://github.com/Epiphytic/marmish/actions/runs/42443528275) | success | 18 | 22.424255257–53.140277666 | 1233.264162–2922.549679 |
| edge-stream | [42443846881](https://github.com/Epiphytic/marmish/actions/runs/42443846881) | success | 27 | 0.003200749–9.996986326 | not measured |
| edge-rsync | [42444107545](https://github.com/Epiphytic/marmish/actions/runs/42444107545) | success | 18 | 1.569847917–46.449560792 | not measured |
| edge-copy | [42444374634](https://github.com/Epiphytic/marmish/actions/runs/42444374634) | failure | 0 | unavailable | not measured |

Stream goodput uses 204800 validated bytes per observation; rsync uses 32768 and copy 65536. Wall includes installed CLI/command/transport work, not independent receipt validation. Functional edge timing includes local observations/cancellation/cleanup. These are not RTT, wire speed, compression ratios or percentiles.

All five passing cohorts independently cross-check 12 canonical records each, installation/source/version/manifest/bootstrap/tooling pins, payload/receipt proofs, metric arithmetic and reported cleanup/stop flags. This is offline evidence inspection, not a second physical-process or transport observation.

Edge-copy retained all 12 canonical records, with byte-identical diagnostic duplicates. Every identity/server-ready chain is valid. All three clients and servers report incomplete; edges are null, pairs/failed_cells/receipts empty, client server-ready pin maps empty. The failure rollup reports validation_or_platform_failure, zero passed cells and no missing records. No failed case or stage is retained, so these records do not establish a cause or prove no remote operation occurred. All three roles report stream cleanup and client stop true, server stop false; the flags are preserved separately.

Native default libraries: macOS engine 148 passed/1 ignored, native 629/3, stream 29/0, vault 13/0; Linux engine 141/1, native 630/3, stream 29/0, vault 12/0. Installed-checks native libraries: macOS 869/3, Linux 870/3. Counts overlap and are not summed. JSON retains actual per-suite times, ignored identities/reasons and successful build/signing/ABI/license/package/tool-isolation steps. All eight downloaded candidate ZIP hashes/sizes and product/tooling provenance bytes were independently checked.

The parallel legacy workflow has three passing installer jobs and three failing managed-packaging jobs. Of 19 unique hosted failed identities, 10 locally reproduce obsolete assumptions and 9 locally pass with hosted causes unresolved. None is waived; failures before intended security assertions do not prove those assertions. Exact local fixture class passed 6 tests with no skips and private locked/offline builds.

Both source refs and Radicle sync were verified by the coordinator. GitHub exact public bytes matched. Two Iris HTTP timeouts leave complete external Radicle HTTP availability unverified.

The accompanying JSON pins all candidate artifact ZIPs, products/tooling, contexts, rollups, canonical record hashes, jobs and private evidence input hashes without publishing identities, paths or raw logs. Independent integrated Claude review of this evidence and correction is pending. The [design review](../reviews/streaming-copy-edge-correction.md) does not substitute for code or qualification approval.

A separate source audit identifies a deterministic fixture obstruction: client/server endpoints use the same workspace and edge-copy exclusively creates the same nonce directory twice before its first case ([pinned source](https://github.com/Epiphytic/marmish/blob/2f98802d25e2a6ec9ebd995adfef3504169175b7/crates/marmish-native/src/installed_edge_copy.rs#L552)). This explains a source-level failure path; no historical runtime errno or stage was retained. The [approved corrective phase](../phases/streaming-copy-edge-correction.md) separates owned roots and adds bounded failure breadcrumbs. Implementation is being reviewed separately; these historical results do not validate the correction or qualify a future source.
