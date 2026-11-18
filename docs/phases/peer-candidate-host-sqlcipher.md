# Guard the daemon's native SQLCipher connections

This slice of the [dispatcher shutdown design](peer-candidate-dispatcher-shutdown.md)
covers host-owned rusqlite at the candidate's native state compatibility check
and application Journal. The base is `c51e92b` and work remains under Bead
`marmish-dwt.4.12.2`. This slice can be qualified before registering a daemon
or starting an actor. It enables no runtime candidate path.

## Shared connection ownership

Reuse the Engine's checked raw and converted owner in
`marmish-engine/src/cipher_fence.rs`. Export a narrow non-Clone
`ExistingConnection` owning that existing private owner, and
`ExistingAccess::{ReadOnly, ReadWrite}`.
`ExistingConnection::open(path, &ConnectionFence, ExistingAccess)` uses
FULL_MUTEX, NOFOLLOW and extended result codes, with no CREATE.
`ExistingConnection::stock(Connection)` wraps today's stock flags and
destructor unchanged. Keep the Engine's existing guarded read-write access
and error mapping. The host still performs file preparation and keying.

`ExistingConnection` provides `Deref<Target = Connection>`,
`transaction(&mut self)` and `close(self)`. Transactions are Deferred,
as in today's Journal. The Engine's Immediate method stays independent.
Expose no `DerefMut`, Clone, `into_inner` or unchecked ownership transfer.
An unsafe rusqlite `handle()` remains available through its existing
shared reference contract for key application. No statement or transaction
from the synchronous Journal crosses an await. On explicit and implicit
close, recheck the exact fence. Refusal parks the handle. Actual failure
parks it and invokes the exact original custody callback at most once.
Do not retry close through a destructor.

Provide `ConnectionFence::admit(&self) -> marmish_engine::Result<()>`
for bodies outside the wrapper, including pre-open file metadata and
creation. `Admission::from_registered` remains the sole production
source in Marmish. Factor the existing Engine conversion through that
admission and keep the captured fatal identity, verifier and custody
callback. Copies delivered to compatibility checks, Journal, Terminal
Registry and worker originate from the same registration. Admission
callbacks run outside registry and SQLite locks. A guard refuses with
`Error::Storage("dispatcher_admission")`, which the candidate caller
treats as fatal and never an ordinary retry or untouched stock outcome.
Other existing storage errors retain their classifications.

## Ordinary compatibility and Journal integration

Keep the existing `native_preflight::check(profile, material)` and
`Journal::open(path, key, account)` as thin wrappers using None. Each
delegates to one shared implementation receiving
`Option<&ConnectionFence>`, called with Some only by the guarded path.
Leave the stock open flags and Journal transaction call sites untouched.

The compatibility scanner performs three independent opens. Recheck
admission before each check and initial native open, preserving the
private-path and absent/empty treatment. Close each read-only connection
before opening the next. Within a guarded check, a refused or failed
close wins over a validation error, and a validation error wins over
successful close. Keep original stock behavior and note locked MDK
`bdf9833` at the unchanged migration bound 214.

Give Journal one `ExistingConnection` field for both modes. Preserve
private-file preparation and checks, secret handling, PRAGMAs, migration
and binding. In the guarded case check before file creation. Acquire
guarded custody before keying and check after initialization before
returning a Journal. Its synchronous binding/migration can finish as
one admitted body. Provide a consuming checked Journal close for the
daemon's first close/reopen proof. The reopen is an independent admission.
A refusal beats an error from the admitted setup body. The mount owns
checks before subsequent async turns and externally visible results.

This slice has no live daemon call site. Candidate mounting depends on a
separately reviewed upstream MDK account open/close ownership path,
transport and Terminal Registry and room lanes. The locked
`SqliteAccountStorage` managed opener has no injectable connection or
callback that can refuse a phase. The explicit checked closer currently
retries a failed rusqlite close via drop. The Journal helper grants no
authority to that path.

## Qualification

Test real SQLCipher backing files with a refused initial open, between open
and key initialization, after one successful compatibility read before the
next database, and at Journal explicit close. Prove a refused read-only close prevents the next compatibility open. Use fresh processes for actual
default VFS displacement. Reuse native Engine partial-handle and failed
close callback tests to cover the shared primitive. Cover committed encrypted
Journal data, real transaction rollback and verified cold reopen on the
normal path, and establish that refusal cannot lead to later SQL or a
second close. Verify no unchecked externally callable mutable or consuming
connection capability. Run focused compatibility, Journal and Engine tests,
stock and candidate strict Clippy, and record the exact Claude Auto design and
code reviews. Network and installed qualification remain with daemon mounting.
