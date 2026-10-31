# Native managed-suite fixture portability: design review

Actual reviewer: `claude-opus-5-5`, Claude Code in auto permission mode.
Verdict: **APPROVED for test-only implementation with conditions C1–C6**, no
blockers. It read the phase, actual report/JSON, eight failing cases and relevant
production contracts. No source edits, subagents, network, CI, Beads, git or
native-provider actions were performed. Raw local result:
`/tmp/marmish-managed-native-portability-design.raw.json` (not published).

The reviewer states verbatim: “Human approval is still required”. This review
record itself grants no human approval or implementation authority. Governing
direct-user phased-work instructions determine authorization independently.
Native matrix, separate SIGABRT and Linux namespace gates remain pending.

Independent disposable bounded probes: 15 affected cases passed on Linux; the
four FinalReport failures reproduced under a module-local Darwin sys stub. A
136-byte Darwin struct replica written from memory compiled with gcc/clang and
all 50 modes passed; that is explicitly not the actual SDK/native evidence.
Missing Bubblewrap or tool lookup masked both hash/home tests. Synthetic real
stat_result set-ID checks refused before access/hash and detected temporary-copy
removal of those rejection bits. Removing scan-result merge was undetected by
old report tests (0 of 3), establishing required new boundary assertions. The
reviewer ran no symlinked/full-suite/native check.

Dispositions, all incorporated into the phase:

- C1: Scanner argument/count/order and report merge asserted; separate direct
  scanner coverage retained; drop-merge mutation required. Exact review condition:
  “it must never be called on the secret-report or failure paths.” This applies
  to failures before scan. Teardown failure is discovered later, after the
  required scan-before-exit order; that case must still record the earlier scan.
  This necessary lifecycle nuance is disclosed for integrated code review rather
  than claiming all final failures were unscanned.
- C2: Actual-platform branching adds no Linux skip; anticipated existing skips
  8 Linux / 32 macOS must be checked against measured results.
- C3: Real special-bit chmod rejection mandatory on Linux; off-Linux conditional
  only on observed bits, plus unconditional real-stat_result predicate tests and
  access/hash tripwires.
- C4: Delegating sys stub, explicit context booleans, both signal handlers and
  exact category/environment assertions.
- C5: Separate prerequisite-order tests; fixed three-name tool lookup.
- C6: Same include order, real SDK on Darwin without synthetic headers, bounded
  compiler failures remain failures, actual native compilation still required.

The /var ancestor alias is hygiene, not an established outcome-changing cause:
_private checks the final component. Actual exception reasons, Darwin bit
persistence and SDK/native acceptance were not established. Completed code
requires independent review and validation before publication.
