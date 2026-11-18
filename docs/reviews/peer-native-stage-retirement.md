# Peer native stage retirement review

Claude Code Sonnet 5.5 independently reviewed the
[design](../phases/peer-native-stage-retirement.md) and code in Auto mode.
The final reviewed private commit was
`b589a5d5db9a85678d6f2b1c9bfd9985513e2170`
with tree `6f731876ab4f6728927595c10fcea4c43842912f`
over the stage-binding checkpoint `f4399f5400bd861a6a3cfaf853157fa22d2daaf4`.

| Review | Claude session | Exact scope | Verdict |
| --- | --- | --- | --- |
| Design 1 | `5e638d0c-45f1-4d0f-b3b9-0ebb2883eccd` | SHA-256 `548ebf90139c8505a3a04cb0377bee173879c5e9ac81a56f86816452c8281410` | Changes required |
| Design 2 | `d4a3aaed-57eb-4913-b0b8-8224b485dd7f` | SHA-256 `d287753e4b75df8a7fdd1b4adee22a0eafc592962ece518ce709604ad6903f53` | Changes required |
| Design 3 | `57705e29-5392-4210-8b42-33da3449d1ff` | SHA-256 `955932431787be05a2e80af610ff6b744731ca141bd3ac8808a9c08afd97d8b0` | Approved |
| Code 1 | `88c9bab6-fea9-4577-9728-c42837bb31ee` | Tree `713bafea9f072b5127d70fb559a128a9d451d87f` | Approved |
| Code 2 | `622391bc-4f7a-4da2-835f-f76c912458b9` | Tree `6f731876ab4f6728927595c10fcea4c43842912f` | Approved |

Design review required a verified upstream-capability boundary, a distinct
export-incapable epoch, complete schema validation before any migration,
fresh authorization at every transition, descriptor-relative file checks
and a durable date-bounded receipt. The first code review verified the
source tree and independently passed 11 focused tests and an ordinary
library build with retirement fixture symbols absent. It suggested keeping
a temporary filesystem error retryable instead of permanently blocking
the stage and charging the inviter cooldown. The correction applies
a durable Blocked decision only to a definite identity mismatch and adds
an unavailable-cursor regression.

The final reviewer verified the source and design hashes and independently
passed all 12 focused tests. Their scratch run with the earlier error
handling failed the new regression as expected. Confirmed unsafe file or
directory identity still records a Blocked intent. An unavailable path
leaves custody in place and a retry can continue after the path is restored.
The final review found no blockers. Its reviewed source SHA-256 patch
digest is `8b622a1ab20d8799fb73ef531682c8d52823c1d4810ce03019950ee39b74a698`.

The early Rust lane continues to use an exclusive profile lease and
`&mut self`. The SQL stage transition guard is a second fence. The
fixture approval covers retirement of a stage before KeyPackage export.
It establishes neither live Consent publication nor native leave.
[Author validation](../validation/peer-native-stage-retirement.md) records
the measured local behavior.
