# Managed runtime spike completion: design review

Reviewer: independent Claude, model **`claude-sonnet-5-5`**, as recorded in the
review wrapper's `modelUsage`. Scope: completion amendment, original spike
design, disposable harness, pinned MDK patches, workflows and failed native
report. Reviewer performed read-only inspection, with no edits or execution.
Source baseline discussed: `dd9abc7`; failed native run: `41136583828`.

Initial verdict: **NOT APPROVED**. The reviewer explicitly stated that folding
all seven gaps into the amendment would pass without another review round.
The revised [completion design](../phases/managed-runtime-spike-completion.md)
incorporates all seven corrections and essential test/run conditions below.
Disposition: **design approved under that explicit conditional disposition**;
implementation, code review and actual local/native acceptance are pending.
This does not approve any host policy change, skip or phase-0 gate waiver.

| Finding | Incorporated correction |
| --- | --- |
| 1. macOS help expectation contradicted the pinned restricted startup contract | Amendment explicitly replaces macOS native-help acceptance with exact guard refusal; original Positive/Interface clarification paragraphs link the correction; real configured startup is deferred. |
| 2. Report still unconditionally claimed native help | Platform-specific mode/assertion; success emitted only after exact results; both slots execute; fixed slot, numeric exit/signal, boolean exact-marker and output-size diagnostics only. |
| 3. Expected refusal lacked artifact binding | Require extracted Darwin manifest `patch_sha256` equal to the verified restricted patch digest and `restricted_protocol == 1`, preserving provenance/signature checks. |
| 4. Namespace diagnostics did not mirror failing tests | Exact-flags ladder includes bare unshare, bwrap and combined stages, ip/sysctl presence and isolated outer setup before child cap-drop; suite environment and three read-only integer policy values; no causal inference or waiver. |
| 5. Interruption could be unobserved or pass on an observer failure | Bounded single-walk observer with empirical staging discovery; live-child check; group SIGKILL/reap/no survivors; distinct early-finish/timeout/observer errors; explicit non-exposure, case-owned environment and fresh-slot retry. |
| 6. Substitution and cwd seam behavior was unspecified | Once-only in-process after-rehash hook; new inode/gzip-MTIME replacement, bounded decompressed equality and exact digest evidence; validated explicit cwd and cwd/parent/private-config decoys. |
| 7. Ambient injection risked leaking state or corrupting the report | Restricted sentinel families, unchanged HOME/PATH, fd cwd restoration and environment finally, failure on restore error, absolute report path, narrow injection window, real slot-two install/control comparison and allowlist/sentinel assertions. |

Essential regression conditions are recorded under the amendment's final
section: private fixed report schema; missing tool/timeouts; platform-specific
negative probes; interruption early finish, observer errors and surviving
children; uncalled callback/invalid gzip/accepted substitution; cwd containment;
restoration on success and exception. Required runs include normal/symlinked
TMPDIR focused tests, real bounded Linux ladder and three executable cases, and
a native macOS exact-result report. Raw subprocess output must remain private;
failed-run evidence is preserved and new reports added beside it.

No design-review item remains unresolved in the documentation. Executable
validation remains unresolved until those runs actually pass. Phase 0 still
requires all three native targets and both Linux namespace tests without waiver.
The separate native confinement SIGABRT remains open; macOS gains no enforced
offline denial or proof of native help, identity or confinement. Any runner/policy
remediation arising from diagnostics requires a separate reviewed design.
