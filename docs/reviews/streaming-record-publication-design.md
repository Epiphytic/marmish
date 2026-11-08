# Record-publication design reviews

Actual Claude Code reviewed the [design](../phases/streaming-record-publication.md) against `e31b8afae452bb30a041916fbf6d2bac4e92f042` on 2026-11-08, using `--permission-mode auto --model claude-sonnet-5-5`. The [machine-readable record](streaming-record-publication-design.json) pins the private raw results. These are design reviews, not integrated code or release approval.

| Version | Session | Verdict | Execution | Raw result SHA256 |
| --- | --- | --- | --- | --- |
| 1 | `7852fab1-48ee-4570-ad9f-717183f7a459` | NOT_APPROVED | 123292 ms / 21 turns | `48253d606e3d6656cb9f6e53887eaeb2f447de403c2f90229d55410d2cad42e9` |
| 2 | `b8058f3e-2cdf-43d0-9624-8418d0469246` | NOT_APPROVED | 118629 ms / 20 turns | `d447cf53960e0f81b604df47224190033d5bb88ef31935184da07129d961fa66` |
| 3 | `1670a315-634f-472b-810f-3249668c5594` | APPROVE | 105455 ms / 21 turns | `b76de4b83541b6f15f525aa9d7f3b8842ec1e00b1e6d7b64e76a4a4104f9dd17` |

The first review required coverage of the rsync and signals live marker readers, explicit strict ENOENT behavior, original timeout budgets and retained native validation. The second required the quota marker reader. Version 3 approved all seven identified live read sites: four inbox loops and three marker readers. Its approved snapshot SHA256 is `c3a9eef82f54600c7a2437911d8e5c87b14ef0131dcedb3235957f7c103fbea8`; the repository plan adds approval metadata and a marker-count typo correction.

The approved boundary treats an otherwise-safe, owned, bounded regular file with exactly two hard links as pending during live polling, before reading bytes. Ordinary and offline reads remain strict, writers remain unchanged, and all other unsafe metadata remains an error. A second read after observing one link remains strict. Pending state consumes the existing polling cadence and deadline; an established signals marker becoming pending is an error. Reviewers requested literal site-wiring guards and malformed-JSON pending coverage, with runtime coverage distinguished from source assertions.

The reviewers used read-only source and evidence inspection; they did not run Cargo, relay workloads or hosted CI. Separately, the actual native retained-record validator passed one test (0 failed, 0 ignored, 887 filtered out), covering all three published client records with six successful pairs each. Its temporary test patch was restored to the exact original source hash. The JSON records the command and private evidence/log/patch hashes. This validates retained client payloads against the actual native validator; it does not recover failure-time macOS inbox metadata, missing server receipts or the hosted cause.

The publication/read contract defect is confirmed in source and a held-cleanup publisher experiment. The initiating e31 hosted rsync failure remains unknown. Integrated implementation review and a new exact-source candidate, all eight original serial cohorts and final auditor remain required.
