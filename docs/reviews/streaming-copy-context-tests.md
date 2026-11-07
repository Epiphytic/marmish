# Inherited copy context qualification increment

This test-only increment supplements the approved native-copy design. Beads:
`marmish-8ka.4.1`. It changes no production behavior. The integration lead
approved implementation after independent design approval. Publication is recorded
separately by the integration lead.

## Scope and reviewed contract

Two test modules exercise the actual engine preparation/launcher and native
helper context reader. Existing files only gain test-module registrations and
`pub(crate)` visibility for the existing test fixture and descriptor-identity
helper. The engine crate uses Rust edition 2021; the native crate uses edition
2024.

The engine tests pin a duplicate descriptor so descriptor-number reuse cannot
masquerade as an open original locator. An owned prepared context closes on drop,
successful spawn, image revalidation failure, pre-fork working-directory failure,
and kernel exec-format failure. A borrowed launcher locator remains owned by its
caller across failures, then closes when that owner drops. Reserved locator
claims do not mutate the owned descriptor. Unexpected successful launches in
negative tests are cleaned up through identity-checked, unreaped `ChildOwner`
ownership before the assertion fails.

The native tests exercise exact compiled context acceptance, missing/truncated/
wrong-domain/entrypoint rejection, compiled build mismatches, invalid descriptor
kinds, oversized data and an open writer that never supplies EOF. Each asserts
consumption of the inherited locator. They call the context reader directly and
do not mutate the helper process's global signal state.

The oversized fixture initially used a concurrent producer. A local test failure
and independent review exposed a race: a nonblocking reader could run before the
producer and report `LocalIo`. The corrected fixture preloads a nonblocking pipe
before reading, keeps the writer alive, and records the queued size. A complete
4097-byte prefill must produce `Protocol`; a smaller pipe's incomplete oversized
prefix must fail finitely with `LocalIo`. The local run queued **4097/4097** and
exercised the actual size-cap branch. No universal pipe capacity is assumed.

## Actual independent Claude reviews

All four reviews used Claude Code with `--permission-mode auto`, model
`claude-sonnet-5-5`, session
`da09712e-4dc2-49ba-b6a5-b5116ced524c`. Reviewers were restricted to read-only
repository inspection; they did not run Cargo, live tests, or network work.

| Review | Verdict | Wall time | Turns | Disposition |
| --- | --- | --- | --- | --- |
| Design round 1 | REQUEST_CHANGES | 103.815 s | 11 | Specified exact fixture visibility, private-field access, owned versus borrowed lifetime assertions, and pinned descriptor identity. |
| Design round 2 | APPROVE | 19.668 s | 2 | Approved the bounded test-only interfaces and ownership assertions. |
| Code round 1 | REQUEST_CHANGES | 93.221 s | 5 | Fixed oversized producer race and unintended edition-2024 formatting churn in engine files; also added safe cleanup for unexpected successful launcher results. |
| Code round 2 | APPROVE | 77.109 s | 4 | Read both complete new test modules, all three existing-file diffs, and ownership types/call sites. Required clean author reruns; those passed below. |

Design review inspected engine preparation/launcher/Linux pre-exec, native
context parsing, protocol decoding and compiled metadata. Final code review
inspected complete new test bodies and targeted `ChildOwner`, `LiveChild`,
`SpawnedChild`, and process-identity contracts. macOS pre-exec and
`install_exec_image` internals were outside this scoped review. Local raw review
files were retained under `/tmp/marmish-context-tests-{design,code}-review-r*.json`;
only this sanitized evidence is published.

## Local validation

Linux, effective UID **1001**. Shared Cargo target was exclusively coordinated;
commands used `--locked`. Engine tests that return early as root were therefore
actually exercised here.

| Command / filter | Result |
| --- | --- |
| `cargo test -p marmish-engine copy_context_tests --lib` (final rerun) | 7 passed, 0 failed; 0.48 s test time |
| `cargo test -p marmish-native copy_helper::context_tests --lib -- --nocapture` | 6 passed, 0 failed; 0.00 s test time; oversized prefill 4097/4097 |
| `cargo test -p marmish-engine copy_launch::tests --lib` | 10 passed, 0 failed; 0.28 s test time |
| `cargo clippy -p marmish-engine -p marmish-native --all-targets -- -D warnings` (final rerun) | Passed; 5.38 s |
| `cargo fmt --all -- --check` | Passed |
| `git diff --check` for the three existing modified files | Passed |

The existing launch cohort includes closed standard descriptors, original pipe
FD 3 relocation, exact inherited context, concurrent context isolation,
authenticated policy checks, helper image replacement, and exec/pre-exec error
separation. The new modules supplement those tests rather than claiming those
boundaries were previously absent.

The first new native run had five passes and one oversized-fixture failure; the
corrected six-test cohort passed. This was a fixture scheduling defect, not a
product behavior change. Final engine and Clippy reruns also cover the later
unexpected-child cleanup helper and reuse of the existing descriptor-identity
helper.

The engine-scoped `ripwire --quality-delta` did **not** pass cleanly: four remaining
gates flag duplication between failure-specific test fixtures and a short polling
loop similar to an existing exit waiter. A duplicated descriptor-identity helper
was removed by reusing the existing test helper. Remaining repeated setup/assertion
sequences are retained to keep distinct ownership failure cases explicit; the
reported dead-code rows are test-harness functions actually executed above. No
production symbol changed in this increment. This is an explicit disposition,
not a claim of a clean static quality gate.

## Limits

These are local hermetic tests, not relay or installed cross-platform results.
The helper's hidden CLI missing-locator dispatch is not exercised by calling the
context reader directly. Same-UID inherited context supplies consistency, not an
authentication capability. Hosted non-root Linux AMD64/ARM64 and macOS candidate
runs remain separate qualification evidence; these local passes do not establish
those results.
