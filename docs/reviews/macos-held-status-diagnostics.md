# macOS held-status diagnostic review

Beads marmish-d97.2; base33138ebdb2ddf19103bcb58f64c2b31edc66b2c4.
Claude Code model claude-opus-5-5, permission mode auto, actual session
ff71191a-e796-4335-ba47-e626ccae1e0d independently reviewed the
[detailed phase](../phases/macos-held-status-diagnostics.md) before code,
then the integrated two engine files, new diagnostic workflow/policy, phase
and agent-spec documentation. No runtime cleanup correction is included.

## Design

Initial CHANGES_REQUIRED: extract the real production validation/failure seam
rather than test a reconstructed Failure; pin module/lib filters and separate
formatter name to avoid false test counts. Resolved both and adopted closed
zero-first categories, stage-checked detail attachment, prefix-only libtest
name checks, metadata-only worker identification, separate targets and honest
fresh-history limits. Actual followup APPROVE reviewed design SHA256
fece46f1133a453cb677fc93eb79f618f867d3e7a9660ff50984b88c2dce0afc.
Approval JSON SHA256
093ab92ba59d0387befc81842c3bcaf20ae7cd7bb27ebd2600290949e2102f56.

## Completed code

Initial CHANGES_REQUIRED identified empty flags-array expansion under nounset
on older Bash. Both commands now use the guarded array idiom, and the ninth
inert policy test executes the actual engine block in debug/release with
explicit argument/result checks. Actual followup APPROVE JSON SHA256
da0f60635f76859dc4c11861cc785978b296345ab86a6e9dec9443e3f40de6cb.
The reviewer reran all nine policy tests successfully. Native source and the
existing macOS regression workflow/policy are unchanged. The two existing
waitid acceptance predicates, flags, errno handling, si_status read ordering,
strict errors, signals, ownership and cleanup deadlines remain unchanged.
Metadata stdout is kept separate from progress stderr to preserve valid JSON.

## Validation and limits

- Portable Linux engine library:145passed,0failed,1ignored,21.71seconds;
  private log SHA256
  3ae218079239b040dcdcc6a2ee88f7f0a4d4cb2f470bb1a98e18ca1e3c707475.
- Strict engine all-targets debug/release Clippy passed; logs SHA256
  4afcc8e178e60a178131771d6fb5dcc9d26558aa5fedfb69a508dd9dd0e73b6e
  and90f6107674a503658fa2b77deaf500c484f3352a812f6ef6b8be847b3ed3323f.
- Nine Node policy tests passed, including shell syntax and inert cases for
  full-command failure, wrong summary, first-repeat failure, successful
  one-plus100 sequencing and five engine invocations. These run stubs, not
  Cargo/native programs or GitHub workflows; local Bash5.2 is not Bash3.2 proof.
- Workspace format and whitespace checks passed. Quality-delta reports16
  findings: two major historical-churn findings, seven new cfg/test/enum
  reachability findings and seven minor findings, including Record formatter
  verbosity57to63. Real call sites/tests and strict portable Clippy exist;
  the bounded debug serializer remains cohesive. No metric suppression or
  unrelated refactoring was introduced. The independent review found no
  remaining blocking code findings.

Linux does not compile Darwin cfg. The two Darwin pure tests in both profiles,
formatter test, strict Darwin engine Clippy and full debug installed-feature
suite remain first-hosted-run checks. Expected907passed/3ignored is a source-
bound admission check, not an observed result. The new manual diagnostic
workflow runs that full command once, then100exact race repetitions only if
it passes, stopping on first failure within45minutes. Passing supplies
observations, not release qualification or proof the race is fixed. The
historical cause and actual rejected wait-status category remain unknown.
Root retains publication and one-shot dispatch ownership.


## B2 actual metadata inventory correction

Diagnostic42665195683/1 at40ab515 failed the metadata guard before all engine/native checks; no cleanup category was captured. Actual cargo auto-discovers the ordinary marmish-release-tool binary, omitted by the original exact five-bin list. Add that empty-feature binary to the sorted exact native list and matching policy assertion only. Engine/spec bytes and all diagnostic test commands/guards/budgets remain unchanged.

Actual Claude auto design and separate-gate clarification APPROVE in the same independent session. The nine Node-only policy tests remain Node-only and pass. A separate mandatory gate ran `RUSTUP_AUTO_INSTALL=0 cargo +1.97.1 metadata --offline --locked --no-deps --format-version 1`, bounded30seconds, then extracted exactlytwo production jq predicates by package selector and evaluated them against actual output with5second bounds. Both real inventories passed; removing one bin from each corresponding package was rejected (jq exit1/false). No build/test or product process was invoked by this gate.

Evidence SHA256:

- held-status-metadata-design-review.json: `0c3d9c3cb41da9f7167ea70febd77748432b20b012fc68d2f8a27bc2a6d2c739`.
- held-status-metadata-design-followup.json: `362a1c1cb798d2e1a48ef4a98040010e2a29572a0ab59a3bc4862c493ccd22ef`.
- held-status-metadata-real-gate.py: `918899c7f37e6050a4431e2c0d1416fe0595a1ede1d67a4d7df08383a4d2163e`.
- held-status-metadata-real-gate-result.json: `8fc695507a0c4958e7968fa4579bb168d03dcee6504367e7fbde764bb9028662`.

Actual completed-code review remains required before local commit. Parent alone publishes and dispatches; no original race resolution or release qualification is claimed.

Actual Claude auto completed-code B2 followup APPROVE, sessionff71191a-e796-4335-ba47-e626ccae1e0d, review JSON SHA256bad9208b7e78d46268bc2c45d4c2f274b95ea6779d3d2611b1a13cdcadd0566b. Reviewer independently reran9Node policies PASS and actual offline metadata predicates PASS; removing exactly the newly inventoried release-tool target was rejected. Ripwire quality-delta against40ab515 reports0regressions/0gating; diff check PASS. No Darwin compile/runtime observation yet. Parent owns publication and one diagnostic dispatch.
