# Managed recipient rollback code review

Claude Code Sonnet 5.5 in Auto permission mode reviewed exact source
`760933cd477a769115c3d3b3fe71b6cc33f885fc` against the approved R3
design `ebaf3987e3672d8808189dc82b0d1ccaacd741ba`. Session
`6cc6f6f0-2689-4d8e-95e1-c375f6759bdf` approved the isolated Linux
AMD64 increment with zero blocking findings. The reviewer ran the
preserved managed rollback test directly in 1.91 seconds. The author's
separate run took 2.47 seconds.
The review also examined the author's scoped regression and Clippy
results. [Design](../phases/peer-receiver-charge-scope.md) · [Validation](../validation/peer-receiver-charge-recovery.md)

The review checked the single leased cursor, exact durable reservation,
inode checks, independent file limits, stock callback delegation,
fixed header and trailer, and the observed main write and sync required
before exact journal deletion. It identified two actionable follow-up
items: make the thread-local probe in `xAccess` robust to teardown and
strengthen the wrong-key test beyond refusal and the retained main
charge. SQLite checks the journal record contents. The current fixture
uses the reviewed private-directory boundary and demonstrates a
process interruption. Directory persistence under power loss needs
separate evidence.

Native guard coverage for wrong selection or inode, changed extents,
per-file and combined growth, sidecars and access modes, and a full
pager attempt with invalid trailer or oversized journal remains a
qualification requirement. The increment's review covers the test-only
seam. Installed owner and macOS qualification remain open.

## Integrated review

Claude Code Sonnet 5.5 in Auto permission mode session
`dc1f51da-4118-4749-b00f-f7be6eec3d0e` approved source
`6a8846ec2a6f3b363ec9b7cdf8f90faf15fbe188` on `091c448`
and requested corrections to the validation record.
The added and removed lines are identical across the seven source files.
The reviewer checked the surrounding Founder and ReceiverQuota paths,
mutual exclusion, lease ownership, and stock SQLite delegation against
the composed test and Clippy results. The reviewer approved the corrected
composed validation record on 2026-11-18.

The shared test-only VFS gains conditional recovery paths. The reviewer
carried forward the thread-local teardown guard in `xAccess`, additional
physical charge assertions, and broader refusal checks for the next
increment. The review covers Linux AMD64 library tests. macOS and a
shipped recipient path still require qualification.

## Measured guard matrix

Claude Code Sonnet 5.5 in Auto permission mode approved exact source
`990c8ca03b3cf2f6c7ba6c966ea2bded506bb565`, session
`18ad854c-35a5-40b1-a910-bdaf83aad396`, with no blocking findings.
The reviewer ran the preserved Linux AMD64 executable using the
author's build provenance and ran the expanded SQLCipher test in
3.22 seconds, the four ordinary inventory cases, and two ordinary
quota cases. The reviewer inspected, without cross-compiling, that
other targets retain the stock access callback.

The matrix fixes the earlier thread-local teardown concern in
`xAccess` and demonstrates separate 16 MiB main and combined logical
limits. The main and journal image bytes remain unchanged across the
seven guard subprocesses. The oversized and super-journal markers
are refused before playback. The earlier wrong-key result still
demonstrates refusal and retained main charge. The pager can write
before that refusal.

A genuine stock filesystem fallback from requested read-write to
read-only, the allowed absent WAL existence probe, and the other
managed open flags were reviewed by inspection only. The tests do
exercise an explicit read-only main request, a foreign access name,
and physical WAL/SHM fencing. Mutation tests, power-loss ordering,
macOS and installed owner validation remain outside this result.

Claude Code Sonnet in Auto permission mode also approved the integrated
source `515557a61235f81debdb3deb6d52bdb60a6cafee`, session
`986e2654-06c3-448f-ad13-ac94afe0384b`. It verified the same source change
over `f5d99ff` with both adjacent Founder and receiver test module
registrations retained. The review inspected callback scope and ordinary
SQLite delegation. The composed tests ran separately.
