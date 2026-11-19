# Near-cap SQLCipher stage for installed Manual Pending timing

This test-only addition implements the input preparation permitted by the
[Manual actor mount timing gate](peer-candidate-manual-actor-mount.md).
The ordinary installed qualification creates real Pending from an independent
founder's signed Offer, Consent, and Delivery and the recipient's real native
Welcome ingest. This fixture changes the extent of that exact stage while the
daemon is stopped. It measures size-sensitive hashing, copying and SQLite
open/read work on a valid database dominated by allocated freelist pages. It
makes no claim about 15 MiB of live MLS records or fragmentation in deployment.

## Offline fixture interface

The controller calls the fixture once after it has verified one exact Pending
including a successful Show and has stopped and reaped the installed daemon.
It uses a fresh, process-private, disposable profile directory created by the
test. While holding the acquired ready `ProfileLease`, obtain the same File
identity material with existing helpers and derive `Paths` from that lease.
Read the existing checked receiver cursor under that lease and require the
selected Unknown binding, Pending selection (founder, offer ID, revision and
nonzero generation) and exact Prepared `FileWitness`. Use existing evidence
readback where possible. A fixture-only read accessor may use the actual
checkpoint's exact row/charge validation, with no independent SQL write,
new grant, marker, port or eligibility decision. The offline accessor has
no route authority. No fresh stage allocation, cursor upgrade or rebinding
is permitted. Do not take an arbitrary stage pathname or caller-supplied key.

`Paths::reopen_provisional(binding, witness)` checks the inode, spelling,
sidecar absence and existing closed cap. Begin with the ordinary small image.
Check the on-disk inode ownership, private mode and single link. The relevant
stage must be named by the same inventory before and after preparation.
The parent retains the exact IPC selection and metadata alongside the
fixture's numeric extent evidence for the cold installed check. If preparation
fails, dispose of the fixture profile. Never restart it to obtain a passing
sample from a partially prepared image.

## Bound and validate actual pager allocation

Open the preexisting same inode with the bundled rusqlite and MDK
`open_hardened_sqlcipher`, the original `SqlCipherKey::raw` from
`Paths::provisional_key`, NOFOLLOW and no CREATE, DELETE journaling, zero busy
timeout and secure deletion. Check SQLCipher is present and that the native
page size and `auto_vacuum=INCREMENTAL` are already set. Use the exact locked
MDK account initialization settings. A schema-aware integrity check may
need existing MDK search functions. If a pragma is unsupported or fails,
return a qualification failure instead of treating an empty result as an
arbitrary successful check.

Read the original full permanent `sqlite_schema` fingerprint, user and
application version, and key native records' logical fingerprint as feasible
from vetted proof methods. Permanent definitions and rows must be preserved.
Allow the schema cookie and SQLite header/bytes to change through normal SQLite
DDL. Key, record data, paths and raw fingerprints are never reported. The
fixture issues only fixed SQL addressing its dedicated table and pragmas, reads
schema/integrity evidence, and never updates native tables.

Apply a transaction with a uniquely named permanent fixture table and a bounded
number of fixed-size rows using SQLite `randomblob`. These disposable contents
contain no test or product secrets. Commit before dropping the table in a
separate transaction so the pager actually writes cryptographic page records
before freeing them. Use a page-count target comfortably between 15 and 16 MiB,
a connection `max_page_count` strictly below 16 MiB, small bounded insertion
chunks (e.g. up to 64 KiB), a fixed iteration limit and checked arithmetic.
There is a fixed maximum offline work budget. Leave the allocated pages on
SQLite's freelist. Do not set a file length, write raw pages, rely on a sparse
allocation or run VACUUM. If any stage becomes dirty or exceeds the cap, there
is no valid sample. Bound the transient DELETE journal separately by the single
private operation and check sidecar absence when closed. No SQLCipher
connection remains live or inherited across the installed restart.

Compare the exact permanent schema and SQLite header versions after drop, and
require the fixture table absent, the native integrity check valid, and an
increase in freelist pages. Check ordinary SQLite structural integrity and
the physical binary through the qualified native reopen. Capture page count,
freelist count and page size. Require the final file length to equal actual
page count times page size, final allocation (`st_blocks * 512`) to cover the
logical limit floor as well, and final length in [15 MiB, 16 MiB). Reopen the
specific `FileWitness` before reporting. Check no -journal/-wal/-shm or copy
remains. Take the actual closed file digest as a fixture-local fingerprint for
post-run unchanged verification, never as a substitute for native semantic
validation.

## Proof and ownership

Keep the grow helper and private tests behind `cfg(test)` with no public CLI,
runtime flag or installed path. Validate provisioned SQLCipher page growth
and cleanup locally, original permanent schema/rows, actual physical
allocation and refusal of bad inode/key or wrong state without network. Test
a disposable real native joined stage where available. The installed
controller separately calls the helper, then starts another actual candidate
process with the same profile. It must return exactly the same Pending
List/Show metadata through the real listener, after the real independently
metered and byte-verified native snapshot of the enlarged original. That
unmodified path checks signed custodies, persisted exact Welcome, sole native
group, current protocol, purpose, current epoch/authenticator, exact two
leaves, routing and consumed KeyPackage. An unchanged original digest is
asserted after that process stops. Capture actual original/copy logical and
allocated extents and actual aggregate accounting, strictly below 128 MiB
including the fixed cursor reserve and any other extent. Use existing
`CleanFiles` accounting and bounded opt-in timing evidence for the measured
attempts. Absence of a measurement or genuine near-cap repro keeps the timing
gate open. The offline leaf's own test cannot establish installed latency.
