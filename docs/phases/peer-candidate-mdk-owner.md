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

Treat the locked `SqliteAccountStorage::open_encrypted_observed` as one
synchronous body. Recheck the exact registered default immediately before
entering it. The body creates private files, opens and keys the connection,
applies migrations, then installs the notification counter, TEMP triggers
and connection hooks before returning. The observer cannot refuse a phase.
The first observer callback follows the file setup and the native open.
An upstream failure's internal cleanup stays inside the admitted call.
On successful return place the complete root in custody before the next
recheck. If that check refuses, keep the root and skip its normal close and
all later Journal or runtime work. Fatal does not interrupt an admitted
synchronous open. Busy retry and migration make time-to-park unbounded.

The startup smoke fixture controls every clone. Independently admit its
ordinary `close()` or true last-owner drop and independently admit the
cold reopen. Collect both outcomes. The marker `closer().is_released()`
reports completion of the upstream close path. Establish release through
file locks and cold readback. A close error still leaves the disposition
uncertain. The explicit close can attempt another SQLite close through
rusqlite Drop after an error. In locked rusqlite 0.40.1 that retry is
`sqlite3_close`. The plain last-owner drop attempts close once and discards
its result.

The reopened root lives in custody before passing its first clone into any
fallible runtime construction. Reaching fatal uses the inert
`DatabaseCloser::begin_close` and leaves a strong root parked until process
exit. That prevents an anonymous clone from becoming the final owner.
The upstream ordinary connection lock refuses new guards after
`begin_close`. Existing guards can finish and internal
`try_lock_idle` deliberately ignores the closed flag. Stop task admission
at the Marmish boundary and prove that owned tasks and physical sockets
finish before parking. Late native results must be discarded. Independently
admit ordinary `close()` from the custody owner, then verify a later
anonymous clone drop has no SQLite effect. Preserve the exact original lease.

The installed startup inventory starts at `MdkOwner::open`. Marmish calls
`AccountDeviceRuntime::open` with `SessionConfig::with_storage`. Its
`SessionStorage::Open` arm reuses the supplied account connection, including
the engine and maintenance clone. The alternate `Path` arm opens a separate
connection and is outside this call. Marmish constructs its key-package
publisher, relay transport and routing directly. Neither `marmot-app` nor
`nostr-directory` is a native workspace dependency. The production mount
must still inventory exact dynamic SQLite roots, including any upstream
sibling open and host Journal, Terminal, Command, and recipient stores.
A root with an unguarded open or close cannot be included on the evidence
of the account fixture.

## Qualification

Use fresh subprocesses with disposable encrypted account files and
actual registered forwarding. Measure a healthy smoke open and cold
reopen, an ordinary wrong-key refusal under the intact dispatcher and
fatal cuts before initial open, after upstream open returns but before
its independent close, and after smoke close but before independent
reopen. For the re-opened root, demonstrate that a real fence cut and
`begin_close` cause a retained clone to refuse new database access,
then park the last root without advancing its release marker. Exercise
real displacement before the first progress callback by performing it in
the dispatcher's intercepted native open, and again during the admitted
upstream progress callback. Prove the caller suppresses the result and
holds native custody if the upstream open returns a root. Observe VFS open,
write, delete and sync separately, attributing any ordinary checkpoint to
the admitted close. Cover explicit close with a surviving clone and prove
later drop has no file effects. Identify or exercise the held-guard and
sibling-connection limits through the inspected upstream contracts. Capture
exact close/open/source observations
without logging paths or secrets. Keep stock behaviour unchanged.
Claude reviews this design and the observed limits before production
mounting is allowed.
