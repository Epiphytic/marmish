# Qualify the installed daemon's upstream MDK account ownership

Bead `marmish-dwt.4.12.2.1` follows the host SQLCipher slice and the
[dispatcher shutdown design](peer-candidate-dispatcher-shutdown.md).
Use the current mainline MDK at locked `bdf9833`. The manifest continues
to select upstream master. The production Proof, Founding and recipient
staging paths have separate quota and lifecycle boundaries.

## Boundary to test

The reviewed dispatcher design permits one admitted synchronous body to
finish, with its own internal cleanup. An independent connection open
or destructor requires another admission. The test below determines
whether the locked upstream account opener and the true last-owner drop
are compatible with that boundary. It does not mount the daemon.

Treat each `SqliteAccountStorage::open_encrypted` as one synchronous
body. At the caller, recheck the exact registered default immediately
before entering it. Upstream creates private files, performs one
rusqlite open, keys it, initializes its tables and applies migrations
inside that body. The progress observer cannot refuse any of these
phases. A failed upstream open returns no owner and its internal cleanup
is part of the admitted call. On success, move the storage into custody
before the next recheck. A refusal after a success retains that complete
root and no reopen or Journal transaction follows. If the admitted open
can return an unowned native handle after ordinary internal failure,
record that separately and do not claim the path qualified.

For the existing smoke open, a test guard owns one unshared account
root. A second recheck admits its ordinary destructor. Observe
`database_closer().is_released()` after dropping that root, then
independently admit the cold reopen. Use no close call when the check
refuses. The explicit upstream checked close checkpoints and on error
drops the returned rusqlite owner, which retries the SQLite close. In
locked rusqlite 0.40.1 the retry is `sqlite3_close`, despite the upstream
comment referring to `sqlite3_close_v2`. The plain last-owner drop has
one close attempt but no returned SQLite error. `is_released` proves
only that this path finished, so use actual file-lock closure and
cold readback as the smoke qualification. Do not treat the marker as a
checked SQLite close or use it to dismiss a reported failure. Review
whether the unshared proof is enough for product startup, or an upstream
checked no-retry close/admitted connection API is necessary.

For the reopened root, keep one strong root from before any native
runtime construction through all fallible initialization. No first
owner clone may outlive it. On observation of fatal, bind the native
API shut with its inert `DatabaseCloser::begin_close` only, retaining the
root so its final destructor does not run. Any other clone dropped
under the same cut must be unable to trigger last-owner SQLite close.
The future mount must separately prove which runtime, tasks and sockets
are reaped before parking and must retain the original lease. Late
returned native results cannot publish availability or success. Do not
add a new runtime, fork dependency, SQL path, SQLite registration, or
public MDK interface in Marmish for this proof.

## Qualification

Use fresh subprocesses with disposable encrypted account files and
actual registered forwarding. Measure a healthy smoke open and cold
reopen, an ordinary wrong-key refusal under the intact dispatcher and
fatal cuts before initial open, after upstream open returns but before
its independent close, and after smoke close but before independent
reopen. For the re-opened root, demonstrate that a real fence cut and
`begin_close` cause a retained clone to refuse new database access,
then park the last root without advancing its release marker. Exercise
real displacement while an open is inside an admitted upstream progress
callback, to prove the result is suppressed and its native owner
retained if it returns. Capture exact close/open/source observations
without logging paths or secrets. Keep stock behaviour unchanged.
Claude reviews this design and the observed limits before production
mounting is allowed.
