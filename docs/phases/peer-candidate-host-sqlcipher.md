# Guard the daemon's native SQLCipher connections

This slice of the [dispatcher shutdown design](peer-candidate-dispatcher-shutdown.md)
covers host-owned rusqlite at the candidate's native state compatibility check
and application Journal. The base is `c51e92b` and work remains under Bead
`marmish-dwt.4.12.2`. This slice can be qualified before registering a daemon
or starting an actor. It enables no runtime candidate path.

## Shared connection ownership

Reuse the Engine's checked raw and converted owner in
`marmish-engine/src/cipher_fence.rs`. Add a narrow native-callable owned
connection for a pre-existing local file, with only fixed read-only or
read-write modes. Both guarded modes use FULL_MUTEX, NOFOLLOW and extended
result codes. Keep the existing verified raw opening, effective mutex check,
partial handle ownership and one checked close. Leave the current Engine
opening flags and ability to map Busy unchanged. Preserve the stock
rusqlite path and caller's file preparation. The helper does not create files,
key, migrate, execute statements by itself or take a route argument.

Host users may borrow a shared connection for an already admitted synchronous
body. Expose no mutable reference, unchecked ownership extraction or raw
transfer. Provide the existing transaction mechanism for the serialized Journal,
preserving its original deferred transaction semantics. If a transaction or
statement has begun in one admitted body, it must complete or unwind within
that body before returning to the async loop. No storage borrow crosses an
await. The connection owner rechecks its fence before explicit and implicit
close. Close refusal parks the handle. A true close failure parks the handle
and reports terminal custody through the exact original callback at most
once. Do not retry close through a destructor.

`Admission::from_registered` remains the sole production source in Marmish.
Factor the existing conversion to an Engine connection fence through that
admission and keep the same captured fatal identity, bounded verifier and
custody callback. Any copies delivered to the frontend, compatibility checks,
Registry and worker originate from that registration. The registration
verifier is not called with an owner connection or registry guard held.
Engine and daemon checks stay independent of native body execution.

## Ordinary compatibility and Journal integration

The existing compatibility scanner does three local opens. Retain each
file's private-path and absent/empty rules. In the guarded entry recheck before
each of the three independent checks, including the first SQLite initialize,
and put the returned handle in the shared owner before keying. The readonly
connections never enter migrations. Finish their scoped SQL and explicitly
close each checked connection before trying the next route, preserving errors
from the compatibility validation when the dispatcher is still sound. Return a
typed candidate storage refusal if the exact latch is cut. Preserve the stock
legacy caller and flags.

Keep the Journal's existing private-file creation, metadata validation,
binding, secret handling, SQLCipher PRAGMAs, schema and transaction order.
In the guarded entry check before creating or opening its backing file.
After checked construction and custody, its synchronous initial binding and
migration may finish as one admitted body. Recheck before returning the
Journal for future use. Do not allow an earlier Journal open or its successful
return to stand for the next open. The ordinary path currently proves
close then reopen. Give the later daemon composition a checked explicit close
for that first instance, followed by an independent opener. All later Journal
transaction callers retain the same behaviour under the stock wrapper.
This slice has no live daemon call site. The composition must check after
every async boundary before admitting another durable storage turn, and must
not return a successful reply on a tripped fence.

A candidate MDK/MLS account store uses a separate upstream
`SqliteAccountStorage` and is outside this increment. Locked upstream exposes
close and phase observation after managed opening, and supplies neither
an injectable pre-existing connection nor a callback that can refuse
a phase. The transport and Terminal Registry/MLS lanes are separately
reviewed. Verify those boundaries before installing a registered daemon.
In particular the upstream explicit checked closer currently retries a failed
rusqlite close via drop. The Journal helper does not authorize that path.

## Qualification

Test real SQLCipher backing files with a refused initial open, between open
and key initialization, after one successful compatibility read before the
next database, and at Journal explicit close. Use fresh processes for actual
default VFS displacement. Reuse native Engine partial-handle and failed
close callback tests to cover the shared primitive. Cover committed encrypted
Journal data, real transaction rollback and verified cold reopen on the
normal path, and establish that refusal cannot lead to later SQL or a
second close. Verify no unchecked externally callable mutable or consuming
connection capability. Run focused compatibility, Journal and Engine tests,
stock and candidate strict Clippy, and record the exact Claude Auto design and
code reviews. Network and installed qualification remain with daemon mounting.
