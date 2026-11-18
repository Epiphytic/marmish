# Peer native Leave C2 code review

Claude Code Sonnet 5.5 used `--permission-mode auto` for both reviews.
The implementation follows the
[approved C2 design](peer-native-leave-c2-design.md).

- R1 approved exact implementation `65c55ea93dd8e24fd6c73f28c2ddf0a376d738f7`
  in session `1df935d5-04c3-4115-8490-8e60dbf0d484` with no blockers.
  The reviewer inspected test-only mounting, the exact epoch-6 SQL
  apertures, native and quota scopes, and the cold disposition table.
  The reviewer ran a non-test library check, a standalone cursor compile
  and the fast C2 tests. The author ran the opt-in native process tests
  and strict native Clippy.
- R2 approved exact source `b794a47ffb8dbaa7763322aa97617f3c29af5e6e`
  in session `9a00cf24-c9f9-419c-8af4-422a7a1d51eb` with no blockers.
  Only two test files changed. The follow-up checks direct
  SQL deadline and duplicate-write refusals, explicit rollback, and
  original-stage preservation when the snapshot name is occupied or quota
  cannot accommodate a copy. The reviewer inspected the assertion and
  implementation source, plus author test logs. The reviewer ran no new
  tests in R2. A preliminary filter that selected zero tests is excluded
  from evidence. The author ran the real quota selector and recorded the
  result in validation.

R1 identified the exact limits of pure SQL fixtures and cold Captured
inventory inspection. The opt-in process suite covers the signed custody
and native checkpoints. The remaining negative coverage and platform
limits are recorded alongside the exact dependency and Linux results in
[validation](../validation/peer-native-leave-c2.md). The test clock and
crash hooks must remain confined to proof fixtures.

## Integrated review

Claude Code Sonnet 5.5 approved composed source `1a973f0` in Auto session
`7061a0dd-b1ca-4c98-9e17-1432163c9ecb`. It checked the transaction and
wall fences, one-use dispatch, authenticated stage inspection, zero egress,
and test-only mounting. The source-only review identified additional
negative coverage for a future production owner, including wrong native
bindings, Rust-side deadline paths, and epoch-6 refusal of epoch-5 recovery
methods. It also noted that the original image digest binds the live token
and the first cut exercises image preservation. The author and integration
runs below supply the native evidence.

Ripwire flagged test helper token matches, similar schema guards and short
churn. Review retained independent predicates for exact schema and owner
admission. A shared constructor would couple the epoch checks, and a shared
native-turn helper would hide the difference between inspection and dispatch.
