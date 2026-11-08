# Failed candidate: f96b692 — partial evidence

Source `f96b692194c4cc6c54dcbb142d03ace781803d42`; candidate [42446123377/1](https://github.com/Epiphytic/marmish/actions/runs/42446123377). **macOS gate failed; Linux job final evidence and final run status remain PENDING. Not qualified.**

[Machine-readable evidence](streaming-candidate-f96b692-failure.json).

macOS ARM64 job [128625304714](https://github.com/Epiphytic/marmish/actions/runs/42446123377/job/128625304714) ran 2026-11-08T04:16:20Z–05:02:40Z. Default native library passed 629 tests, 3 ignored, in 617.64s; the installed-feature native library had **874 passed, 1 failed, 3 ignored in 667.22s**. Default engine 148 passed/1 ignored (34.38s), stream 29/0 (0.34s), vault 13/0 (0.03s). Counts overlap and must not be added.

The failed step was “Build isolated installed-test tools without changing product assets.” The prior default component verification, final executable build/signing, dependency/ABI inspection, locked license collection and candidate packaging steps succeeded. Later tool preservation/uploads were skipped. This is not completed candidate publication or installer qualification.

`stream_runner::tests::authority_withdrawal_halts_queued_input_irreversibly_without_cancel` passed its earlier default invocation, then failed at `stream_runner.rs:1074`: expected `Some(Cancelled)`, observed `Some(Failed { code: Uncertain })`. Reaching that assertion establishes that the preceding marker/input-suspension assertions returned successfully. The hosted log prints no stored lifecycle/error context, so the actual failure branch and underlying cause remain **unknown**. It does not establish physical worker cleanup completion.

Raw job log: SHA256 `30e3c764c78eef5bb62fa2dd25f80ca3ded575ae9fc74be2b1bd35af378c97f8`, 462931 bytes. Retained bytes were hashed independently for this draft.

A proposed test-only correction reuses existing CleanupGuard ownership and diagnostics before the unchanged terminal assertions. A separate source audit demonstrates stale signal timestamps and a cached negative exit observation at the expired post-KILL boundary; a narrow fresh-timestamp/fresh-owned-probe amendment is proposed. Neither proposal proves the historical cause. Actual implementation, RED/GREEN proof and independent integrated review remain separate pending evidence.

Zero installed cohorts were dispatched; final auditor did not run. Linux AMD64 job 128625304446 and ARM64 job 128625304518 remain **PENDING final evidence** in this draft. A later Linux success cannot repair the failed macOS gate or qualify this source.

The [historical 2f partial chain](streaming-installed-2f98802.md) is separate. Legacy CI’s 19 failures remain unresolved as hosted evidence: 10 locally obsolete assumptions, 9 locally passing with hosted causes unresolved; none waived. No automatic f96 legacy run is retained because its changed paths do not trigger that workflow. Public installer remains **v0.2.3**; no release/tag/public installer pin change is authorized or asserted. Independent evidence review: **PENDING**.
