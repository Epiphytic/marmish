# Candidate build budget reviews

Scope: package job cap in `streaming-candidate.yml`,90→120minutes, and its
job-scoped policy test; coordinator-owned evidence/specification documentation.
Product behavior, tests, transport deadlines, installed workflow65-minute cap,
workloads, acceptance and candidate provenance stay unchanged. No version, tag,
public release or installer pin change is authorized by these reviews.

## Independent actual design reviews

Actual Claude Code `--permission-mode auto --model claude-sonnet-5-5`, session
`17daba31-37b8-4b31-aa3a-4f679cc87085`:

| Round | Verdict | Duration | Turns | Private result JSON SHA-256 |
|---|---|---:|---:|---|
| 1 | APPROVE conditional design only | 46.933s | 5 | `f72441cb0f78981945a376a9750acc34266a6ce33fd538e55b4187edc1f8fade` |
| 2 | APPROVE revised conditional design only | 11.268s | 2 | `d747bef58530dc8d0888981f26fa327322bc4ab88722ec33011eccdc7d4c143f` |
| 3 | APPROVE triggered design for implementation | 43.816s | 6 | `16e2389f5278886ffffad0b12dea900a00d531a0868862f14be5f3f25dd49921` |

Round1 required an evidence-based timeout trigger rather than relying on a
GitHub conclusion label, exact package/render cap scope, exclusion of the
separate version-lifecycle workflow, and explicit binding of the passing macOS
diagnostic to its old SHA. Round2 approved those corrections. These approvals
preceded the actual timeout and did not authorize implementation then.

After candidate42408127994/1 completed, round3 inspected actual completed
run/job API evidence, timeout annotation and current/baseline logs. It verified
5416s job elapsed, successful native/installed-feature tests, strict Clippy and
active release compilers at cancellation. The revised approximately100m23s
completion estimate supersedes the earlier94m20 projection. The120-minute cap
provides bounded estimated headroom, not guaranteed completion or fourfold
transport margin. No failed test is suppressed or timeout reclassified as PASS.
The reviewer found no blocking findings. It noted workload and timing variance
remain uncontrolled; if120minutes also expires, preserve and investigate rather
than silently increase again.

The actual reviewed private plan SHA-256 was
`36497def2b7a574075d044eda07eed4b60a08e9e5080d866e555fb7ba33090d6`.
The repository copy adds this review link and marks the earlier projection
superseded. Reviewers made no source edits, Cargo, DB/Beads, dispatch, cancel or
rerun calls. Round1 queried status read-only; round3 read actual saved evidence.
The absence of an external cancellation request is a session report, not an
independently inspected GitHub audit log; the actual timeout annotation and
elapsed time establish the observed budget expiry.

Approval was recorded here before delegated implementation. Beads task
`marmish-8ka.5.8` remains open: installed clients expect schema66 but the shared
tracking database reports53. No migration, schema bypass or manual database
change was attempted. A separate source-verified schema53 CLI was built in
private temporary state, but its startup maintenance is not certified free of
migration/backfill side effects; it has never been executed against the DB.
This is partial tracking, not a claim of synchronized Beads completion.

A fresh reviewed source SHA must get a new complete three-platform candidate
and all eight original installed cohorts. The cancelled2ebc8b6 candidate remains
unqualified, and its two successful ARM jobs cannot be mixed into new artifacts.
Integrated code/test review and actual fresh execution are still required.


## Integrated source and evidence review

Actual Claude Code AUTO resumed the same session, APPROVE,50.708s/7turns;
private result JSON SHA-256:
`60941812dab01909318628ee68e72bd0858bdf3be5cd99e1b7fe2ac555c0dcdf`.
It inspected both source files, all tracked documentation and the six new
evidence/design/review documents, excluding only the passive Beads export.
The reviewer confirmed the exact one-line workflow change, all new scoped and
negative policy assertions, and preservation of every existing assertion.

It independently reran the focused7 and full158 Node tests, whitespace checks
including new documents, and matched the hosted macOS summary byte-for-byte to
the private reviewed summary. It verified all four failed-candidate API hashes,
three platform log counts/timings, AMD log hash and job times. No blocking
findings. It did not fully inspect the quality/change-context logs or fetch the
separate legacy workflow; those checks/observations remain root-reported,
explicitly limited evidence. Cosmetic doubled whitespace in the specification
was corrected. No source behavior changed after this review.

Approval covers this source publication and beginning a fresh exact-source
candidate only. It does not qualify the old or new candidate, authorize release,
or change any version/tag/public installer pin. No Cargo, DB/Beads, source edits,
network calls or Actions mutations were performed by the integrated reviewer.
Fresh three-platform candidate and all eight original cohorts remain required.
