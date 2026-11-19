# Observe the installed receiver's clean copy charge

The installed near-cap qualification measures a genuine recovered Pending stage
and records refresh latency. The receiver makes an encrypted copy of the exact
original on its existing 500 ms scheduled native read. Observe that copy with
test-only metadata reads instead of changing the daemon, storage or debug schema.

After both latency cohorts, keep the same installed process and exact Pending
profile running. The controller observes the next normal scheduled native
Pending refresh. It sends no request to trigger a copy or change custody, and
does not poll metadata during either latency cohort. This measures the already
warm stage and spends at most three seconds of the existing fixture deadline.
There is one bounded observation window, without retries or extension. Missing
stable evidence fails qualification. Report the filesystem type.

Pin the private peer directory with a nofollow directory file descriptor,
require the same directory device and inode at the beginning and end, and
enumerate via that descriptor. Read file metadata with `fstatat` and
`AT_SYMLINK_NOFOLLOW`. Require exactly the fixed cursor, Known/Unknown original
and snapshot names for qualification. Specifically, the cursor must be present,
with the Known original and all sidecars absent. The healthy installed daemon
may handle other states; this fixture qualifies only that narrow state. Require
regular single-link files with distinct device/inode pairs on the local volume.

Take original and snapshot metadata before and after enumeration to reject
disappearance, replacement or growth during a sample. Accept only two complete
matching observations spaced by 2 ms, within the single three-second window.
Require the original to match the exact offline cursor witness's device, inode,
logical length and allocation, and the snapshot to have the same logical length
with a distinct inode. A mismatch fails observation. The controller never opens
the snapshot data.

Report the observed logical and allocated extents and the aggregate at those
sampling instants, with no peak claim or claim that the daemon observed the same
allocation. Match `Inventory::calculate` using `max(logical, allocated)` for
each family and the installed cursor reservation from `RESERVED_BYTES`. The
snapshot logical extent must be positive and at most `CLOSED_STAGE_LIMIT`.
Aggregate charged bytes must be strictly below `TOTAL_ADMISSION`. Independently
the installed native classifier and its own physical admission checks establish
the real read and completed publication.

Capture only bounded numeric observations. The ordinary installed proof and
core CI are unaffected.
