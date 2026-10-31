# Native managed-suite portability: integrated code review

Independent Claude Code in auto permission mode, actual model
`claude-opus-5-5`, initially **NOT APPROVED for publication as submitted**.
It found no test-code blockers and confirmed all six design conditions. Its
publication objections were missing promised source hashes in the phase
checkpoint and documentation scope changing during review: root added the
agent-spec paragraph and an agent appended the quality disposition after the
review began. Final review explicitly includes those documents; the checkpoint
will record final hashes. The root review prompt incorrectly said those hashes
were already in the checkpoint. This record corrects that claim.

Raw local result: `/tmp/marmish-native-portability-code.raw.json`.
The reviewer independently passed 68 affected tests normally and through a
symlink, and all 512 tools tests with 8 skips in 21.6 seconds. Diff check passed.
It caught all 16 bounded disposable-copy mutations, with every child owned,
60-second limits, exit 1 and copies removed. These included the author's five
mutations plus scan-call/order, signal installation/restoration, environment
restoration, Darwin/Linux admission, individual set-ID bits, access ordering
and prerequisite ordering. A separate Linux cleared-bit simulation correctly
produced two failures. No managed proof, actual Darwin, CI/network,
administrative, keychain, Beads/git mutations or subagents were used.

The scanner lifecycle clarification was checked against production: scan runs
after e2e/secret validation and before exit; later teardown failure retains its
earlier scan. The reviewer says verbatim: “A human should explicitly accept
this.” Its code assessment nonetheless states all six conditions are met and
there are no code blockers. This record grants no approval or implementation
authority; governing direct-user phased-work instructions determine authority.
The exact design-review human-approval caveat remains in the design record.

## Useful findings incorporated before final review

The C fixture should include sys/proc.h before syscall-name macros so SDK
declarations are not renamed by later production-header inclusion. Add an
explicit regression for Linux silently cleared special bits, retaining the
Darwin simulation and unconditional deterministic predicate coverage. Both refinements are implemented. Final affected runs passed 69 tests normally
and through a symlink; full tools passed 513 tests with 8 skips in 21.507 seconds.
The five author mutations were re-detected under bounded ownership. The phase
checkpoint now contains all three final source hashes. Narrow review explicitly
includes the three tests, phase, design/code review records and agent-spec diff.

The reviewer also noted short controlled /tmp fixtures were not documented:
Unix socket path length is checked before private-path/hash validation, so a
long ambient TMPDIR could mask these targeted checks. This affects disposable
test fixtures only and matches existing repository fixtures; no runtime path
policy changes. Signal/environment restoration mutations were detected by the
first affected test, not every test independently. Broader report-shape cases
and style nits were nonblocking observations, not native acceptance evidence.

The root quality scan exited 2 against 49c95d3: 34 findings, 17 gating, 2 minor,
15 new-symbol rows. Actual test clones, fixture parameter/line growth, class
growth and churn are disclosed in the phase; callback/unittest discovery flags
are structural limitations. No suppression or baseline reset and no clean
quality claim. PR-context exited 0; actual tests provide executable validation.
Final source changes require fresh affected checks and narrow review.

Final root structural checks again returned quality exit 2 and PR-context exit 0.
Final quality counts: 37 findings, 17 gating, 2 minor and 18 new-symbol rows;
no suppressions or baseline reset. The three additional rows are new-symbol
discovery/structural findings from the mandatory Linux cleared-bit regression.

## Final narrow review: APPROVED

Independent actual `claude-opus-5-5`, auto permission mode, APPROVED publication
of all seven named paths with no blockers. It confirmed final source hashes,
checkpoint hashes, explicit scope, truthful initial rejection, unchanged
production files and all exact review quotations. It checked the SDK preinclude
and independently detected removal of the Linux-mandatory assertion using the
new regression (bounded disposable copy, exit 1, cleaned).

Actual independent affected checks: 69 tests normally, 69 through a symlink,
no skips; full tools 513 tests, 8 skips, 21.625 seconds; diff check passed.
It confirmed final quality counts 37/17-gating/2-minor/18-new, acked=0, and the
honest duplication/growth disclosures. Earlier 16 reviewer mutations and five
repeated author mutations were not re-run by this narrow review. The C1 nuance
matches the existing production order and is not represented as an unscanned
teardown failure. No actual Darwin, network/CI, managed proof, administrative,
keychain, source or git changes were performed by the reviewer.

Raw local result: `/tmp/marmish-native-portability-code2.raw.json`.
Native acceptance and the historical configured SIGABRT remain open. A later
status edit recording this approval changes documentation only, not source
hashes or the measured source tests.
