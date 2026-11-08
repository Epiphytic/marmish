# Runner cleanup diagnostic review

Scope: test-only authority-withdrawal fixture lifetime and existing finite failure-context reporting. No product logic, budgets, expected terminal or original assertions change. Historical f96 macOS cleanup root cause is unknown. Product lifecycle amendment was subsequently implemented as separate commit 8d0dbf23 after its own design and code approval; see [its integrated review](streaming-runner-exit-observation.md).

## design-review.json

Actual Claude Code, model claude-sonnet-5-5, --permission-mode auto. Session e7d74222-545a-4fa5-8b2f-9d8b6dc904a2; 103840ms, 9 turns. Raw private JSON SHA256 843fa2d962609cd3fabaceae9bb8c2c4d5992c8d366ee0e2e9a9b8b92b3b7452.

Initial NOT APPROVED: pre-cancel injection could be overwritten by worker and miss diagnostic assertion. Corrected injection after worker return; added checksum/fresh-mtime restoration and isolated-tree requirement.

## combined-design-review.json

Actual Claude Code, model claude-sonnet-5-5, --permission-mode auto. Session bb9b6951-241e-4e99-976d-144f50d495e8; 144888ms, 12 turns. Raw private JSON SHA256 5a78ede564b905c66558d5462d21351b0b0327a58262acf25dbc397c0b452f40.

Diagnostic design APPROVE with validation additions: baseline old-shape injected failure must lack finite site fields, changed-shape failure must include them; diagnostic change is its own commit before separately reviewed product change. Those additions were recorded before implementation. Lifecycle proposal separately NOT APPROVED pending spec corrections; its review belongs to subsequent increment.

## Integrated result

Actual integrated Claude review APPROVE; no qualification claim.

Local restored-source validation: exact authority-withdrawal test PASS (1 test); full stream_runner module PASS (17 passed, 0 failed, 1 ignored, 861 filtered, 2.06s). The ignored explicit large-duplex qualification gate was not run. Strict locked/offline installed-feature all-target Clippy PASS (22.76s); scoped formatting/diff checks PASS. The baseline and changed injected tests each deliberately exit101 with one failed test: baseline lacks site=, changed includes existing finite site/engine/lifecycle/window context. This validates diagnostic wiring only, not the hosted failure branch. Both post-worker injections were reversed with exact SHA256 restoration and fresh mtime before the clean tests.

Private sanitized-evidence provenance (raw logs are not committed):

- baseline-injected.log: SHA256 3fef6b3d48b856e22450f3dc9b796c0dfde0279a675932c2fca0cdeeff8e5628.
- changed-injected.log: SHA256 d509cbb4915272307605698530138be06b5dab8a2f2f31f831be31763650b6ca.
- restored-exact.log: SHA256 735987a88907f5c27ea14a87f9b0f40623fd0d63ab2bab62f5bff7c1ce9cd5e9.
- restored-runner.log: SHA256 6a392a61de0ec0824e85bdbf579bc2e43740d844a765d02469562fffec5f236d.
- restored-clippy.log: SHA256 1b958f7be7b466bbec546744cba7eb6f37a7e16f8f6e23919e19aab46777189d.
- restoration.json: SHA256 d9c281bd77cd7e3c5f5807f11a8a306bd25c5387fee51066a55d9c4836719849.

Quality-delta exits2 with two short-horizon-churn findings on the changed test/enclosing tests module. No structural/API/error-masking regression is reported. No baseline or acknowledgement was used to suppress them; actual independent code review must assess this small fixture change. Scoped change-check exits0 and is structural context, not proof of behavior.


Actual integrated review: claude-sonnet-5-5, --permission-mode auto, session f6861865-abc3-4a38-8c48-94a8618f6e88, 74740ms, 10 turns. Raw private JSON SHA256 ca8d2943c87bc920c96c83194f992609f9880e26ccc2efee9638296ee4c7052b. Reviewer independently reran exact test (1PASS), runner module (17PASS,1ignored), strict all-target installed-feature Clippy (cache fingerprint hit0.35s), formatting and diff checks. Approved exact4-line test-only change, unchanged product behavior and all original checks. The two unsuppressed churn findings remain disclosed, not acknowledged or suppressed.

Nonblocking wording fixes incorporated: explicitly record the additional existing8s cleanup-finish wait and intended timeout failure; describe reached input assertions as a control-flow inference; remove the uncited publication claim from the spec. Linux job states in the failed-candidate snapshot remain pending at this review time. Raw hosted log was outside the reviewer's supplied scope; independently hashed by two agents, not represented as independently reread by Claude. No new product/source change follows this review; separately approved lifecycle work must be a later commit.
