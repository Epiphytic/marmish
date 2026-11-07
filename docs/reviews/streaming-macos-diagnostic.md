# macOS installed streaming diagnostic reviews

Beads `marmish-8ka.5.8`; baseline product source
`571d98b77780d265a2fb29d84ae160fec72ba37b`. Scope is the
[bounded diagnostic design](../phases/streaming-macos-diagnostic.md).
This diagnostic cannot qualify the failed installed matrix or a release.

## Independent design review

Actual Claude Code used `--permission-mode auto --model claude-sonnet-5-5
--output-format json`. Session `aa3821bf-c9e7-436a-8ce1-8ed4e4a8303f`:

- Round1: **REQUEST_CHANGES**, 216.180 seconds, 25 turns. Blocking findings
  required preserving partial captures on early child failure, enforcing old
  product/new harness provenance, removing cumulative server command limits,
  and explicit asynchronous cleanup on every exit with coherent budgets.
- Round2: **REQUEST_CHANGES**, 66.267 seconds, 8 turns. All four original
  findings resolved. The remaining blocker corrected bootstrap verification:
  the artifact is rendered from `scripts/install-native.sh`, rather than being
  the literal source `scripts/install.sh`.
- Round3: **APPROVE**, 18.061 seconds, 3 turns. All blocking findings resolved.
  The reviewer confirmed the rendered-template check and two-second bounded
  drain grace before descendant cleanup. Full-history checkout and its policy
  regression remain implementation requirements.

The reviewer inspected actual helper contracts and independently checked the
candidate/run and three artifact ZIP checksum anchors through the read-only
GitHub API. No source edits, Cargo tests, live relay workload, dispatch or Git
mutation ran during design review. The coordinator recorded approval before
delegating implementation. Original product binaries, old fixture and rendered
bootstrap remain distinct from the new build-only diagnostic harness.

## Integrated code review

Actual Claude Code auto review used session
`7fcb9f6a-37f9-4770-8900-890cd8765547`, model `claude-sonnet-5-5`.
The first invocation reached its 30-turn limit after 301.185 seconds (31 recorded
turns); it made no approval. The resumed invocation returned **REQUEST_CHANGES**
in 25.242 seconds, one turn. The blocker was missing hermetic coverage promised
by the approved design: failure classifications, terminal/receipt decisions,
finalizer ordering and stop-both behavior, pass/count/cleanup precedence,
post-link publication failure, and a copied-fixture check. The reviewer found
no proven diagnostic runtime defect in the inspected source. Existing JS tests
already exercise ancestry; that guard does not need a duplicate Rust test.

The reviewer actually ran 20 Node tests, 12 Rust diagnostic tests, workspace
formatting and strict installed-checks all-target Clippy; all passed. Two earlier
Cargo filters matched zero tests and are not counted. No live workload, hosted
run, source edit or Git mutation ran in review. Corrective tests and useful
interruption/reporting improvements were implemented and re-reviewed below. No
hosted result or installed qualification is asserted.

## Author preflight evidence

The acquisition helper was additionally exercised on all three actual frozen
artifact ZIPs through read-only GitHub downloads, without executing candidate
binaries, installer or probe. All fixed ZIP sizes and checksums passed, with
five product members, three tooling members and one bootstrap member. The actual
version 0.2.3 macOS manifest was 601 bytes with SHA256
`abfe1ae7daa0ebc374c271a01f272f38b7332ec57fbbb766de5d437b4f193e59`.
The old tooling record was 440 bytes with SHA256
`9684214b58e7ee52676d45ea40c60bf33ae735ae8c2da37386a0cca329d8456b`;
its 6,546,288-byte fixture matched SHA256
`e745c823a75a781908acbf2b9fac41614e8c1996cbc5afccd1b66231ac6bcc55`.
The 9,120-byte rendered bootstrap passed the actual frozen template check.
These checks establish input consistency only, without macOS runtime evidence.

### Corrective author evidence

The real copied-fixture local check was run separately from Rust unit tests. A
fresh locked installed-checks build produced `marmish-installed-fixture`; the
author copied it to a new private `fixture/main` inode (mode0700), created private
receipts, and executed its exact six literal arguments, including empty, quotes,
backslash, substitution-looking text, semicolon, globs, newline and Unicode. The
nonce was 32 repetitions of hexadecimal `ab`. Independent subprocess input/output
checks used the fixture's byte generator with labels7/43/149. The result was
exit7, 98,304 stdin bytes, 98,304 stdout bytes and 8,192 stderr bytes in 2.854
seconds, with exact bytes and a validated receipt. Receipt checks included the
actual image hash, canonical vector-of-UTF8-byte-arrays argv hash (including its
required trailing newline), and input/output/error hashes and lengths. The
copied image SHA256 was
`f049433a0dd3520f483865aabdd442b3990ea95d50153be6107d05fa929bf2ae`. The
15-second subprocess deadline was not exhausted; owned temporary state was
removed. The first external observer omitted the argv canonical newline and
failed its own assertion; correcting that observer required no source change.
This is **new-source Linux offline harness verification**, not old-candidate,
macOS, relay or release evidence.

The corrective JS suite independently validates closed failed reports and markers,
while retaining failure exit status. The workflow outer probe/job limits are now
19/48 minutes; inner work/cleanup remain900/180 seconds. This supplies record and
upload margin without expanding the live workload. Corrective Rust tests and
independent code reapproval passed as recorded below.


### Corrective independent approval

The same actual Claude auto session returned **APPROVE** after 66.382 seconds,
nine turns. The reviewer approved the administrative budget amendment and found
no remaining blocking code findings. It inspected the production seams and their
tests, and independently reran 21 Node tests, 18 diagnostic Rust tests, workspace
formatting and strict all-target installed-checks Clippy: all passed. It did not
rerun the author's full 181-test installed-check suite, the copied-fixture check,
any live workload or any hosted execution. No source/Git mutation occurred.

Author final checks passed 181 installed-check tests (including the18 diagnostic
cases), strict feature all-target Clippy, formatting and diff checks. An actual
Linux debug-probe build completed in20.76 seconds and produced201,172,992 bytes,
inside the268,435,456-byte limit; this does not establish macOS binary size. The
seven reviewed code files' canonical path/SHA256 map has SHA256
`4ebdcfa3058b4c66140bb52d12108bcb9782073c7dd7567a55f7fc2f7aac05db`.

Ripwire change context and quality deltas were inspected. The quality run exited2
with name-collision attributions to unchanged product functions and real
complexity in the new owned-child capture, chronological setup and typed cleanup
operations. This is not a quality-clean claim. Ownership ordering and all six
cleanup operations remain explicit and tested; the reviewer accepted the bounded
implementation without a broader refactor. No product/shared-helper, dependency,
version or release changes were included.

Residual nonblocking diagnostic limits include a single-shot server terminal
snapshot, unrecorded batch-level List counts when concurrent streams have no
prefix, and the source guard running after build. Existing closed markers and
failures prevent these limits from qualifying a release. The hosted macOS run
must still exercise actual installer, daemons, relay and cleanup. Diagnostic
success cannot qualify the failed matrix or cross-platform interoperability.
