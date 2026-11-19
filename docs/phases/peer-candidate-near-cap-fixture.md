# Near-cap SQLCipher stage for installed Manual Pending timing

This test-only fixture prepares the input permitted by the
[Manual actor mount timing gate](peer-candidate-manual-actor-mount.md).
The installed controller first obtains Pending from a real signed Offer,
Consent, and Delivery and the recipient's native Welcome ingest. It grows
that same SQLite inode while the candidate daemon is stopped. The workload
measures hashing, copying, and SQLite read work on a database dominated by
allocated freelist pages. It does not represent 15 MiB of live MLS records.

## Ownership and exact stage

The controller and helper live under `peer_invitation` in the Linux AMD64
libtest crate. After exact List and Show, the controller stops and reaps the
installed child, acquires its disposable profile's ready `ProfileLease`, and
loads the same identity material. Call `near_cap_fixture::grow` with that
lease, material, exact canonical Peer URL, and observed `peer_read::Entry`.
There is no product flag, arbitrary pathname, caller-supplied key, generic
cursor dispatch, newly minted source, or route authority.

The exact controller selector is a fresh, unmanaged process. It must never
register or bind a receiver quota. All actual receiver quotas are owned by
the separate installed children. The helper checks for the VFS stock `unix`
name and matching default pointer before either database opens, using the
same `Dispatch::checked_stock` verification as installation. It refuses a
registered dispatcher, even if an arbitrary path might forward. It never
names a VFS explicitly or bypasses it. Offline tests using the process-wide
VFS run in an isolated exact selector.

`Paths::new` and `Paths::cursor` pin the lease, account, same checked existing
cursor pathname/inode and absence of sidecars. Key the cursor read-only
(`READ_ONLY | NOFOLLOW | NO_MUTEX`, no CREATE) with its derived cursor key
and existing SQLCipher hardening. Check its closed digest and identity before
opening and after close. The checkpoint accessor uses a deferred read-only
transaction, `Epoch::verify` for the **existing** installed custody schema,
and `stage_journal::load` for the exact Unknown binding. Require one Pending
row, Manual mode, exact account/canonical origin/founder/offer/revision/
generation/held time/deadline matching the observed Entry, no terminalized
row, and `State::Prepared` with a real nonzero `FileWitness`. Never call
`attach_installed_custody` or bind even a fixture Source. A mismatched layout
has no accessor and is never initialized or upgraded.

Use `Paths::reopen_provisional(binding, witness)` to pin the actual stage.
Require same inode, sidecar absence, owner, `0600` and one hard link. Refuse
an existing image of 4 MiB or more, a freelist over 64 pages, or a stage with
full/disabled auto-vacuum. The controller retains Entry and the returned
extent/digest witness. It must dispose of the private profile following a
failed grow, including a process or I/O fault during the DROP transaction.

## SQLite growth and exact checks

Open the existing same stage image read/write, no CREATE, NOFOLLOW,
NO_MUTEX, zero busy timeout, and the derived `Paths::provisional_key` via
locked MDK `open_hardened_sqlcipher` with `live_cache()`. Verify existing
DELETE mode, `secure_delete=ON`, `auto_vacuum=INCREMENTAL`, FULL synchronous
and SQLCipher enabled. Disable cache spilling for these two private
transactions. Require at least 64 MiB available in the checked private
volume before touching the image. No file truncation, raw page writes or
VACUUM are used by the grow helper.

Read and retain the exact permanent `sqlite_schema` rows, including
rootpage, and `user_version` and `application_id`. Read `page_size`,
`page_count`, and `freelist_count`. Use the locked SQLCipher implementation's
`PRAGMA cipher_integrity_check` requiring a recognized result column and zero error rows and
`PRAGMA quick_check` requiring exactly one `ok` row before and after. The
bundled source implements `cipher_integrity_check` explicitly. Do not open
the original through the mutating MDK account initializer after growing it.
The original MLS data takes no application-layer writes. No key, digest,
SQLCipher error text, SQL results or path is published.

Set `max_page_count = floor((16 MiB - 1)/page_size)` and verify the effective
value. Aim to reach 15.5 MiB in actual page count with at most 256 inserts
of 65,536 random bytes per insert, checking the target after each insert.
Allow at most one insert of overshoot within the hard cap. About 16 MiB of
dirty pages may be held in memory with cache spill disabled. Create one fixed-name permanent table
with one BLOB column and no index, AUTOINCREMENT or trigger, then insert
SQLite `randomblob` rows within one transaction. Commit allocation before
starting a second transaction that drops the exact table. The two durable
steps ensure SQLCipher writes the random content before secure deletion
frees its pages. With incremental auto-vacuum, DROP returns pages to the
freelist without truncating the main image or relocating preexisting roots.
Keep a 32 MiB budget for the DELETE journal, allowing the room needed to
securely zero pages during DROP as well as page records and journal headers.
With cache spill off and no other writers, a transaction journals each
preexisting page at most once and only the bounded main database can be
journaled. A pre-growth commit SQLITE_FULL rolls back the first transaction
and the offline test verifies the original closed digest. A failure after
allocation commit, including any hot journal, invalidates the fixture.

Before returning, require scratch table absence, equal permanent schema and
header versions, equal page size, retained page count after DROP, and an
increased freelist covering the grown bytes. Close the connection, fsync
the existing stage fd, reopen via the exact `FileWitness`, then measure inode
and mode, sidecar absence, `page_count * page_size == logical`,
`allocated = st_blocks * 512`, and `allocated >= logical >= 15 MiB` with
`logical < 16 MiB`. An unsupported sparse or compressed allocation fails
qualification. Persist nothing in the cursor. The returned witness gives
numeric extents and `assert_unchanged` for inspection after the installed
run has stopped with the same ready lease again acquired.

## Evidence and test boundaries

The isolated offline positive extends the existing in-process two-account
native Welcome fixture without any relay traffic. Grow its actual joined
stage with the same native key, verify closed physical extents, page and
freelist relationship, shape fingerprint, inode and no sidecars, then use
its existing native joined readback on a separate checked copy to compare
exact group/epoch/own leaf and receipt again. Check existing inventory on the
real installed scope (or in its controller) classifies the prepared inode
as `Identified` with charge under 128 MiB. The installed cursor offline read
must show no changed digest or identity. Run fast negatives for wrong key,
wrong inode, a sidecar and substituted file (symlink, extra hard link, wrong
mode), existing grown image, an exact 16 MiB image, wrong/missing custody
selection or absent Prepared state, a too-low max page count with digest
rollback, and a bound receiver VFS. Assert no unrequested write and fixed
failure category. A corruption control on a disposable copy can demonstrate
the cipher HMAC check sees stored page corruption if needed.

The separate controller phase must reopen the same profile in a fresh actual
candidate process and return the same Pending Entry from List and Show after
real charged byte-verified native snapshot readback. It must measure actual
original and copy logical/allocated bytes, before/after native read of the
copy, actual aggregate (including the fixed 49 MiB cursor reserve and any
other family) strictly below 128 MiB, and the reviewed warm refresh/read
samples. After stop, the exact original digest/inode and unchanged offline
cursor verification are checked in context. Record filesystem type if
available and report any failure to meet the reviewed measurements. The
helper cannot stand in for installed native validation, performance results,
or public availability.

Claude Code Auto approved design `650556c` in session
`675cf22b-e02d-43cc-a971-92aacbf9423b`. Its nonblocking findings refined
the integrity check, headroom, and per-row stop condition above.
