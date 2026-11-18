# Unmounted Peer receiver quota boundary

The implementation scope is one private eligibility and callable quota boundary
at `ffb9b219cf9c4e3a24634bcae4c4f4428c2b1bb6`. Use bundled SQLCipher and
Cargo.lock MDK `b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1`. The
[dispatcher design](peer-quota-dispatch.md) retains its VFS, capacity, isolation,
and two-platform production mount requirements.

## Implementation scope

1. Project receive intent from normalized `CommandConfig` into a private
   eligibility check. It checks immutable configuration against the same
   `Arc<ProfileLease>` incarnation from which it was loaded. The sealed,
   non-cloneable `ReceiverCandidate` keeps the same lease and exact normalized
   receive policy. Compare the exact current bound configuration before any
   later owner initialization. Only Client and Both with Manual, AutoAny, or
   AutoFounders and an explicit supported private relay are candidates. Off,
   missing config, Server, and `RouteMode::Always` remain closed. The existing
   parser refuses Server with receive intent. On Linux AMD64, candidacy means
   the target can be considered for later qualification. Linux ARM64, macOS
   ARM64 and all other targets are closed. Evaluation performs no storage,
   network, or relay operations. Keep test bootstrap's fixture authorization
   separate from installed candidacy and reuse the common role predicate.
2. Add a callable private `ReceiverQuota::admit(&self, Reservation<'_>) ->
   Result<QuotaTurn<'_>>` boundary. The owner alone mints a non-cloneable
   reservation branded to its own lease and charge snapshot, with exact
   known/unknown slot, generation and file witness. `admit` consumes it and
   checks owner, current witness, capacity and exclusive dispatch slot under
   the existing reservation mutex before issuing a non-Send turn. The turn
   borrows the owner and accepts only one bounded synchronous native operation
   through the closed guard. Callers cannot supply a charge scalar, a path, or
   a constructor for a live owner or turn. Keep `ProfileSlots` fixture inputs
   available only in tests. Test the same checked admission against the
   private harness using a fixture constructor compiled only in tests.
3. Compile the shared checked quota callbacks, forwarding and admission code
   in normal builds. Keep all mounting and construction of a mounted owner
   test-only in this cut. This includes the separate proof VFS, injected crash
   and poison controls (`Quota::resize`, `ProfileSlots`), and C2's synthetic
   snapshot turn. Test-only instrumentation does not supply release accounting.
   No CLI, IPC, Cargo feature, installation or daemon changes. Neither this
   boundary nor `installed-checks` can mint the later route `ReceiverAdmission`.
   It requires the actual quota owner as well as the independently approved
   route gates.
4. Verify missing/Off, Client, Both, inert Server and Always, exact lease and
   configuration binding, wrong owner, stale/reused witness and turn
   exclusivity. Re-run the opt-in quota harness on the current lock. Compile
   non-test and `installed-checks` builds and run scoped strict Clippy. Audit
   the reachable source and package entrypoints for no added defaults or mount
   callers, and inspect artifacts for Rust-specific fixture names and crash
   controls. Linker elimination of unmounted code is expected. These checks
   establish compiler coverage and exact fixture behavior. The installed daemon
   continues to have zero live receiver quota mounts.

## Dependencies and evidence for a later runtime phase

The following defines the qualification handoff. The runtime integration needs
its own detailed approval, including un-gating the exact authenticated Peer
receive and durable custody chain. It cannot be tested in the unmounted cut.

- **Ownership and budget.** A future authorized daemon path starts at fresh
  foreground `run_with_startup` → `run_inner` from the installed binary. Give
  it sole access to registration. Split preflight setup so that path owns an
  `Arc<ProfileLease>` before evaluating intent, mounts before
  `native_preflight::check`, and carries the same Arc through the Peer owner.
  Test `marmishd init`, the public `daemon::open` including two-profile
  installed checks, and ordinary daemon startup with Peer Off, with no mount.
  A read-only preflight on a populated profile is already a SQLite open.
  Prove build-time call ordering and actual registration before that open.
  Relay unavailability or any pre-registration refusal leaves ordinary startup
  on the stock VFS. If registration's invariant fails after the irreversible
  switch, fail the private candidate rather than run against an unverified
  default. This is a release qualification failure.
- **Charge inventory.** Reconstruct known and unknown durable reservation and
  quarantine ownership from actual custody under the sole profile lease. The
  128 MiB aggregate counts each native provisional main and rollback journal
  and each other Peer provisioned artifact owned by those same reservations,
  including abandoned or quarantined bytes, once. The later phase must name
  each allowed artifact and its owner before mounting. Charge unknown entries
  conservatively by fencing the inventory. Command, Terminal, copy, stream,
  ordinary MDK, Arti and future accepted-stage stores are outside this scope.
  C2-only snapshot and founder fixture paths cannot be admitted. Bind fixed
  native stage path fences without SQLite before registering. Read the custody
  checkpoint through audited ordinary storage after registration. The custody
  journal itself is separate from the native stage mains and never enters a
  managed turn. No managed turn or receiver admission before inventory proof.
  Keep the independent 16 MiB clean closed-main guard. Reconstruct actual lengths on
  restart and hold ownership exclusion until all managed handles close. Audit
  both the current MDK call graph, worker assumptions and SQLite flags, and
  every ordinary SQLite consumer in the current packed graph, including the
  Command family, Terminal recovery and Arti's lazy directory cache in Auto.
- **Shortest useful Linux run.** A separately approved private candidate uses
  normal release-shaped AMD64 client and daemon binaries, ordinary UID, an
  explicit authenticated private WSS relay and a populated disposable profile,
  without installed fixture switches. Prove registration and stock methods in
  a symbolized build of those exact artifacts by observing the Marmish caller
  and ensuing ordinary open/method table. Compare syscalls against an unmounted
  run for ordinary I/O and use directory metadata to measure both managed file
  lengths. Concurrently drive real Command and ephemeral Terminal, an actual
  known stage and a bounded unknown stage through the genuine owner. Reach a
  real quota-refusal case using bounded authenticated input or a legitimately
  occupied admitted budget. Verify no violating physical growth and ordinary
  function after refusal and restart. Check second-process lease refusal. Stage
  reports must distinguish observed overlap, main growth, journal growth and
  refusal. A fixture scalar cannot close the installed cap claim.
- **Crash/reopen.** Observe journal write and durable sync, then stop and kill
  the owned daemon externally while the verified transaction is still open.
  A supervisor can pause the exact process at a symbolized boundary after
  observed journal sync, with a bounded timeout and cleanup. A missed cut is
  inconclusive. Before restart inspect file lengths and inode independently.
  After restart inspect durable generation, charge and quarantine using an
  independently reviewed, sanitized read-only snapshot under the lease or
  other exact observation approved for that phase. Verify preserved files,
  no reallocation or uncapped open, and ordinary Command/Terminal. Use opt-in
  harnesses separately for deliberate default displacement, injected callback
  failures, and named-VFS/direct OS bypass controls. Keep the installed source
  paths audited against those bypasses. Report exact source, lock, target,
  flags and sanitized measurements.
- **APFS and activation.** Keep APFS closed until the
  [original platform matrix](peer-quota-dispatch.md#subsequent-code-and-test-gate)
  passes independently. It includes canonical local APFS paths, `unix-posix`
  locks, explicit AUTOPROXY and nonlocal-volume refusal, forced proxy env and
  compiled flags, mixed real applications and crash/reopen. Private Linux
  evidence can precede APFS. Both platform qualifications and separate review
  remain prerequisites to production mount under the original approved design.
