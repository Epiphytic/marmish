# Live inbox publication and strict readers

Status: implemented, locally validated and approved by actual Claude auto code reviews on 2026-11-08; fresh hosted qualification pending.
Baseline: `e31b8afae452bb30a041916fbf6d2bac4e92f042`.

## Evidence and scope

Candidate 42504599079/1 passed all three native package jobs and rendering.
Installed stream run 42510167247/1 passed. Rsync run 42510560482/1 stopped the
sequence: macOS reported `remote_client_barrier` / `work_error` / `invalid`.
All three retained client results report six successful ordered transfers;
Linux server results contain six receipts each, whereas the macOS result is
incomplete with no receipts. The rollup failed and qualifies no rsync cells.

Independent artifact comparison found no retained payload or binding mismatch.
Failure-time inbox inode metadata is unavailable. The following source defect
is demonstrated, but is not established as the cause of that hosted failure:
`transport.mjs::putImmutable` publishes a fully written file by hard-linking it
into the inbox, then asynchronously removes its staging directory. During that
interval the target has two links. `installed_records::read_bytes` rejects any
link count other than one. Streaming and finite-matrix rendezvous readers can
observe the target during that legitimate interval and fail immediately.
The native immutable writer uses the same link-then-unlink pattern.

Correct only the live rendezvous reader contract. Retain atomic no-replacement
publication, strict completed-record validation, existing polling intervals,
deadlines, workloads, schemas, retries and acceptance predicates. No product,
protocol, installer, release version or public pin changes. No historical cause
claim or retry of the failed run. Beads remains unavailable because its remote
schema53 cannot be safely opened by the installed schema66 client; do not
migrate, initialize, alter or bypass the database.

## Interface and safety

Keep `records::read` and `records::read_bytes` strict for ordinary consumers,
offline rollups and all accepted records. Add one polling-specific typed read
returning `Result<Option<T>>`: `None` means a structurally safe regular file is
still in the two-link publication interval. Use the existing opened descriptor,
not a pathname precheck followed by a separately opened file, to inspect owner,
mode, type, size and link count. Factor the existing bounded byte read privately
to avoid duplicating its descriptor flags or validation.

Check all existing metadata fences first. Only an otherwise safe regular file
with exactly two links may return pending; zero or more than two links, wrong
owner, unsafe mode, nonregular type, excessive size and malformed JSON remain
errors. A one-link record follows the exact existing bounded byte read and JSON
deserialization. Pending bytes must never be read, parsed, validated, inserted
into the rendezvous map or accepted. Do not sleep or retry inside the reader.
The caller's existing deadline and 100 ms polling loop bound pending state;
a permanently two-link file must time out without acceptance.

Wire this operation into the four live inbox loops: streaming servers and
clients, finite matrix servers and clients. Preserve their existing existence
checks, validation calls, map rebuilding, deadline checks and sleep cadence.
All later schema, context, pin, identity, pair, receipt and physical cleanup
proofs remain mandatory. Offline rollups continue to use strict reads.
Do not change JavaScript publication, transport behavior or retries. Do not add
a generic catch/retry for `Invalid`; only the explicit pending result waits.

Also wire the three live native marker readers using the same publisher contract:
`installed_edge_rsync::started`, `installed_edge_quota::started` and
`installed_edge_signals::running_snapshot`.
Preserve their existing phase eligibility, charged-slot validation and 50 ms /
caller polling deadlines. Pending before marker establishment means not ready.
Once `running_snapshot` has established a one-link marker, a subsequent pending
two-link observation must fail, using the existing established-marker invariant;
it must not become an opportunity to wait through tampering. Keep marker digest
and schema/nonce/executable/argv checks strict after publication. Preserve the
existing rsync startup source-contract assertions and unchanged reconciliation
gates. Add focused filesystem tests of any small marker-read seam needed to
exercise the established and not-yet-established cases without live IPC; source
wiring review is distinct from runtime IPC coverage.
This is hardening of the same source-demonstrated defect, not evidence that an
edge cohort caused the earlier hosted failure. Presence-only marker checks in
quota, preopen and signals do not read records and need no change. Second marker
reads for digest pinning remain strict after the first one-link observation.
The rsync configuration is fully published synchronously before starting the
client command that can launch its remote fixture, so its launch-time strict
read is not a concurrent polling reader.

