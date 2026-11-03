# Installed workflow integration: local validation

The manual `.github/workflows/v02-installed.yml` workflow integrates the reviewed
artifact supervisor with the feature-gated Rust installed driver. It is a matrix
gate, not a release acceptance declaration. No hosted installed matrix has passed
for this increment yet.

Local checks on 2026-11-03:

- After correcting the first integrated review findings, the locked Node coordinator suite passed all 31 tests, including seven new
  provenance and pre-execution verification tests. These reject a different
  control source, wrong candidate run/attempt/workflow/repository, unsupported
  relay, open tooling schema, wrong target/source/harness, invalid size,
  mismatched original record and changed executable bytes. Failed verification
  leaves the mismatched executable without execution permission.
- Node syntax checking passed.
- Official actionlint 1.7.12 passed the actual workflow and local Node24 action.
  Its downloaded archive matched the upstream release checksum. Earlier
  actionlint 1.7.7 cannot validate Node24 action metadata and is not the acceptance
  result for this workflow.
- `git diff --check` passed. Node dependencies, `.env`, private `.nsec` files and
  logs are ignored by the repository.
- The broad Ripwire quality delta exited 2 over the combined in-progress working
  tree, with 51 gating findings. This includes Rust state-machine complexity,
  test functions reported as dead code, workflow length and short-horizon churn.
  It is not recorded as a passing gate. Component code reviews and focused
  runtime checks remain required; no suppression or baseline reset was applied.

Build-only JavaScript is not installed with Marmish. Product subprocesses receive
an explicit utility-only environment; installed runtime qualification separately
checks the absence of Python, uv, Cargo, rustc and Node. Real keychain selection,
command-count/uptime exit, benchmark, policy, recovery, immutable publication and
both public HTTPS installer sources remain separate release gates.

The first integrated Claude code review found mismatched Rust/JavaScript public
record filenames and a rollup directory membership conflict. The coordinator now
reads the actual role-prefixed Rust records while each uploaded archive retains
its fixed unprefixed basename. Diagnostics use the same role-prefixed sources.
Additional tests cover exact twelve-record gathering, missing artifact refusal,
mandatory context tooling pins, candidate-attempt binding and equality of the
Git-tree digest with the candidate pipeline. The subsequent reviews checked the integrated fixes; actual round4 approval is linked below.

Actual integrated round 2 independently repeated all 31 Node tests, 13 focused
Rust tests, strict feature-enabled Clippy and modern actionlint successfully.
It confirmed the first round fixes and the sealed cleanup authority, but requires
reserving cleanup inside the daemon uptime and using installed setup for grant
and trust updates. The fixes were implemented, including a further sorted-key regression caught in round3. Actual [round4 code review](../../reviews/v0.2-installed-workflow-code-r4.md) APPROVED the integrated nine-file Rust driver and Actions workflow, independently repeating16 focused Rust tests,31 Node tests, strictfeatureClippy and modern actionlint. This is local code approval only; no hosted qualification is claimed.

After initial publication, independent benchmark design review found the matrix client inherited the server command-count limit. The corrected driver assigns count4 only to the server and validates both real setup-generated configurations through the daemon activation overrides. Actual [round5 review](../../reviews/v0.2-installed-workflow-code-r5.md) APPROVED this correction and the integrated scope;16 focused Rust tests, strictfeatureClippy and the31 existing Node tests passed. Hosted qualification remains pending.
