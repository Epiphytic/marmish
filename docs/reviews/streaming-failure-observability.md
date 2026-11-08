# Installed failure-observability reviews

The [design](../phases/streaming-failure-observability.md) follows failed installed copy run [42501629829](https://github.com/Epiphytic/marmish/actions/runs/42501629829) at baseline `46c9cb46e382735a8678b17eca5e416f30b0c264`. Its initiating cause remains unproven. No release qualification or historical cause is inferred from diagnostic approval.

## Independent design reviews

Reviewer: actual Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, read-only source/evidence inspection and local tests. Reviews occurred on 2026-11-08.

| Session | Verdict | Execution | Raw result SHA256 |
| --- | --- | --- | --- |
| `5ddad58b-df7e-4420-8e3e-9d99a309c6c3` | NOT APPROVED | 288021 ms / 30 turns | `083dd92a6ba3e2d8a67070f5cbcd8b1b6ed28253c81d9ad2cd25652b8ab195be` |
| `c6db5202-7120-4a33-9341-32b7fd456c56` | APPROVE | 264731 ms / 21 turns | `e7dda6146ecf9c76a02630d454da9624e32dae600eac97d8de7a0af1c7e66960` |

The first review found that normal supervisor uploads of failed terminal records contradict the simple supervisor-poll-error-initiated kill theory. It also identified nonexistent run-level native fixtures, uncovered setup/post-work error sites, unspecified diagnostic ownership and public-log security requirements. The amended plan makes native attribution primary, specifies a compiling-stub assertion RED and source-site table, preserves every error/cleanup path, and defers HTTP/retry changes.

The second review approved the amended design with no blocking findings. The reviewer mechanically checked every fallible run site against the table, ran 24 existing JavaScript tests and a scratch fake-download probe showing the baseline omits the diagnostic while retaining failure and owned-child termination. Native borrow feasibility was assessed from source; no Cargo build or live transport run was performed by the design reviewer. Binding clarifications are recorded in the phase document.

## Integrated code review

**APPROVE**, no blocking findings. Actual Claude Code session
`fb740e93-c88c-4b5f-9211-d9e3326ba0a2`, `claude-sonnet-5-5`,
`--permission-mode auto`, 142670 ms / 18 turns. Private raw result SHA256
`5cb3da5eb5f62d73385f11a79c7a9a9ff959c8797fa69138217864619a7d70af`.

The reviewer reconciled every fallible native site against the phase table,
checked select equivalence and unchanged fallback/cleanup/error precedence,
once-only work/setup/post-work ownership, finite peer attribution and public-log
security. The reviewer assessed all unsuppressed quality findings and found no
code defect or blocker. Actual reviewer checks repeated42 native streaming tests,
strict installed-feature all-target Clippy, formatting,172 Node tests,38 focused
Node tests, all four JS syntax checks and `git diff HEAD --check`; all passed.
Source hashes were checked again after review and remained unchanged.

The reviewer did not repeat the prior stub/security mutations or run real
daemons, relays, OS-process failure injection or hosted CI. Pure/seam tests and
source reconciliation do not establish the unknown historical copy cause.
Approval covers this diagnostic code increment only; a fresh complete candidate,
all eight original serial cohorts and final auditor remain required. No release
qualification, new version, tag or installer pin is approved by this review.

## Local implementation validation

The [local evidence](../validation/streaming-failure-observability-local.json)
pins eight implementation/test files and retained private test logs. Native
streaming tests passed42/42, strict installed-feature all-target Clippy and
format checks passed. The full JavaScript suite passed172/172; the focused
supervisor/transport suite passed38/38. Both agents first demonstrated missing
diagnostic assertion RED against compiling/working baselines, then GREEN.
Actual temporary payload-reflection mutations failed security assertions in
both languages; exact clean source hashes and fresh modification times were
restored before passing reruns. Native tests cover pure output contracts and
actual temporary-inbox rendezvous reads/validation/timeouts, not daemon/relay
run injection. JavaScript child fixtures use EventEmitter with the real
`ownedChild`; they are not OS-process or hosted validation.

Root review also corrected a stateful JavaScript `message` getter leak risk
(single validated read) and setup attribution before listener cleanup. Regression
tests cover both. A normalized source comparison found the old native work body
unchanged after removing diagnostic statements and the approved equivalent
select tuple; independent code review must still reconcile actual sites.

The integrated `ripwire . --quality-delta` returned2 with14 gating findings;
no acknowledgement or baseline change was used. The exhaustive native classifier
and existing I/O-kind classifier share match shape over different types; the
two JS ownership wrappers share short catch structure; two standalone temporary
test helpers share fixture setup. Consolidating these is a conscious review
trade-off rather than a claimed clean score. Four churn findings reflect the
required supervisor edits. The reported masking/verbosity locations in unchanged
`command_engine.rs`, `resolve.rs` and historical JSON evidence are outside the
exact diff and involve shared symbol/key names. New-symbol rows include the
preserved moved work bodies, explicit schema-test vocabulary, provenance data
and executed tests/types marked dead by the name-based map. The integrated
`--pr-context --max-tokens=6500` completed and points to affected supervisor and
streaming paths; its name-based broad reach is not execution coverage. These
findings remain visible for independent assessment; neither tool result is
reported as an all-clear gate.


The staged follow-up `--pr-context --max-tokens=6500` included all17 intended
files (the initial unstaged scan omitted new files); its log SHA256 is
`6fc70ca84aceff20c9fad5dde90ffcf040b144f30ee4ab79eaef0c0b083cfd19`.
The map does not recognize native path-module test reach reliably; the42 executed
native tests provide the local evidence. The staged-path audit found no runtime
state, private-key/token formats or unintended files. Existing ignore rules cover
Node dependencies, environment files, identities, databases, logs and temporary
files. Beads task updates remain unavailable because the installed CLI and
existing database schemas differ; no database migration or bypass was attempted.
