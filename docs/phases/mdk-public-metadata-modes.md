# MDK public metadata modes correction

Beads: `marmish-ldi`. This is a bounded corrective increment to K2.

## Evidence and boundary

CI run 40975469283 failed the exact pinned MDK bootstrap test
`marmish_bootstrap::tests::metadata_and_setup_recovery`. In a clean temporary
checkout, the same compiled test fails under child umask `002` and passes under
`022`. Metadata-only strace shows the account directory created explicitly
`0700`, setup metadata explicitly `0600`, and public `account.json` created with
the default requested mode `0666`. Under `002` that becomes `0664`; the injected
bootstrap correctly rejects group-write permission on this record. This is a
production initialization defect under that mask, not only a fixture defect.
No contents, credentials, or sibling working trees are needed for the diagnosis.

## Change and invariants

Extend the maintained bootstrap patch in `patches/mdk/marmish-bootstrap.patch`
with a Unix-only change in `marmot-account/src/io.rs::create_temp_file`: request
`0600` for private writes and `0644` for public writes. Apply the mode at
exclusive temporary-file creation before any payload write or publication.
Keep the private-file descriptor chmod and all atomic rename/sync behavior.
Do not chmod public files after creation: the requested maximum must continue
to honor a stricter process umask (`077` produces `0600`). Do not change the
process-global umask, relax bootstrap validation, repair preexisting writable
metadata, or change non-Unix behavior.

Extend the existing bootstrap metadata test to self-reexec its exact name with
one private test-only mask marker, once each for `002`, `022`, and `077`.
Set umask only in the child's async-safe pre-exec hook; never mutate the parent
test process mask. In the single-test child, verify the effective mask by a
same-value set/readback before creating fixture files. Assert exact generated
public modes `0644`, `0644`, and `0600`, respectively. The parent requires both
successful exit and the exact named one-test success result (zero tests is a
failure), with a fixed ten-second child deadline and kill/wait on timeout.
Print no raw child output on failure, only fixed assertion descriptions.

Add a new explicit negative check: chmod owned `account.json` to `0664`, require
metadata rejection, restore the saved expected permissions, and require success.
Do this before the existing extra-account rejection so failures cannot mask
each other. Retain existing identity/setup assertions. No secret-bearing fixture
diagnostics are printed.

Refresh the bootstrap patch hash and both complete Linux/Darwin source guards
from fresh pinned temporary trees, applying the restricted patch only for
Darwin. The upstream revision, Cargo.lock, and restricted patch remain unchanged.
Rebuild the Linux daemon from reviewed guarded source and emit a matching
manifest. The ordinary builder runs its three exact bootstrap tests once; the
metadata test itself owns the mask matrix. Also run the exact metadata test
externally under child masks `002`, `022`, and `077` as validation of the parent
path under those environments, and run account `io::tests` affected by the mode
change. All exact-test gates require the named test and one-test success count.
Use only temporary clean MDK trees and the designated build cache; never modify
the dirty sibling checkout. No native macOS execution is claimed.

Builder captured-output loss is outside this minimal security correction;
retain fixed, sanitized evidence of this known failure instead of printing
unbounded unexpected subprocess output into CI logs.

## Review and completion

Obtain independent Claude design approval in auto permission mode before source
edits. After implementation, run the focused tests, source guards, patch apply
checks, relevant Python provenance tests, and formatting/diff checks; obtain
independent integrated Claude code review in auto mode and address blockers.
Record sanitized review and validation evidence. Report the rebuilt daemon path,
digest and manifest to the parent for coordinated publication; do not commit or
push independently. A successful local run does not prove the failing CI host's
actual umask, only the reproduced mechanism and the corrected behavior.

Preexisting group-writable public metadata remains rejected. Operators must
audit ownership and the affected record before explicitly correcting its mode;
there is no automatic repair or permission relaxation in this increment.
