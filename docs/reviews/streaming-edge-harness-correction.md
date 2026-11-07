# Installed edge harness correction reviews

Scope: the bounded [edge harness design](../phases/streaming-edge-harness-correction.md),
prepared against `50ad3d1`. No product/runtime, wire, workflow, canonical evidence,
workload, deadline, version, public installer pin or release changes.

## Actual independent design review

Claude Code `--permission-mode auto --model claude-sonnet-5-5`, actual session
`6be8f8f5-e90e-462b-b19c-3ea3b6e725b0`, reviewed repository code independently.
The first invocation hit its 18-turn cap without a verdict (108.773s, 19 turns);
this is not approval. Its private JSON SHA256 was
`c31318bf88c4333082e571a45c38094690bce63e2a4084cac779ab8e31d8de79`.

The resumed review returned NOT YET APPROVE (40.175s, 2 turns), SHA256
`8872fa6bca924983cefc438e5bcfa81ba5713faf7e89f8f556cc32dc5faa19dc`.
It required an explicit one-shot established-running observation, including
missing-marker rejection, and named pure marker/charged proof seams for tests.
Useful nonblocking findings clarified actual Open validation, client-role
labeling, both reconciliation callers, daemon-lock independence and once-only
trace completion. The design author incorporated these before coding.

A second resumed review returned **APPROVE for implementation** (24.686s, 2 turns),
private JSON SHA256
`866a261223edc49abb4eac2f7909cd1c8b5a743a7991ecd7beac7495e714af9f`.
The exact approved design SHA256 was
`fecc4ac17af9e5f7c21171993652d86b5da8c0667c1bad73e7832d72cf47b336`.
Approval was recorded here before delegated implementation began.

Implementation conditions: retain strict marker JSON fields/types without an
extra read, test the pure startup/established marker decision and actual dwell
wiring, and preserve quota's later inline held-command checks and IPC count.
The existing marker hash read remains unchanged. These conditions are part of
the accepted design and will be checked in integrated code review.

The reviewer verified the missing gate directory, legitimate inactive Hello,
restart-disposition behavior and original cleanup error precedence. It performed
no edits, tests, builds, live transport, DB or Actions mutations. It did not
independently establish historical failure causality, the asserted validated
workspace prerequisite, hosted cleanup flags or local admission results.

Beads task `marmish-8ka.5.8` remains open and unsynchronized: installed clients do
not safely match database schema 53. No migration, reinitialization, bypass or
manual DB edit was attempted. This record is review evidence, not a substitute
task tracker or a claim of completed qualification. Integrated code review,
fresh all-platform candidate and all eight original serial cohorts remain
required. No tag, version or public release authorization is implied.


## Integrated code review and local checks

The same independent Claude Code AUTO / `claude-sonnet-5-5` session returned
**APPROVE**, with no blocking findings, after reading the full staged diff and
all new files (90.289 seconds, 10 turns). Private review JSON SHA256:
`b146e76d3eb1c34320dc5f2f0bcc9e3f33527a82222b15d584ca056244de7812`.
The reviewer confirmed private gate preparation, strict marker validation,
startup versus established observations, unchanged charged-status checks and
quota IPC count, bounded closed breadcrumbs, both reconciliation callers and
original cleanup/error precedence. It ran no builds or live transport. It relied
on the root's logs for tests and did not independently audit all numerical
artifact claims. Cosmetic spacing findings in the candidate report were fixed.

Actual root checks on the integrated code passed:

- `cargo test -p marmish-native --features installed-checks --locked --lib installed_check::`:
  231 passed, zero failed, 633 filtered, 5.73 seconds. Log SHA256
  `1512ce33c88a9c77cba1107d861287f29a820a193d4a223ba4d18f2c1012ba5e`.
- `cargo clippy -p marmish-native --features installed-checks --all-targets --locked -- -D warnings`:
  passed in 18.06 seconds. Log SHA256
  `70cba064e801f2b4f38334760beebf1832e9282de7b0062af336f7fcf9aeeecf`.
- The full workflow-tooling Node suite: 158 passed, zero failed or skipped.
  Log SHA256 `03c7de9aadbc7b174bc91021e4d0425239c12224e341e441ff1dde97871a6b2f`.
- Scoped rustfmt and the exact staged diff whitespace check passed.

`ripwire --quality-delta` exited **2**, not clean: 17 gating rows. Root and Claude
triaged the findings rather than suppressing them. The reported complexity and
verbosity growth at unchanged `command_cli.rs::run_inner` is a name-resolution
collision with the new build-only wrappers. The flagged test functions actually
ran. Wrapper and cross-domain predicate clones, exhaustive-test nesting and
short-horizon churn are retained with explicit contracts. The separate closed
trace follows the approved design; a shared observation framework was excluded.
`prepare_gates` and private `identity::private_dir` are a real clone, but the
same inline copy already existed in edge-rsync: net copy count is unchanged.
Reusing it requires a production visibility change outside this correction;
no production change was made merely to silence the metric. The two inode
fixtures prove different, opposite filesystem transitions and remain separate.
Quality log SHA256:
`cafc984a1b23efe29e0ff1d7deb90845458fbfed3bc08aa81739d17f5a9367a6`.
The change-context report was also inspected; its name-based graph overstates
product reach for these feature-gated modules. Report SHA256:
`ee65b3d760d8790ca7a7ebb05ab4bb1ebd6ff87d579a10432c7266955e39a7f6`.

Test limitations remain explicit: the dwell call-site guard checks source text,
backed by a behavioral decision table; it is not runtime IPC evidence. The sink
precedence test mirrors a closure; the actual trace accepts only a borrowed
result. Gate ordering and reconciliation wiring were inspected rather than
proved by new daemon tests. Fresh installed qualification remains mandatory.
