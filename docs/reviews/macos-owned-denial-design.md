# Owned macOS denial diagnostic: design review

Reviewer: actual independent Claude Code `claude-opus-5-5`, auto permission mode.
Scope: proposed `docs/phases/macos-owned-denial-diagnostic.md` at HEAD `983a678`,
initial SHA256 `f5275db38fe4062c6f836000af4316f3a6d7490ed759abca1e456ea35fde56c1`.
The reviewer read the proposal, existing probe/crash drivers, workflow, production
PROFILE and prior dyld evidence. No code edits, native queries, tests, public
lookups or workflow dispatch were performed. Raw local review evidence:
`/tmp/marmish-macos-denial-design.raw.json`.

**Initial verdict: NOT APPROVED for implementation.** Subsequent approval and
condition disposition are recorded below.
The reviewer accepted the conservative scope, trust caveats and capture budgets,
but required B1–B5 contract corrections. The revised proposal disposes them as:

- B1: kernel-only predicate and exact returned `/kernel` processImagePath;
  sandboxd is excluded. CLI/JSON expectations remain unverified and fail closed.
- B2: finite expanded operation subset, explicitly not a complete vocabulary;
  current PROFILE grants explained; exact proposed cryptex aliases, bounded
  numeric subcache suffixes and target-none class; all other targets unclassified.
- B3: finite content-free format-shape diagnostic, with strict JSON array parsing;
  NDJSON/trailing summaries remain format_unknown until separately reviewed.
- B4: literal generated UTC timestamp format, floor/ceil inclusive window,
  explicit-offset bounded returned timestamps and wall/monotonic consistency.
- B5: total category precedence, empty results on all limit/format failures,
  mixed known/unknown operation semantics, and explicit UUID-image match even
  for exact/alias crash paths. Crash attribution never authenticates log content.

Useful nonblocking findings also incorporated: <=15-character basename,
pre-exec sandbox-exec exclusion, separate workflow-test ownership, exact full
remaining-line target comparison including spaces, strict UTF-8/duplicate-key/
nonfinite-number/recursion handling, new-session group kill before leader reap,
and reliance only on the account's existing log-read authorization. No privilege,
profile or runtime-acceptance change is proposed.

The reviewer's CLI/schema/image-path, cryptex and operation spelling claims are
explicitly unverified memories, not primary-source or native evidence. They must
be checked by the reviewed native format-validation run, with incompatible
formats remaining inconclusive. A candidate denial is not causal proof or
permission allowance. Follow-up independent Claude design approval is required
before any implementation; subsequent code and native results need their own
review and must preserve the configured SIGABRT as failure.


## Second independent review and residual amendment

Actual Claude Code `claude-opus-5-5` read the amended proposal at SHA256
`2248a75faff4c8296894572cb3fe040fe66117bdb7ff94c3a56bdc04181433ea`,
the review record and actual crash-attribution helpers. Raw local evidence:
`/tmp/marmish-macos-denial-design2.raw.json`.
**Verdict: NOT APPROVED for implementation.** B1–B4 and most of B5 were resolved;
follow-up approval remains pending after these two narrow residual corrections:

- R1: expected records require bounded-string timestamp/processImagePath/
  eventMessage fields; extra keys are ignored and never exported. A successful
  query returning [] means no_owned_denial; zero stdout is format_unknown.
- R2: every returned-but-rejected record invalidates the entire collection as
  format_unknown with empty denials/null count, above matching_limit precedence.
  The plan explicitly enumerates outcomes for image/window/grammar/token/control/
  multiline/sandbox-exec/deny-counter and other negative fixtures.

Nonblocking clarifications incorporated: matched_count equals unique pair count
(len(denials)); actual waitid/WNOWAIT support is checked before compilation with
fixed outer query_observation_unsupported on failure; capture-size polling is at
most 50 ms. The preflight uses only an owned benign child, no privilege/policy
change. The reviewer performed read-only inspection only: no edits, tests, log
queries, native execution, network or CI actions. Unknown native contracts,
privacy limits, candidate-only evidence and the unwaived SIGABRT remain unchanged.
No approval is inferred from these dispositions; another design review is due.


## Third independent review: APPROVED with C1 incorporated

Actual independent Claude Code `claude-opus-5-5` reviewed the revised plan at
SHA256 `1b642abea4c08f3cc909ca0fdba478ca3e056fde448767a6ff47991427808025`,
the review record and prior residual review. Raw local evidence:
`/tmp/marmish-macos-denial-design3.raw.json`.
**Verdict: APPROVED with condition C1.** The reviewer explicitly stated the
required doc-only correction needs no further design review.

C1 is incorporated: unsupported non-reaping observation is the preflight outer
query_observation_unsupported/log not_queried result before compilation;
query-time observation failure after successful preflight is query_failed.
Never signal a process group after its leader has been reaped. Nonblocking
clarifications also incorporated: oversized record fields are format_unknown
with empty denials/null count, preflight timeout/exception is
query_observation_unsupported, and its two seconds count within the unchanged
ten-minute workflow budget. Current approval wording replaces stale pending
status; earlier NOT APPROVED verdicts remain historical evidence.

The reviewer stated verbatim: "This approval counts as the independent design
review only. Your protocol still needs a human to approve the design before
implementation starts." Whether further human approval is required and whether
it has already been given must be determined from the governing task
instructions. This record grants no implementation or native-run authority.

Final phase-plan SHA256 after C1 incorporation:
`f40e8b2585fa5fd3cd795d9be00985f06c1e0396219847668450953611606a95`.
The latest independent evidence check
(`/tmp/marmish-managed-native-evidence2.raw.json`) was NOT APPROVED for this
review-record wording. Its condition is now corrected by the verbatim quotation
and authority limitation above. Independent follow-up by actual
`claude-opus-5-5` explicitly APPROVED publishing these four design/evidence
documents (`/tmp/marmish-managed-native-evidence3.raw.json`). The phase-plan
and aggregate hashes remained unchanged; no implementation or native authority
was granted.

The reviewer only read files and performed no edits, tests, native commands,
log queries, downloads, CI or admin actions. Approval covers the detailed design
and bounded file ownership only. Native schema/CLI contracts remain unverified;
privacy, confinement and candidate-only interpretation remain mandatory.
Integrated code review and a separately reviewed native format-validation run
are still required, and configured SIGABRT stays failure independently of the
managed-runtime native acceptance gate.
