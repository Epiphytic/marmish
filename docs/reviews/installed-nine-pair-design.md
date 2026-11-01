# Installed nine-pair design review

Scope: high-level phased design and its specification/build-plan references.
No implementation or native success is claimed. Author: Codex. Independent
reviewer: Claude Code, `claude-opus-5-5`, `--permission-mode auto`, read-only
repository inspection. The initial review is preserved in
[structured evidence](installed-nine-pair-design-initial.json).

Initial verdict: NOT APPROVED. Dispositions:

| Finding | Revision |
| --- | --- |
| B1 Published archive lacks proposed admission/event interfaces | Baseline uses existing archive, CI supervisor and exact-count assertion. No new admission-control claim; future production flags require a matching published archive or reviewed candidate lane. |
| B2 Authenticated receipt interface unresolved | Use principal-exclusive fixed fixture executables plus existing authenticated MDK history/terminal responses. Correct the suggested deny-all defaults: additive allow sets intersect, so defaults allow exactly three fixtures and each key narrows to its own. Detailed observation schema remains an explicit phase-B design gate. |
| B3 Fixture source/provenance unspecified | Native test-only fixed-role fixtures built from harness SHA, separate owned paths/digests and provenance from the installed application. No MDK sandbox permission expansion. |
| B4 Scheduling would contend/deadlock | Separate canonical-master manual workflow, existing shared concurrency group, all-installed barrier, concrete preparation/pairing/server/cleanup budgets, suspend checks and explicit five-minute assignment notification. |
| B5 Single profile cannot run simultaneous transports | Serial three-command client sequence with bounded 300-second commands; always publish received/failed/timed-out terminal reports before server waits. |
| B6 Installer deletes downloaded archive | Resolve exact script blob, archive checksums and release manifests, run actual script unchanged, then compare/re-hash installed payload/ownership/launchers. Do not claim measuring the installer's deleted download. |

Useful nonblocking findings were incorporated: bounded artifact pagination,
unauthorized-principal coverage, separate per-pair receipt files, no live debug
stderr contamination, owned uninstall/version-root cleanup and rate-limit
diagnostics. The shared laptop label does not prove host placement or concurrent
capacity; the revised design requires observed runner assignment.

The [follow-up review](installed-nine-pair-design-final.json) APPROVED the
high-level phased design, resolving B1–B6 and explicitly acknowledging the B2
policy correction. It required one documentation correction (D1): with the
current default shared concurrency queue, newer runs can supersede pending
runs even with `cancel-in-progress: false`. That sentence was corrected and
checked against GitHub's official documentation. The reviewer explicitly said
this one-sentence correction did not require another design review. Useful
nonblocking wording/schema/readiness clarifications were incorporated as well.
Each implementation increment still requires detailed design approval and
integrated code/validation review; none is authorized as accepted native code
by this high-level review. Documentation whitespace and local-link checks pass;
no application tests are claimed for this documentation-only increment.
