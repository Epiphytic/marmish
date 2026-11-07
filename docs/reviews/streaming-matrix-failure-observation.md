# Original stream-matrix observation design review

Beads `marmish-8ka.5.8`; [detailed design](../phases/streaming-matrix-failure-observation.md).
Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`, session
`83027a18-86db-4c9b-b8f3-8ee144b78a31` reviewed the actual source contracts.

- Round1: **REQUEST_CHANGES**,89.771 seconds,11 turns. Required concrete
  fallible sink/pure test seams, attributed Pair predicate checks preserving
  short-circuit errors, full terminal/active projection, nonpanicking locked
  stderr/saturating time, begin-before-await and reserved final/truncation slots.
- Round2: **APPROVE**,18.258 seconds,three turns. All five blockers resolved.
  Spot-checked CLI remote error tokens, Terminal/WireErrorCode and original
  validator order. No edits, tests, Cargo, live traffic, dispatch or Git mutation.

Nonblocking dispositions: exact wire-error scan applies even if prefix parsing
fails; ambiguous is a closed enum, not arbitrary text; short-circuit null checks
show only the first failure, matching original behavior; failing-sink parity is
required for success and each exercised failure branch.

The coordinator records design approval and authorizes implementation within
these build-only files and tests. This is observation of the unchanged original
matrix pipeline, not a product fix, acceptance relaxation or qualification pass.
Integrated code review and all affected checks remain required before publication.

## Implementation and local checks

The frozen implementation is limited to six build-only Rust files: module
registration, the existing process/cases/records modules, and the observer plus
its tests. The original duplex workload, command arguments, absolute deadlines,
report schemas and acceptance predicates remain the qualification contract.
There is no product, dependency, release-version or workflow change.

Author checks on Linux AMD64 with Rust1.97.1:

- `cargo test --locked -p marmish-native --features installed-checks --lib installed_check::`:
  203 passed, zero failed or ignored, 613 filtered, 5.57 seconds.
- `cargo clippy --locked -p marmish-native --features installed-checks --all-targets -- -D warnings`:
  passed, 7.47 seconds.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

The added contracts cover exact output/error parity with recording and failing
sinks, early input closure, wait expiry, stdout-only and stderr-only drain expiry,
overflow, owned-child termination/reaping, real I/O and join-error classification,
prefix ordering, informational remote tokens, binding terminal projections,
ordered construction/validation checks and cancellation while waiting for List's
lock. No live test result follows from these hermetic checks.

`ripwire --quality-delta` returned exit2 with ten gating findings; this is not a
clean metric gate. Bounded real growth is duplex70 to81 lines, a114-line shared
runner, a78-line ordered checker and a127-line parity test (complexity19).
These retain a coherent process-ownership and short-circuit contract. Other
findings include generic-name and enum-shape collisions, missed cfg-test/trait
reachability and an unchanged engine-module attribution. Recent duplex churn is
real. The read-only review bundle is stamped d06fd47fc+dirty; new untracked files
must also be inspected directly. No unrelated refactoring was introduced to
change the metric score.

## Independent integrated code review

Actual Claude Code `claude-sonnet-5-5` in `--permission-mode auto` returned
**APPROVE**, no blocking findings, session
`980926d0-0d60-485b-a141-6e959263989e`: 95.751 seconds,13 turns. The reviewer
inspected the tracked diff and both new Rust files, checked process ownership,
error/deadline preservation, shared validator ordering and bounded sanitized
evidence. It independently ran the installed-check library tests, feature
all-target strict Clippy, native formatting and diff checks successfully.
Its test report listed203 passing tests plus a separate one-test result block.
The latter is the existing socket test's self-executing subprocess; these are
not summed into a new suite count.

Both useful nonblocking test suggestions were incorporated: pin the decisive
event ordering rather than only event presence, and assert every terminal
projection's serialized shape. Their focused tests passed (one each,1.08 and0.00
seconds); formatting and diff checks passed. The initial strengthened binding
assertion included begin events and failed; its evidence-kind filter was
corrected before the passing rerun. Other limits remain explicit: synchronous stderr
observation adds time inside the original wall-time scope, oversized lines are
dropped, the actual async wrapper is covered by the hosted matrix rather than a
hermetic daemon test, and tests exercise Unix platforms. Role parsing has the
same closed roles already enforced by nonce construction. None changes the
acceptance contract or establishes a root cause/qualification result.

The same actual Claude session reviewed the two strengthened tests and returned
**APPROVE** again:14.977 seconds,three turns. It independently reran both focused
tests (one pass each,1.07 and0.00 seconds), formatting and diff checks. No source
behavior changed after the integrated approval. The follow-up approval covers
source/tests; coordinator documentation records the already reviewed evidence.
