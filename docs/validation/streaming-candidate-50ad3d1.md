# Native candidate 50ad3d1

Candidate [42416184935/1](https://github.com/Epiphytic/marmish/actions/runs/42416184935)
built source `50ad3d12abaa7698737bf23c68ffd7002c07112d`. All three package jobs
and rendering passed on 2026-11-07. This is candidate/native evidence, not a
release or completion of installed qualification. Public installation remains
v0.2.3. The candidate used the reviewed 120-minute package cap; no suite, workload
or installed deadline was reduced.

| Native job | Elapsed | Engine library | Native library | Installed-feature native library |
|---|---:|---|---|---|
| Linux AMD64, 128534679990 | 3933s | 141 passed/1 ignored, 9.34s | 630 passed/3 ignored, 1088.54s | 843 passed/3 ignored, 1089.73s |
| Linux ARM64, 128534680226 | 3803s | 141 passed/1 ignored, 9.51s | 630 passed/3 ignored, 1070.15s | 843 passed/3 ignored, 1109.88s |
| macOS ARM64, 128534680282 | 3446s | 148 passed/1 ignored, 37.01s | 629 passed/3 ignored, 638.79s | 842 passed/3 ignored, 683.66s |

Every listed library block had zero failures. Default and installed-feature
counts overlap and must not be summed. Existing ignored tests remain ignored;
they are not successful execution evidence. Stream 29, vault 12 on Linux and
13 on macOS, and engine process limits integration 1 passed in each applicable
configuration. Strict Clippy,
release binaries, native dependency/ABI/license checks, separate installed-tool
builds, product-hash preservation and packaging passed on all targets. macOS
ad-hoc signing and verification passed. Render job 128550338698 passed in 8 seconds.
AMD finished 65m33s into its 120-minute cap. This measures observed elapsed build
time, not a causal explanation of runner variation or transport headroom.

The saved GitHub run response SHA256 was
`04e0606d7825a10e1d6f014d4694c8d016cd8f39495347f91c44f715ad6ed62a`;
its complete jobs response SHA256 was
`e3214fa466704030e469276d14f86c1a17c83936692a3c4da4c215ed12199e93`.

| Full package job log | SHA256 |
|---|---|
| Linux AMD64 | `669965182ef684a02fdb1c7b067b327496a2adfb03e4cb7abda8d8137ac2330a` |
| Linux ARM64 | `71c7884ae1829dff748bfcc0805c57d0a05d1981e4bf8770258626a492f342fc` |
| macOS ARM64 | `f0117586774b17a7ebb093635a3d0e40bc29a68d2e2a0d30d186cab12e9bf765` |

The first three installed matrices passed, then edge-stream failed on all three
hosts and the serial chain stopped. See [installed evidence](streaming-installed-50ad3d1.md).
Neither the passing candidate nor those partial matrices qualify the complete
initiative. Corrective source requires a fresh all-platform candidate and all
eight original cohorts, bound to its new exact source and attempt.

The separate legacy managed-validation run 42416166342/1 remains red in its three
full Python tools suites; its three legacy installer jobs passed. Several focused
reproductions identify historical Python installer-marker assumptions. A later
bounded reproduction identified the native HTTPS cleanup-contract fixture's
dependence on a short temporary parent: the managed runner's longer TMPDIR
reaches the runtime-path length guard before the mocked installation. This
does not establish every historical subcase's cause. The additional macOS
failure was the denial-diagnostic unittest, not the legacy native probe. Its
exact failing assertion and capability state were not retained and remain
unexplained; the legacy probe produced its expected guarded refusal.
These failures are not waived or reclassified as harmless. Native candidate
success does not mean all repository CI is green.
