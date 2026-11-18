# Prepared Consent publication reviews

Claude Code `--permission-mode auto`, Sonnet 5, reviewed the design and exact
code in an isolated Linux AMD64 worktree. Beads `marmish-dwt.4.4.3`.

| Review | Scope | Session | Verdict |
| --- | --- | --- | --- |
| Design R2 | `b9ba9076543e5bd73344bd1e9801c43b7228d6b8` | `1d9275f2-33c9-49f1-8a7b-e41a796a468b` | Approve |
| Code R1 | `5e7b854d0d2bc02861dc805df60d67fac297a4b6` | `4db751c8-aba0-47e1-ade7-45ce45f09bca` | Request changes |
| Code R2 | `3b8308f7baf2dc69f269f66bde50a95dc5009cbf` | `86ca4428-9819-4666-b77a-6111a275df23` | Approve |

R1 found that the WSS tests configured two fault cuts without executing them.
The implementation now replaces the witnessed stage inode after split-sink
staging and enables SQLite read-only mode before the settlement CAS. The exact
R2 run confirmed zero relay writes after the inode refusal and one relay
write followed by a durable settlement failure in the read-only case.

R2 confirmed the once-only flush hook, per-poll physical fences, owner-local
attempt and port-scoped ACK, cold retry, and test-only publication entry
point. The reviewer independently ran the four fast authenticated route
cases. The author ran all six WSS cases with real time and a separate existing
WSS ACK regression, plus strict Clippy and standalone cursor tests. See
[validation and exact selectors](../validation/peer-installed-consent-publication.md).

The design now clarifies what independently binds the approved frame. The
fixture takes one fresh route sample after the synchronous digest, on the
first potentially exposing flush poll. The inode-refusal case covers
retirement and zero relay writes. A separate cancellation-during-hook test
was a possible extension and was outside the completed proof. No blockers
remained in R2.

## Integrated source review

Claude Code Sonnet in Auto permission mode, session
`7a6984b7-032a-4046-b323-c82fddcd76a1`, approved exact integrated source
`2c18a7de05d14a0e589fcd0f1c20cb424c3d4638` on `515557a`. The reviewer
confirmed that added and removed source lines match the isolated author
delta across all 15 files. The Founder and Consent port modules are both
registered. The review checked ordinary raw socket callers, the shared
SQLCipher compare-and-set, the pre-flush hook and the ACK witness. The
integrated validation ran separately.

The base-relative Ripwire report flagged fixture-only relay publication
complexity (34 to 37) for the explicit negative ACK fault, plus the new
raw-write hook parameter and test-support overlap. The Auto review checked
the shared write behavior and the fixture's failure paths. The existing
callers pass a no-op hook and the production build has no Consent entry.
