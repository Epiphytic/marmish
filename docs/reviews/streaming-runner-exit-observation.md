# Runner exit-observation lifecycle reviews

Scope: fresh successful-signal and exit-observation timestamps, one owned exit refresh at the existing post-KILL failure boundary, deterministic real-child tests. No deadline constants, signal count, ownership, wire contract or cancellation-vs-natural meaning changes. Historical macOS cleanup cause remains unknown. Diagnostic-only preceding increment651cb98 has its own approved review.

## Actual design review: combined-design-review.json

Claude Code, claude-sonnet-5-5, --permission-mode auto. Session bb9b6951-241e-4e99-976d-144f50d495e8; 144888ms, 12 turns. Raw private JSON SHA256 5a78ede564b905c66558d5462d21351b0b0327a58262acf25dbc397c0b452f40.

Initial lifecycle proposal NOT APPROVED: clarify reachable boundary forces cancellation; remove unreachable natural-state counterpart; distinguish source-inspected fresh timestamp from RED-proven refresh; reuse existing synchronous ChildOwner cleanup rather than invent guard; specify real stdin-gated child, checked timestamp subtraction, old-code negative controls and exact assertion order. All five were amended before implementation.

## Actual design review: lifecycle-design-rereview.json

Claude Code, claude-sonnet-5-5, --permission-mode auto. Session ebdaef60-2eb4-4810-9c0d-f2e6da3852a9; 39903ms, 7 turns. Raw private JSON SHA256 0b97c70c5c8c13ec75f5840838f7192c305885a050a39ef10d402d9bfc23de6b.

Revised lifecycle design APPROVE. Reviewer independently derived reachable state and traced fresh observation through existing output abandonment and single final reap. Scope and no-timeout-inflation constraints upheld; actual historical cause remains unknown. Timestamp assignments have inspection support only; deterministic old/new regression covers cached negative observation. Existing reachable natural-exit regression remains required.

## Integrated code and validation

Actual integrated Claude review APPROVE; deterministic RED/GREEN, real-child negatives, restored runner suites and strict Clippy passed as recorded below. No all-platform qualification claim.


Local Linux x86_64 validation after the actual final-group signal was added to the real exited-child premise: OLD default focused 2PASS/1expectedFAIL0.63s; OLD installed-feature focused 2PASS/1expectedFAIL0.81s. Both negatives pass old code; already-reaped finite error attribution is asserted only with the new probe. The exact same positive status/ownership assertions fail old cached-observation code and pass corrected code. Temporary old-code patches were reversed with a saved final-source byte comparison and fresh mtime before GREEN. This is a stale-state seam, not a reproduction of the historical Mac failure; timestamp assignments have source-inspection evidence only.

Final focused GREEN default3PASS0.58s and installed-feature3PASS0.72s. Full runner module default20PASS/1existingignored6.80s and installed-feature20PASS/1sameignored7.50s; overlapping counts are not summed. Strict all-target installed-feature Clippy PASS8.43s; scoped formatting/diff checks PASS. The ignored explicit large-duplex qualification gate was not run.

Final source SHA256 d73b1b1773a541d39d41c161ef6371dc4bdc08d93358bf11661d25d96a9c67e5. Private exact command/log manifest SHA256 e2177b0174f689a416acd146d720d21d57b2b2d3c85eaf20db0d4a1d1b83e823.

Quality-delta remains unsuppressed exit2: 17 final rows,10gating, including two changed lifecycle churn rows, finish verbosity102→116/minorcomplexity24→26, three #[test] graph dead-code false positives, a standard bounded real-child poll resembling private helpers in other modules, and repeated test fixture/finish assertions. No baseline or acknowledgement suppressions. Existing helpers are private to different test modules or typed to RunnerHandle, whereas these seam tests must retain real LiveChild ownership; broadening production/test interfaces solely for a shared poll is outside this correction. Independent integrated review must assess these tradeoffs rather than call the gate clean. Scoped change-check exits0; its structural results are not runtime proof.

Final unsuppressed quality inventory: `<quality-delta baseline="git-HEAD" regressions="17" minor="2" acked="0" preexisting-worse="12" new-symbol="5" gating="10" at="651cb9893+dirty">`


Actual integrated code review: claude-sonnet-5-5, --permission-mode auto, session 88378359-21f3-4260-8e07-eea41fffcea3, 86688ms, 15 turns. Raw private JSON SHA256 e6cdf3fc66033fc9fdeca960408e590241b2f152002462a182be498cc4cc9f78. Verdict APPROVE for corrective source publication and fresh qualification only. Reviewer independently verified exact final source/manifest/raw Mac log hashes, compared approved design and preceding diagnostic change, reran default and installed-feature runner suites (20PASS/1existingignored each), strict installed-feature all-target Clippy, full cargo formatting and diff checks. Old-code RED logs were inspected, not independently recreated by the reviewer.

No blocking findings. Quality exit2 remains disclosed: repeated small bounded owned-child poll resembles private or differently typed helpers, new #[test] graph dead-code blindspots, changed lifecycle churn, and finite finish complexity/verbosity increase; no ack or baseline suppression. Expanding interfaces or extracting broad helpers solely to reduce the score was rejected. Final report corrected the pre-format inventory wording to17rows/10gating. Actual exited-leader group KILL exercises Darwin owned-zombie proof, which still needs hosted macOS validation. Timestamp correctness remains source-inspected, not RED-proven. No substantive source change follows approval.
