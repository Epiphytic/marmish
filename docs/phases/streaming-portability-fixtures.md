# Streaming candidate portability corrections

Beads `marmish-8ka.5.2` and `marmish-8ka.5.3`; source baseline
`ad9cf27e184214d826ea1876b3c0964b69448739`. This bounded increment corrects
failures observed in candidate run 42328683611. It does not waive the installed
qualification gates or change release assets. Independent Claude design and
code review are required before implementation and publication respectively.

## Observed failures and proposed changes

1. Linux ARM64 engine test
   `owned_context_is_read_only_cloexec_above_three_and_has_no_writer` received
   WouldBlock after reading its complete context. Another test thread can fork
   while the short-lived CLOEXEC writer exists; the forked child temporarily
   retains that writer until exec. Run the unchanged immediate-EOF assertion in
   an exact-test subprocess, avoiding unrelated parallel forks. Reuse a bounded
   subprocess helper for this test and the existing closed-stdio test: exact
   filter, explicit --test-threads=1, 15-second deadline, owned kill and wait on
   timeout. Require a distinctive child exit code after all assertions, rather
   than accepting libtest's exit 0 for zero matching tests. A negative test must
   show an unmatched name is refused. Do not serialize the whole engine suite
   or change production descriptor ownership. The related production reader
   race needs the explicit correction below, not a test-only dismissal.
2. Thirteen macOS local-copy journal tests failed at the shared fixture's
   `Journal::open`, before their SQL setup. The fixture uses an aliased temporary
   parent, correctly refused by SQLCipher's SQLITE_OPEN_NOFOLLOW. Reuse existing
   `journal::private_test_directory()`; preserve all production no-follow and
   leaf checks. Existing canonical-parent/symlink-refusal coverage must pass.
3. The filesystem special-name test committed to a raw non-UTF-8 destination
   and Darwin returned EILSEQ. Use a portable UTF-8 special-character path for
   its cross-platform assertions, retaining a Linux-only raw-byte-name commit
   assertion. Keep leading dash, quotes, newline, glob and shell metacharacter
   coverage. Do not convert or sanitize user paths in production or claim
   arbitrary-byte filename support on filesystems that reject those names.
4. The preparation-interrupt test compared every F_GETFL bit before and after
   diagnostic output. Darwin adds kernel-owned FWASWRITTEN (0x00010000) after
   writing; F_SETFL does not restore that observation. On Darwin compare access
   mode plus the public settable flags O_APPEND, O_ASYNC, O_SYNC, O_DSYNC and
   O_NONBLOCK; check both F_GETFL calls succeeded. Retain exact comparison on
   Linux. Keep the no-Open and interrupted-before-Open assertions. Production
   Nonblocking guard behavior is unchanged. Contract evidence: Apple's XNU
   `bsd/sys/fcntl.h`, FWASWRITTEN and FCNTLFLAGS definitions:
   https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h.

5. The macOS post-receipt actual-helper cancellation case timed out awaiting its
   Cancel acknowledgement. Source inspection finds that the test Bridge handles
   controls only between requests, so a pending request/reply can prevent Cancel.
   The log does not identify the exact blocked await. Give Cancel a dedicated
   bounded channel (capacity one), separate from Gate controls. Scope the whole
   accepted request's reads, pipe processing and reply write in one future;
   select its completion against Cancel and owned stop. On Cancel, first drop
   the in-flight future/socket to release borrows, then signal the same owned
   child in common outer code. Preserve stdin until the existing signal-driven
   wait/cleanup; never use EOF as a substitute for cancellation. Do not require
   polling the paused client operation, enqueue Cancel twice, or enlarge the
   timeout. Gate remains on its current lane while its caller polls the client.
   Deterministic regression: when the existing post-receipt mode observes the
   real durable Commit and applies its gate, set a distinct `parked` metric and
   park the request before writing its IPC reply. The case must wait for this
   metric, not the earlier `gated` observation. Establish red/green evidence:
   baseline plus only the parking hook fails with cancel_ack_timeout, then the
   control-lane fix passes. Bound the parked hook and select it against owned
   stop even in the red run, preserving finite cleanup. Update the closed-control
   regression's Bridge struct literal for the dedicated lane. No MutexGuard may
   survive across request I/O awaits. Both Put and Get must acknowledge Cancel,
   confirm helper exit 1, produce no protocol Error/stderr, retain exact committed
   bytes/metadata and recover the receipt via a fresh Resume without Data.
   Stop must also interrupt a parked reply for bounded failure cleanup. Existing
   closed-control failure and all eight cancellation cases must remain covered.

