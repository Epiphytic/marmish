# Explicit reduced Linux legacy reference scope

Claude Opus 5.5 independently reviewed the integrated change in auto permission
mode and returned **APPROVE**, with no blocking findings. Review session:
`9a87a434-9c45-4bed-bc65-6fb4d0958310`. The private review JSON SHA256 is
`683f789bbb1f6a04087e9132a3083af166f7f0d86650e8c940516b4baccf2ed3`.
The approved design and its prior review are recorded in the
[phase amendment](../phases/legacy-namespace-coverage.md#amendment-explicit-hosted-linux-reference-scope).

The reviewed baseline was `28db4a1b96ba3e7e6b7137bd582a1a8ff34a758e`.
Reviewed SHA256 values, unchanged throughout the code review:

| File | SHA256 |
| --- | --- |
| `tools/marmish_managed_runtime_spike.py` | `ac973191c95a270a250b702732d4669e0910b076052c950d41ae417fc87b8b3b` |
| `tools/test_marmish_managed_runtime_spike.py` | `24ba9e9cc6dc80662ecfdd45293791959f770f43ba20ff75e6dd5f9ee6deef72` |
| `.github/workflows/installer-validation.yml` | `51fd906c35eac55d876fc24af5766555bbde30aa125c32efb717d32d144bda51` |
| `docs/phases/legacy-namespace-coverage.md` | `7ccdbeacad003a4ef3ec3d54d0f90c6e96c8824eda514ae231805667e1eeb131` |

The reviewer independently ran the complete spike test module on these final
bytes: **126 tests passed** with the namespace opt-out absent, and **126 passed**
with its value set to1. The four new scope tests plus the existing workflow
contract also passed by name in both modes, five tests each. `git diff --check`
passed. These checks used local mocks/contracts; no real compiler, cache,
download, namespace, network or hosted workflow was exercised by this review.

The review confirmed early invalid-scope refusal, unchanged default offline
acceptance, reduced completion only after full-tools success, preservation of
negative-stage failures, and final report derivation after cleanup/home-integrity
checks. Only the Linux automatic step selects the new scope. macOS, manual
namespace tests, native Rust and release gates retain their existing behavior.

`ripwire --quality-delta` exited2 for four short-horizon churn findings on the
necessarily edited entry points/tests. The reviewer found no defect requiring a
change: unittest discovery explains its dead-code reports, explicit test matrices
explain added test complexity, and the sixth proof argument implements the
reviewed interface. Nonblocking observations concern narrower stage-order
assertions and broad `Path.exists` mocking in the cleanup-failure test; final
integrity behavior has direct coverage. No score-only refactoring was made.

No fresh hosted result is claimed. Historical failures remain failures. The new
scope explicitly does not qualify full Linux legacy packaging or independent
offline retention; capable infrastructure remains necessary for that acceptance.
The capable manual two-test lane has not executed, and would not alone establish
full packaging acceptance. Visible GitHub check names change; external branch
protection configuration has not been verified. Confidential trace diagnostics
remain intact.
