# Temporary legacy diagnostic retirement

Beads `marmish-d93.7`; source parent G:
`2b951aee49b10825cf66997aa2892ee4352a8d79`. The
[phase amendment](../phases/legacy-confidential-trace.md#retirement-amendment)
records the active G/R binding and pre-code independent approval. Claude's V2
design verdict was **APPROVE**, session
`6f72a512-c27d-4123-b906-b4bce4a42c35`; review SHA256
`89e343635b9769119a2f44c01d0e557fc05c0723e4a259b2ccde6597ccc378fe`.

The pre-deletion gate was ordinary legacy workflow
[42649925720, attempt1](https://github.com/Epiphytic/marmish/actions/runs/42649925720),
at exact G. All six jobs passed. The source/run association, artifact digests,
closed JSON inventory and report predicates were independently verified before
retirement. Sanitized accepted validation record SHA256:
`1141ec06fc8905b21838e47197db38fc3a4914845c8fa23e21c6637a9056d5b2`.

| Platform | Full-tools total (includes skips) | Skipped | Exit | Requested scope | Full packaging complete |
| --- | ---: | ---: | ---: | --- | --- |
| Linux AMD64 | 1024 | 11 | 0 | Reduced Linux reference | false |
| Linux ARM64 | 1024 | 11 | 0 | Reduced Linux reference | false |
| macOS ARM64 | 1016 | 45 | 0 | Default full legacy packaging | true |

Both Linux reports explicitly leave independent offline retention unselected
and unmet. macOS retains `uv_offline_only_not_independently_enforced`; this is
not independent OS network-denial proof. Reported cleanup completed. Earlier
failures remain failures; successful corrections do not establish the kernel
cause of historical EPERM or explain every earlier failure. These are legacy
reference results, not native release qualification.

The retirement removes exactly the temporary workflow, public recipient
certificate and nine associated mock-only tests. A delegated read-only tracked
consumer audit found seven literal/basename/concurrency-group occurrences, all
inside the deleted workflow/test module; no surviving consumer blocks removal.
Static AST inventory confirmed nine test methods with no skips. The coordinator
also audited external operational scripts and confirmed no pending dispatch.
Private evidence/key material and historical Actions artifacts are preserved.
Registered Actions history remains; the removed workflow is unavailable for
dispatch on the new source. Existing private failure-log support is unchanged.

Local managed CPython3.13.13 validation ran eight explicit existing modules/classes:
managed spike, installed rendezvous, installed-state DarwinZombieCleanup, Linux
namespace workflow, macOS denial workflow, managed workflow, provider diagnostic
workflow and release automation workflow contracts. **184 tests passed** with the
namespace opt-out absent, and **184 passed** with its value set to1. Checks were
serial and included the existing own-loopback Watchdog test. No full discovery,
public relay, namespace-positive execution, real cache/compiler operation or
native build was run for retirement. `git diff --check` passed;
`ripwire --quality-delta` reported zero regressions. Blob comparison verified all
1570 other tracked files unchanged, excluding the three deletions and bounded
phase/spec documentation; the review document itself is new.

Claude Opus 5.5 independently approved the initial integrated deletion and docs
in auto mode, session `6f72a512-c27d-4123-b906-b4bce4a42c35`; response SHA256
`892377986d61017016ecba9c3fa20f5550956bd168f890eb1391dedde8e7e7a4`.
The reviewer independently verified all1570 unchanged blobs, the nine deleted
tests, absence of surviving consumers and exact accepted G evidence. Five
remaining workflow contract modules passed in both flag modes,29 tests each;
the reviewer also inspected the local184-test logs. A first reviewer invocation
used bare module names under `-I` and failed imports; corrected explicit discovery
passed. No source change was needed for that invocation error.

Claude Opus 5.5 returned combined design/code **APPROVE** for the sequencing
amendment in session `6f72a512-c27d-4123-b906-b4bce4a42c35`; response SHA256
`45d0766bce2ec60207cf214a3efc800a1e84dfe6bdb55088a2120d728eabc661`.
R has no hosted
result yet. After review and coordinator clearance, R is published and its
own all-three-platform legacy workflow must pass, with expected totals1015/11
skips on Linux and1007/45 skips on macOS. Any count/predicate/source discrepancy
blocks acceptance; G's results cannot qualify R. R is retirement only, not the
final release freeze. A separate native test at G failed with `AlreadyOpen` in
`store::tests::denial_precheck_does_not_exhaust_execute_tokens`; the competing
holder remains unknown. Separately reviewed test-only correction N, tracked in
`marmish-d97.1`, follows R. Final source A is N or a later corrected source;
all release gates require fresh exact-A evidence. No G or R result qualifies N/A.
No native gate is waived; no release/tag or publisher enablement is included.
