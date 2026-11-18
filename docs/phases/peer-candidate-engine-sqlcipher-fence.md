# Candidate Command engine SQLCipher connection fence

This is an unmounted prerequisite of the reviewed
[Command worker keeper](peer-candidate-fatal-command-keeper.md) and
[fatal dispatcher design](peer-candidate-dispatcher-shutdown.md). Start from
`33c7338`. Bead `marmish-dwt.4.12.2` tracks composition. The only new
authority is the existing candidate Command admission at independent
engine connections. Keep the daemon, shared default registry, Peer and
Terminal unchanged.

## Owned call sites

- `Engine::open_storage` creates the native state root and later calls
  `Store::create_frontend`. `Store::connection` prepares the on-disk
  state file, uses the shared SQLCipher opener (WAL), and verifies the
  sidecars. `create_frontend` can return after opening, before there is
  a Store, while populating schema/configuration, or during post-commit
  file proof. `Engine::open_storage` can also return an error after the
  completed Store goes into a partially constructed engine.
- `Engine::read_output` gets the owned output status under the existing
  per-job IO lock and calls `Spool::read → load_prefix`. Each saved
  stdout/stderr prefix can open its own SQLCipher connection (DELETE
  journal). A regular spool error currently describes unavailable
  retained bytes and can infer a gap. Admission refusal is a separate
  typed engine error and bubbles out before gap inference. Explicitly
  close the checked connection before returning bytes. Recheck after the
  inner call before consuming its result. The guarded destructor records
  a sticky refusal in the same fence before returning from an error
  branch, including when the original error was Busy. Keep Busy,
  unsafe path and ordinary damaged spool handling intact. The native
  `poll_outputs` immediately returns the first read error before trying
  another stream or consulting worker status. Match the typed error
  explicitly in `Spool::read` alongside Busy and UnsafePath, and
  recheck the sticky state in `Engine::read_output` before returning.
  Native Poll handles the
  exact new engine error as an uncertain internal result, so the shared
  fatal worker gate publishes no ordinary result. Dispatch submission
  and input error recovery also return immediately on that error before
  their existing status reread. Treat the same error as Uncertain in
  the native error mapping and the remaining WriteInput reread and
  Cancel fallback arms. Map the otherwise unreachable variant to
  `storage_failure` in the prototype's exhaustive response mapping.
- `Engine::into_cleanup` has an explicit Store drop on success and
  implicit Store drops on UID or lock/proof failure. The cleanup
  guard holds file/job locks only. It contains no SQLCipher handle.
- Detached finite helpers call `Store::open_worker` and the writing
  `Spool::open` on their own processes. Migration, standalone engine
  frontends and stock native use the existing unguarded constructor.
  Ordinary native preflight (`native_preflight.rs:344`), the existing
  stream runner (`stream_runner.rs:839`) and copy launch
  (`copy_launch.rs:249`) stay stock and supply no candidate fence.
  The current call inventory has no other candidate engine opener.

## Fence and ownership

Add one opaque `marmish_engine::ConnectionFence` carrying a synchronous
non-panicking `Send + Sync` observer and an explicit
`Engine::open_with_master_fenced` entry point. The native candidate worker
passes a clone of its existing `command_engine::Admission` as the
observer on that entry point and retains the exact same original latch
and admission in its worker. Its second callback reports one sticky
unsafe owner outcome (an actual guarded close failed) through the exact
native registration owner's shared fatal signal, without running SQLite
or acquiring the registry mutex. The owner supplies that narrow
notification independently of the guarded registry observation. Native
`Admission::new(&Arc<Trip>, verify)` derives Fatal and a one-purpose
unsafe-close closure from the exact paired Trip. Add a narrow pairing
method on `Trip` that returns its Fatal observation and the closure;
Admission never retains or exposes a general Trip. Registration owns
the original Trip and supplies the checked observer from that same
lifetime, so the close notifier cannot be paired with a different
latch. Tests assert identity by notifying through the closure and
observing refusal via the original Admission and Fatal. No
thread-local fence, environment switch,
new async observer, or second default registry. Stock native and
standalone constructors cannot acquire the candidate authority through
ordinary options. The engine owns only checks, with no registry access,
connection repair, async task or relay authority. The fence has one
sticky local refusal bit shared by the state and spool custodians. The
registered observer trips atomically when a default check refuses, and
the close-failure callback separately trips on loss of safe connection
custody even with a still-default dispatcher. The future mount must
supply both outputs and the same latch.

