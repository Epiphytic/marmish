# Exiting-member cleanup review

Beads `marmish-4a3.1`; [detailed phase](../phases/macos-exiting-member-pending.md).
Claude Code (`claude-opus-5-5`) reviewed the actual integrated source in
`--permission-mode auto`, with repository inspection and bounded local checks.

Design session `811c9a8c-395a-44f4-b4ed-a07263fb2085` required three original
corrections and then a sticky failed state; v3 received **APPROVE** before
implementation. Approved design SHA256:
`b7acbd03fbc7dd81d10138ca281c9fb33bee5ad81df9ed8d1a2f914a9ca457b7`.
Actual design approval JSON SHA256:
`c7e55eed5efff4093d7cc9e910e0394c6c44c43410fc687fca078f20a6d0cf56`.

Completed-code session `42f03715-b8e9-482a-9f8e-3cfba9963640` initially required
one behavior-preserving Clippy fix: collapse the runner's nested start/poll
condition using an edition 2024 let-chain. After correction and final-byte
checks, the actual follow-up returned **APPROVE**, with no blockers.
Actual follow-up JSON SHA256:
`2dd71f291dbb58cda985b68b8412df396fe7399ecfcb17d819a9985060af5aeb`.
Final eleven-file source/phase pin manifest SHA256:
`d62ef9d6e4c05ff759e5062082b532438f6aae307d98c9e5161ddaf3c90831d8`.
The reviewer also inspected the specification correction, SHA256
`dee1d1d7a7fba4ded4a173c6455a7cc0f4dde182e003e33785648e56c7be79b5`.
The extra test-fixture change only initializes the new private cleanup state.

## Local validation

- Combined engine/native strict Clippy, all targets: debug, release and
  `marmish-native/installed-checks` each passed on final bytes.
- Engine library: 144 passed, zero failed, one existing ignored test.
- New runner budget family: exactly four passed in debug and release.
- Affected runner family: 24 passed, zero failed, one existing ignored test;
  recorded before the mechanical let-chain correction, with its four budget
  tests rerun afterward.
- Formatting and diff checks passed. The reviewer independently reran the
  workflow policy tests: nine passed.

These are Linux checks; they do not establish nonroot Darwin runtime behavior.
The five Darwin engine tests, seven retained tests, 100 authority observations,
original combined suites and 80 repetitions, SDK assertion, and strict Darwin
Clippy remain required in the exact-source guarded workflow.

`ripwire --quality-delta` returned exit 2, **not PASS**. The reviewer examined
all 24 gating findings: 17 history-churn signals, five unrelated normalized
match/wrapper clone findings, and two justified length increases for the
explicit no-reap guard and exact workflow gates. None was judged a real
maintainability blocker; no metric was suppressed or relabeled.

Approval covers source for controlled hosted validation under the disabled
publisher. It is neither completed macOS validation nor release qualification.
Pending never establishes terminality or authorizes reaping. Strict finite and
Drop behavior remains unchanged; Drop may perform its own strict cleanup after
a runner failure. Historical coarse failures remain unexplained.

## Darwin import follow-up

Guarded run `42645112715/1`, source
`1ca0c46c4e7038eadf46bb10bd0ee7aee85353f0`, failed during compilation
with E0659: the resolver test's unqualified `diagnostic` import conflicted
with the tool namespace after a glob import. Cargo exited 101. The SDK, all
deterministic tests, authority100, original combined suites, cleanup80 and
Clippy were skipped. Private strict terminal summary SHA256:
`c2b194381ae94837f11a21f3fd30f9903ae36c8f90652d79da9320d615057e7c`.

The sole source correction qualifies that import through
`crate::process::diagnostic`; runtime behavior and test selection are unchanged.
The affected portable process tests passed (two tests), strict engine all-target
Clippy passed, and formatting and diff checks passed. These Linux checks do not
compile the Darwin-only test. Quality delta returned exit 2 for two churn
findings on the edited test and its module, with no structural regression.

Claude Code auto follow-up in session
`42f03715-b8e9-482a-9f8e-3cfba9963640` returned **APPROVE**, verified both local
log hashes, independently checked formatting and the diff, and inspected debug
and release visibility plus the other new Darwin imports. Actual review JSON
SHA256: `7855949400f7edade25cd0df241745af8997ba40babf28772afdae78d7bcc90b`.
Approval covers one fresh guarded exact-source run; all hosted gates remain
required. Neither Darwin runtime success nor release qualification is claimed.
