# Local Get receiver lifecycle and GC design review

Scope: [the scoped design](../phases/streaming-copy-local-gc.md), supplementing the
approved native-copy phase. This review does not approve implementation until
its blocking findings are resolved. No local receiver GC code is implemented by
this design review alone.

## Round 1

Actual independent Claude Code review with `--permission-mode auto`, model
`claude-sonnet-5-5`, session `c459ff74-6ac6-44b0-b4a9-6544c465c6d7`.
Verdict: **REQUEST_CHANGES**. Read-only source/design inspection; no Cargo, tests,
production binaries, network qualification or code edits were run by the reviewer.

The reviewer accepted encrypted actor-owned indexing and bounded frontend-run GC
without a CommandEngine, but found six blocking gaps:

1. Raw relative destination paths lacked a stable receiver base; absent state in
   another cwd could falsely delete the real index. Revision 2 records absolute
   base plus actual base/parent identities and checks them before absent success.
2. Rejecting unknown/active rows permanently broke crash Resume. Revision 2 lets
   Resume supersede intents while physical receiver/admission locks still reject
   a live worker. This never permits GC from free-flock evidence alone.
3. Single-shot non-idempotent IPC could strand leases/claims after reply loss.
   Revision 2 specifies bounded replay-safe requests/results, exact revisions,
   repeatable claims under locks, and index-only explicit reconciliation.
4. Repeated oldest retained rows could starve collection. Revision 2 gives finite
   outcomes, checkpoint-expiry propagation, retry backoff and bounded fair sweeps.
5. Attempt binding could advance before filesystem last_stream. Revision 2
   separates intent from confirmed physical receiver establishment at release.
6. Local refusal after remote Open could orphan a helper. Revision 2 reserves
   before Open, activates against its persisted record afterwards, and uses finite
   Error+EOF only for deterministic post-Open local failure.

Additional clarifications now specify the reservation matrix, bounded row/claim/
receipt counts, worker ownership outside the cancelled future, dropping both
worker channels before confirmed thread completion, and direction-specific Get
reclaim without weakening server Put or unknown-launch GC rules.

The draft was refined for physical last_stream and worker ownership while round 1
was inspecting its earlier snapshot. Round 2 explicitly reviews the final full
revision rather than inferring approval from those concurrent edits.

## Round 2

Actual independent Claude Code `--permission-mode auto` review, model
`claude-sonnet-5-5`. Verdict: **REQUEST_CHANGES**. The reviewer inspected actual
filesystem, worker, CLI, IPC, runtime and store contracts without edits, Cargo,
tests, binaries or network qualification. It confirmed all six round-1 blockers
resolved, then identified three bounded followups:

1. Reserve-before-GC deadlocked at the row cap. Revision 3 sweeps before Reserve,
   excluding the intended namespace.
2. Ordinary failures before Receiver submission leaked unknown intents. Revision 3
   adds NeverSubmitted/Attempted/Established release dispositions, predecessor
   rollback and bounded unactivated-intent pruning behind mandatory Activate.
3. Persistent claims could pin all eight slots. Revision 3 enumerates finite
   error classes and five-grant exponential retry exhaustion into retained unknown
   state, releasing the claim slot.

Revision 3 also makes daemon retention replies, reserved base checks, Activate
ordering, non-command IPC deadlines, bounded identical-request retries and replay
receipt retirement explicit.

## Round 3

Actual independent Claude Code `--permission-mode auto` review, model
`claude-sonnet-5-5`. Verdict: **REQUEST_CHANGES**. Read-only review confirmed all
three round-2 blockers resolved, plus identity anchoring, physical Resume locks,
worker shutdown proof and client-only feasibility. It identified one remaining
blocker: bounded replay receipts could themselves refuse collection and recreate
a capacity deadlock. Revision 4 defines all replay-record producers, atomic oldest
record eviction with a monotonic acceptance floor, and no receipt-cap refusal for
Release/Prune/Activate/GC/Reserve. Validation fills the cap and exercises those
operations. It also clarifies the narrow activated-before-filesystem crash,
chained supersede, stale release and unresolved target-claim recovery.

## Round 4

Actual independent Claude Code `--permission-mode auto` review, model
`claude-sonnet-5-5`. Verdict: **APPROVE**. The reviewer read the complete scoped
design, round-3 findings and relevant foundation/replay-floor contracts; no edits,
Cargo, tests, binaries or network were run. It confirmed receipt-cap starvation
resolved and the earlier safety boundaries preserved, with no remaining blockers.

Three implementation cautions were clarified directly in the approved design:
the local floor rejects IDs at or below the floor (unlike the existing remote
strict-less-than helper); delayed Activate is refused by its missing intent or
revision while the floor fences Reserve replay; oldest receipt means minimum
request timestamp including a backdated incoming Prune record. The validation
matrix now names these exact boundaries. This is independent Claude review of
Codex-authored design, not a claim of cross-model review between Claude rounds.

Design approval does not claim implemented lifecycle/GC support. Source changes,
meaningful hermetic tests and independent completed-code review remain required.