Reuse stock `cipher::open` and the shared SQLCipher hardening routine.
Create a private guarded connection owner and carry the chosen fence from
the state store into the saved-prefix read. The owner dereferences to the
same rusqlite connection used by existing SQL helpers. No broad
`DerefMut` exposes ownership replacement. Use only a private
transaction method borrowing its held handle when a mutable connection
is necessary. Stock read/write
callers keep their journal modes, flags, error handling, SQLCipher key
derivation and settings. No SQL runtime dependency is added.

Candidate boundaries are:

1. Existing checked worker admission before engine storage initialization
   or helper work. Before state file preparation, check again so a new
   file preparation turn cannot start after the observed cut. For saved
   spool prefix access (which never creates a file), check before its
   preparation/inspection and again immediately before its actual open.
2. Under a checked initialization turn, use the existing SQLCipher
   once barrier. Before raw open require nonzero `sqlite3_threadsafe()`
   and the locked rusqlite runtime check using `sqlite3_mutex_alloc(0)`
   and `sqlite3_mutex_free`: refuse the single-thread sentinel 8 and
   null. Recheck the observer immediately before raw open. Use read-write,
   NOFOLLOW and extended result codes with an absolute private
   validated path containing no NUL, and a null VFS name to use the
   verified default. The guarded raw path uses FULL_MUTEX and requires
   non-null `sqlite3_db_mutex` on success before wrapping the handle.
   This makes the actual controlled handle confirm that core mutexes
   are effective and supports the safe Rust transfer. The locked
   5ae7fdf SQLCipher build defines SQLITE_THREADSAFE=1 and the bundled
   `openDatabase` applies FULL_MUTEX only with core mutexes enabled.
   The stock rusqlite route retains NO_MUTEX and its existing checks.
3. Preserve nullable raw-handle ownership around the call and across any
   failure, including SQLite returning an error alongside a non-null
   handle. Locked rusqlite 0.40.1 `InnerConnection::open_with_flags` closes
   these handles internally and may also close on timeout setup error.
   Therefore the candidate path calls the published `sqlite3_open_v2`
   C API under its own small private RAII raw slot. On success, transfer
   the sole handle into a guarded `rusqlite::Connection::from_handle_owned`
   before keying, timeout, pragmas or any schema access. Review the exact
   locked handoff to avoid hidden close or duplicate ownership. Neither
   a raw nor converted handle can escape the guarded owner. Use one
   shared refusal error that carries no path, SQL, key or raw details.
4. After guarded open and before timeout/key/SQLCipher validation, admit
   that synchronous configuration turn. Separately check before
   transaction/schema startup and startup reconciliation of each pending
   job. Existing outer worker admission covers ordinary synchronous
   operations that reuse the held connection. Nested opens have their
   own check. Cleanup lock/proof retries require a check at each retry.
   A single already-admitted synchronous operation may finish its
   SQL/rollback/finalization. Admit no subsequent independent turn;
   post-effect worker tests own suppression of stale results.
   Check each `reconcile_step` before the first `job_state` SQL,
   including `await_worker` retries and background maintenance. Return
   the refusal as `ReconcileFailure::Global` so neither startup nor
   background handling absorbs it as a per-job anomaly. Check the maintenance transitions after
   pending reconciliation, before each independent cleanup candidate and
   each orphan, including standalone orphan cleanup on submit failure.
   Gate issue-token and submit retries after maintenance and the new
   job-file/SQL turn after orphan sweeping. Check immediately before
   `reserve_worker_start`. Treat one reserve-and-spawn attempt and
   its necessary synchronous compensation as one bounded admitted
   turn. Preserve accepted durable evidence on refusal before a separate
   launch attempt, and let unrelated detached workers finish.
5. Every implicit/explicit candidate connection close, including
   failed open, failed key/config, failed validation, local drop,
   success and panic unwinding, takes the same exact check immediately
   before entering SQLite/Rust connection close. On refusal park the
   precise handle to process exit, with no SQLite global shutdown or
   second close. Use `ManuallyDrop` ownership before the callback.
   A close attempt admitted before the latch may finish. For converted
   candidate owners, a failed `Connection::close()` returns the complete
   still-owning Connection. Park that owner instead of a destructor
   retry, poison the same connection fence, and signal fatal via its
   narrow unsafe-owner notification. This applies to any actual close
   failure including SQLite Busy. Neither result retries or certifies
   cleanup. A raw holder has at most one
   checked `sqlite3_close` attempt, also including a half-open handle.
   Any failed raw close takes the same park, poison and notify path.
   The one shared sticky state prevents duplicate notifications.
   Borrowed statements/transactions finish normally within the already
   admitted turn before the owner close check. Actual OS exit is a
   separate lifecycle boundary that retains the profile lease. Dropping
   ordinary local non-SQL file handles on partial failure does not run
   the SQLite close path.

