# Legacy namespace selection: integrated review

Beads `marmish-d93.3`; implementation base
`6a4072c7a1e482ac119a16fb9fbcca22ee0c3f85`.
See the [approved phase](../phases/legacy-namespace-coverage.md).

Claude Opus 5.5 reviewed the seven integrated source, workflow and test files
using Claude Code auto mode, session `a910283c-5084-42de-8e11-404fd6bf52da`.
The initial review required removing broad Linux platform mocks which could
change macOS test expectations. The correction preserves actual host platform
behavior, adds a regression, and keeps environment isolation local. It also
restores the Darwin skip-reason check and sanitizes early manual-wrapper errors.
The followup verdict is **APPROVE**, with no blocking findings.
Review record SHA256:
`f6d4daf3c015e313800ea520f41002c22aa9a9adc84d5aabc89aa3f13b504372`.
Frozen source manifest SHA256:
`34969ca73e809401d3f0f4b08cbb0f1a2e5ee0e3ab0cecfe09ef52721da18106`.

The author and reviewer each ran the fixed safe contract inventory with the
parent selection flag absent and set to literal1: each run executed62 tests,
with zero skips, failures or errors. This inventory does not execute real
namespace-positive methods. AST checks confirm that both positive integration
bodies and the protected negative body are unchanged. Diff checks and Python
compilation pass. The nine quality-delta gating findings were disclosed and
accepted by the reviewer: small environment-isolation duplication, prescribed
AST/workflow test size and short-horizon churn. No clean quality-gate claim is
made.

Actual reduced hosted-suite and capable manual-lane validation remain pending.
The manual lane requires the real diagnostic probe plus exactly two successful
integration tests with zero skips. Reduced legacy success cannot qualify full
namespace coverage. Neither change adds a namespace requirement to native Rust
Marmish or changes release, installer or runtime behavior. Historical failures
remain failures; this selection does not establish their causes.