The fixture's `released()` also polls a strict byte read, but has no publisher
in the controlled edge cohorts: the only edge `.release` references are its
path construction and read; hold/flood fixtures are ended by the cancellation
tests. The separate recovery fixture writes a release file under its own
recovery paths and uses its existing `read_fixed(..., 2)` contract. Neither
release protocol is changed here. This exclusion follows bounded source tracing,
not an assumption that every polling reader is safe.

An `ENOENT` on open, including a lookup/open race after `try_exists`, remains an
error rather than pending. No record deletion is part of inbox publication.
Permanently safe two-link records change from immediate Invalid to Timeout at
the original deadline; they are never accepted. Malformed content is rejected
once one-link publication permits reading; pending content is not parsed.

## Regression and acceptance

First validate the retained three rsync client payloads with the actual native
private validator in a temporary test-only reproduction, before implementing
the correction. Keep private records out of the repository; this is not hosted
qualification. Then use real temporary files with a hard link retained to hold the publication
interval deterministically, without timing sleeps or remote services. First
create a compiling baseline test seam that delegates to the strict reader and
observe the intended pending assertion fail with Invalid. Implement the new
reader and demonstrate pending while linked and exact successful bytes/JSON
after unlinking the staging name. Assert strict readers still reject the linked
file and malformed/oversized/unsafe/nonregular/symlink files remain errors.
Test link counts greater than two separately. Preserve existing strict-reader
and immutable no-overwrite/publication-fault tests.

Exercise actual streaming servers/clients loops and finite-matrix loops where
existing fixtures permit it: one pending valid record cannot complete a
rendezvous; unlinking permits the unchanged validators to complete; permanently
linked records hit the original deadline. Use paused Tokio time or bounded
owned synchronization rather than nondeterministic scheduler races. Deliberately
malformed linked content may exercise pending-then-invalid without constructing
complete valid clients. Extend streaming servers diagnostics coverage to prove
pending reaches `ServerRendezvousWait` with no peer role. Test the three native
marker readers' pending and established-tampering decisions; do not claim live
IPC execution from their isolated filesystem seams. The existing coarse client
barrier diagnostic remains a known limitation if a future Invalid recurs.

Use agents with disjoint ownership: one implements the shared reader and its
filesystem tests; another wires and tests the four rendezvous and three marker call sites.
Root coordinates Cargo use, integration, strict installed-feature Clippy, fmt,
the affected installed-reader/matrix/streaming suites and existing Node tests.
Run ripwire quality/diff checks without suppressing findings. Obtain actual
Claude auto code approval of the integrated change and test evidence. Update
specification and sanitized failure evidence, then commit and push to GitHub
and Radicle with verified external availability.

New source requires its own three-platform candidate followed by the original
eight serial cohorts and exact final auditor. Do not mix earlier results or
weaken any gate. Preserve both failed chains and their unknown hosted causes.
Publish application wall time and goodput with their limits only after actual
evidence verification; no RTT or wire-rate claims.

Approved v3 design snapshot SHA256: `c3a9eef82f54600c7a2437911d8e5c87b14ef0131dcedb3235957f7c103fbea8`. Actual Claude session `1670a315-634f-472b-810f-3249668c5594` approved all seven live call sites; the implementation must retain strict readers, no-overwrite publication and existing deadlines. This repository copy adds approval metadata and corrects the marker-count typo.

Implementation and test-only followup received [actual Claude approval](../reviews/streaming-record-publication-code.md). [Local evidence](../validation/streaming-record-publication-local.json) preserves meaningful RED/GREEN, four actual polling loops, three marker seams, positive stream and matrix completion, strict negative fences and original parallel benchmark failure. Clean isolated/serial and independent parallel reruns do not explain that failure. New exact-source hosted qualification remains mandatory.