Check candidate `Engine::into_cleanup` admission at entry and each
independent retry in `cleanup::acquire`. Consume the store via an explicit
checked close and return a guard only after actual successful close.
On ordinary acquisition failure, the default guarded drop still applies.
On refusal while consuming the engine, park the exact Engine, including
its frontend guard, if it still exists. Once Store is consumed, park the
frontend and successfully obtained job guards on checked close refusal
or poison. Partially acquired job locks from a refused acquisition may
be released because the retained frontend guard excludes other
frontends and cleanup, and detached workers must stay independently
recoverable. The candidate owner still holds its original profile lease
through process exit. Released partial job locks grant no new runtime
cleanup authority.
Use this safe engine path for healthy candidate `Control::Cleanup`, keeping
the previous closed behavior on error and suppressing the result after the
fatal latch. Existing Worker parking of the whole live Engine or held
CleanupGuard remains decisive when fatal is observed by the existing
worker/keeper path. This increment never reports runtime file cleanup on
the fatal path and still has no mounted candidate caller. Existing
independent detached job durability remains unchanged.

Before the daemon mount, its registration observer must atomically latch
shared fatal under the matching registry guard on any version/default
displacement or indeterminate observation. Neither this engine adapter
nor an isolated SQLCipher test proves that registration observer,
profile lease/daemon shutdown or non-Command owners are wired.

## Focused validation

Use disposable state, real SQLCipher files and deterministic synchronous
cuts. Keep tests measuring C open/close admissions near the private
boundary. The fixed observer and narrowly scoped test instrumentation
must preserve the exact production call path. Execute intentionally
parked real handles in short disposable child processes.

- Initial refusal skips state preparation and the SQL opener; a cut
  between an admitted preparation and raw open leaves the exact prepared
  file bytes and no newly opened handle.
- Force a raw open producing an error and non-null handle, and prove no
  C close after refusal. Prove the healthy error branch closes that exact
  handle. Pin an actual cut after raw success, before keying; then cut
  after ownership conversion during key/configuration or startup.
  Observe no new startup SQL turn, no guarded close and no installation
  of an engine on a denied stage. Deny at the first startup
  reconciliation with a real persisted pending job and prove its state,
  start attempts and worker launch count are unchanged.
- Exercise an actual admitted close failure using a deliberately live
  raw prepared statement in an isolated child. Prove the actual Busy
  return parks the exact connection, that repeated checks are sticky
  and none retry close, and that native shared fatal notification fires
  at most once. Trigger a failed raw close in the private C owner
  test with a real SQLite handle and outstanding raw statement; the
  same poison rule applies to partial handles. Verify that the exact
  original native Admission refuses after its own notifier runs.
  Exercise healthy candidate cleanup and the consuming failure cut on a
  real keyed state database. A false close must refuse the CleanupGuard,
  leave the SQLite handle parked, and skip the native worker's Quiesced
  publication. Keep the prepared-state/root footprint and original
  profile lease semantics documented.
- Exercise a healthy saved-prefix open and an admission cut before
  raw open plus a cut after read just before close. Verify exact bytes
  when healthy, typed refusal rather than synthesized missing bytes and
  no post-cut close. Exercise the error-path destructor while ordinary
  corruption or Busy is pending, and prove refusal wins both. A native
  Poll cut yields no completion and never tries a second stream.
  Exercise one submit or input error through the real worker
  post-execute gate as well. Pin empty retained data opens no connection
  and the existing missing/corrupt spool semantics remain valid.
- Re-run engine cipher, storage, spool and focused Command/lifecycle
  regressions, candidate no-link compile, strict targeted Clippy and
  formatting, with a focused immutable artifact when feasible. Real
  default displacement and installed daemon eventual join remain
  separate integration evidence. Scope review must check all candidate
  storage and temporary handle close paths against the locked rusqlite
  ownership contract before accepting code.