6. The same transient writer inheritance can make the production helper's
   nonblocking context read fail on WouldBlock after valid buffered bytes.
   Replace the single immediate read-to-EOF with a nonblocking read/poll loop
   under one absolute monotonic **one-second** deadline, not a resettable idle
   timeout. Normal prewritten, closed contexts still finish immediately. Retry
   WouldBlock and Interrupted within that deadline, using bounded remaining-time
   poll waits, and retain bytes already read. EOF remains mandatory: do not
   accept a decoded prefix while a writer remains open. Reject more than 4096
   bytes immediately, allocating no more than the existing 4097-byte sentinel
   bound. Preserve FD >3, FIFO, read-only, exact decode/trailing-byte and compiled
   build checks, descriptor ownership, finite startup diagnostics and exit 1.
   A permanently held writer is refused after the fixed ceiling. An unrelated
   child that retains a writer longer than that ceiling can still cause safe
   refusal; this is not a guarantee under arbitrary scheduler delays.
   Preserve existing internal classifications: deadline expiry is an I/O timeout
   mapped to LocalIo (as existing held-writer/partial-prefill tests assert), while
   EOF with malformed/partial/trailing bytes and overflow remain Protocol.
   Both emit the existing invalid-context startup diagnostic, never Build.
   Deterministic tests must demonstrate baseline failure then corrected success
   when a duplicated writer closes after a short delay with complete valid bytes
   already buffered. Also cover never-closed, empty and partial contexts,
   malformed/trailing data, appended data before delayed EOF, and oversize data;
   no decoded prefix may bypass EOF. A private readiness-wait seam may let tests
   observe the first WouldBlock before instructing a writer thread to close or
   append; production supplies the real bounded poll implementation. The tests
   call that same poll after observing readiness, with no global/environment
   hook. This avoids a sleep-only race where the writer closes before the reader
   even runs. Reuse existing context fixtures, check the one-second startup wait
   against caller deadlines, and keep finite cleanup/join of writer threads.
   This is a production behavior
   amendment with its own Beads bug and independent review.

## Trust, persistence and recovery boundaries

Except for the separately reviewed bounded context-reader correction, changes
are confined to cfg(test) fixtures. No remote input, copy protocol,
authorization, descriptor inheritance, cancellation state machine, durable
checkpoint, receipt, journal format or filesystem policy changes are intended.
Existing failures remain recorded with exact run/job/source. A source review
cannot substitute for a macOS execution result. Any discovered production bug
requires an explicit design amendment and affected regressions.

## Validation and exit criteria

Run engine copy-launch tests with normal parallelism and repeat the isolated
EOF test under concurrent spawn activity. Run local-copy journal tests and the
canonical-parent/symlink-refusal regression, filesystem tests, preparation
interrupt/descriptor restoration tests and actual-helper cancellation tests.
Use a non-root UID, coordinated shared Cargo target, finite owned workloads and
no relay. Strict native/engine all-target Clippy, native installed-checks Clippy,
format and diff checks must pass. Inspect quality changes and record limitations.
Give Claude the exact integrated diff and permit bounded tests without competing
for the Cargo lane. Incorporate blocking findings before publication.

After review, explicitly stage only source and sanitized design/review evidence,
commit and push both remotes, verify refs, then dispatch a fresh exact-source
three-platform candidate. Confirm jobs start within five minutes. Its full
native verification, packaging and tooling must pass before eight serial
installed cohorts can qualify cancellation, interoperability and the installer.
Do not change public version pins or publish a release.
