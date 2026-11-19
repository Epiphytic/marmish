# Registered daemon process test for dispatcher loss

This phase qualifies fail-closed behavior on Linux AMD64 after the healthy
registered Command subprocess proof. The implementation gate is the exact
reviewed healthy composition and its passing real Command round trip. Use the
private candidate build, real same-UID listener, actual registered owner, and
private authenticated loopback WSS/installed helper layout. Keep the installed
launchers, non-test qualification executable, default feature policy, release
pipeline and ordinary protocol unchanged. This phase covers idle registered
owners. A real in-flight Command/runner, loss during the graceful callback and
indefinitely parked obligations remain separate tests under
[the owned-stop design](peer-candidate-owned-stop.md).

## Private harness

Add a candidate-only libtest subprocess selector alongside
`peer_daemon_registered_qualification_tests.rs`. Each scenario uses a fresh
profile and process, with the same checked helper install and Ready preparation
as the healthy control. Use a checked Both-role fixture with one disposable
authorized peer so the ordinary engine and its actual SQLite store are open
and idle. Do not send a job, observe an invocation or start a job helper.
Call the real `daemon::registered::run` with the checked layout and ordinary
WSS connector. Share existing qualification layout, input and CA checks
without weakening them.

Each case must start with a new Ready identity and an actual current
issuer-signed route qualification receipt. A Manual config plus Ready is
insufficient: if route checks fail, `daemon_candidate::select` selects Stock,
which refuses a registered startup channel. Reuse the corrected healthy
fixture's `qualified_control_fixture` and
`QualificationFixture::from_ready_configured` path with a test-only Both
configuration builder. The current healthy Client fixture leaves its
execution list empty. Before receipt issuance, the Both builder sets Manual
receive, the exact direct qualification origin and its trust in `peer.relay`
and `peer.relay_trust`. Set top-level `relays` to exactly one ordinary
private WSS URL, at a **different origin** from `peer.relay`, using the
healthy ordinary WSS harness.
The same disposable account is the trusted remote and execution principal.
Run the real bounded qualification probe and issuer on the Peer origin. Use
the ordinary endpoint and its own checked CA for the subprocess and Command
SDK key-package publication. The two listeners use distinct private trust
roots, with no route or trust fallback. Leave the strict qualification
listener and its filters in place. Retain bounded accept and event
disposition transcripts and require the probe connections to close during
preparation. Any accept on the Peer listener from spawn through reap is a
setup failure. No other client connects to either listener in that window.

Drop the preparation custody and retain the exact account, config, issued
receipt and profile for fresh subprocess reacquire. Check the qualification
count and issuer-produced record before each spawn. Do not rewrite config
after issuance, shorten checks, copy a receipt from another profile, or use
a synthetic qualification record. Require the real Registered selection and
ordinary engine startup before any cut. Missing, stale or mismatched
qualification is a setup failure with no substitution.

The separate healthy test still uses the non-test example and proves the
Command round trip. The two fault cases qualify the private candidate libtest
composition only.

Reuse the real parent startup parser and fd 3 `StartupReporter` handshake.
Keep child's libtest argv to the one exact selector and standard harness
arguments. Supply a distinct private anonymous Unix socketpair endpoint on
stdin via a test-only spawn variant. The other endpoint stays with the
controller. Validate fd 0 as the expected connected Unix stream, set its
close-on-exec flag and take it once before claiming fd 3. A private test-only
constructor claims prevalidated fd 3 through the existing descriptor type,
peer and close-on-exec checks. It does not call the single-threaded
`take_registered_from_env`, mutate the process environment, or close unrelated
libtest-owned descriptors. The spawn variant inherits no fixture descriptors
except fd 0 and fd 3. The production handoff, marker handling and unrelated
descriptor cleanup keep their existing behavior.

Controller frames are one-byte tag, big-endian u32 length and exact payload.
The initial parent frame B carries a one-byte scenario and four u16-length
fields (canonical install daemon path, private state root, exact ordinary
loopback WSS URL, private ordinary CA path), each at most 1024 bytes. Use a
fixed profile, direct mode and existing qualification input, layout and CA
validation. The exact combined config and receipt in the prepared profile
remain the source for Peer origin and trust. B carries neither. No other
inputs are accepted. In the reverse direction S is the single pre-cut record (fixed
scenario and ordered bounded witness entries), P is the single physical
record (closed enums, including observed failure basis), and X is the
one-byte fixture failure category. Each database entry has a fixed index,
presence indicator and, if present, device, inode, length and SHA-256 digest.
Known file count limits S to 1024 bytes. P is at most 32 bytes. The parent
permits only S then P. It sends empty G once after validating P and independent
proofs, and then waits for control EOF and the owned process. Unknown tags,
duplicate/trailing frames, truncated data, wrong order or mid-proof EOF fail.
Bootstrap is bounded by five seconds, exchange by the original operation
deadlines and the finish pause by two seconds. Control bytes are never parsed
as fd 3 readiness or Status IPC.

The child substitutes the real SQLite default once, using the existing
registration fixture's SQLCipher ABI. A once-only pre-cut assertion verifies
forwarding admission before the switch. Replace the registered default with
SQLite's initialized stock `unix` VFS and check the new actual default.
Never restore it. Only existing production admission may trip the actual
process Fatal. Emit no addresses or pointers. Invalid fixture transitions,
missing deadlines and controller EOF after bootstrap emit X if possible and
terminate via a reserved fixture-only exit code (86). They cannot be swallowed
by the listener's JoinSet. An independent test control reader starts after B
and detects EOF/controller loss even if the async owner is stuck. It accepts G
only after P was flushed. The controller always owns the exact Child in a
cleanup guard from spawn onward, even while the original report parser runs
with a finite deadline on a separate thread. Cleanup on harness failure closes
the control pair and kills then reaps only the known owned setsid leader and
its verified group, before returning failure. Verify its actual PGID equals
the fresh owned PID before signaling the group and retain the unreaped child
during that operation. After successful wait, a zero-signal probe of the exact
former process group must return ESRCH. Never enumerate or signal unrelated
processes. Parent loss closes the sole controller endpoint and triggers the
child watchdog; this idle fixture has no job subprocess to clean up.

No controller gets a database, flock or transport handle. Keep the fault
driver, mutation and observers under `cfg(all(test,
marmish_peer_receiver_candidate, target_os = "linux", target_arch = "x86_64"))`.
No paths, install flags, signals, env switches or network traffic can opt a
non-test executable into the fixture.

## Two distinct cuts

1. **Before Commit.** The real parser reads Ready and sends A. A one-shot
   child hook in `ChildChannel::exchange` runs after valid A and immediately
   before the production `Stage::Commit` admission/write. Capture the
   pre-cut image and switch the VFS there only with a measured minimum one
   second remaining on the original five-second child ACK budget. Require
   the actual exchange failure to be `Failure::Fatal` in P. Let the unmodified admission/write
   path observe the mismatch. A read-only test observer in the actual parser
   records the real R/A ordering and terminal EOF/reset with zero post-A
   bytes, while the ordinary report returns an error. A decoded failure tag,
   unknown byte, premature EOF before R/A, timeout, or any C fails this
   oracle. No active Status is expected or used to establish this case.
2. **After Commit.** Require exact real R/A/C and one completed real Status
   exchange showing active. With the dedicated private same-UID socket, send
   a second strictly sequential active Status request. Count at the valid cached
   active-response selection, so startup polling of an activating response
   does not consume a selection. In the registered Status arm
   of `ipc::serve_with_listener`, pause the one accepted request after it
   has cloned the successful cached active response but before any write.
   Take the pre-cut image and replace the default during this hook, on
   the daemon's current-thread reactor. Require at least one second of
   remaining original Status I/O budget when replacing the dispatcher. The
   hook waits for that real `Fatal::tripped()` with a shorter deadline, after
   which the original fenced write remains the only possible write. The listener
   may cancel the handler only after the actual latch trips. Require production
   detection before the earlier test deadline and zero bytes before the original
   exchange deadline. The hook invokes no post-cut admission or verification
   itself. It never constructs a success,
   writes bytes, directly trips Fatal or just closes the socket. The client
   reads the original accepted socket until actual EOF/reset, with zero
   observed response bytes. The accepted-before-cut marker and baseline active
   response are required. Separately a fresh same-UID Status attempt after
   observed fatal must return ECONNREFUSED or ENOENT on connect, or EOF/reset
   with zero response bytes. Socket unlink is not evidence for either assertion.

Use an idle dedicated profile and both local listeners with no inbound
deliveries, new requests or Command work during the measured cut. Do not interpret quiet
relay time as a transport shutdown. Keep the original deadlines in place,
including the five-second startup ACK deadline and the actual Status exchange
deadline. Bound snapshot work separately and require the observed terminal
outcomes within the operation deadlines. A race, a dirty witness or a scheduler
delay is a failed qualification and cannot be turned into a new baseline or
passing timeout.

## Physical completion and storage evidence

Use a read-only one-shot hook in `RegisteredOwner`'s `finish` after observing
the actual `HealthyStop` report and listener join, before the fatal exit and
without taking custody. Copy fixed categorical values for actual listener
join outcome, sticky Fatal, graceful eligibility and all five physical
reports (engine, runners, SDK, forwarder and raw). Require listener
`Some(Ok(Ok(())))`, actual Fatal, no graceful completion and all five
`Complete`. A code-1 outcome qualifies only after P flushed and G received.
Require the existing `registered_shutdown/owned_stop_failed` category. The
single test-only pause retains the live guard and flushes evidence to the
controller before a tightly bounded release. It cannot
change the real shutdown disposition. Before either cut, require one or
more ordinary listener connections from this child in the exclusive
post-spawn window. Record their transcript connection IDs and loopback
peer ports and require the observed engine subscription and SDK key-package
publication on those connections. On the pause, assert independently that
the ordinary listener observed every one of those IDs close in its bounded
socket-end transcript. The Peer probe traffic is separate by listener and
preparation window. Assert the pre-cut image is unchanged and that another flock
acquisition for the exact profile fails with `profile_in_use`. Keep scoped
release/reap bounds separate from socket and startup observations. Release
the pause and require owned wait status `code() == Some(1)`, then check the
raw image again and prove flock becomes acquirable only after this exit.
Report an obligation marked Complete with no live instances as such. A
successful wait or a transport counter alone cannot stand in for another
barrier.

Use byte witnesses for `ProfileLease::{journal_database,mdk_database}` and
`commands/engine.sqlite3`, including every conventional `-wal`, `-shm`, and
`-journal` sibling. Require all three main files to be present. Confirm the
fresh engine's preflight has no jobs or spool payloads and none are added.
The engine store is based on `command_config::engine_policy` and
`enable_candidate_commands`; the Both-role authorized fixture avoids the
client-only path that creates no engine. The pre-cut child snapshot is taken
synchronously in the named hook just ahead of substitution. Use only those
paths relative to the checked private profile directory with no symlink
following. Require private owner-checked regular single-link files and
stable device/inode/length across each read; match absence explicitly.
Use bounded fixed-count streaming hashed reads with at most 32 MiB aggregate
per image and a maximum 250 ms observed synchronous pre-cut capture budget.
Check clock and pin before/after each bounded read and also enforce the parent
wall watchdog. Refuse oversized, changed or unreadable evidence. Perform
pre-C first. For this idle topology, every tested sidecar must be absent at
every observation or present at every observation with the exact same
device, inode, size and bytes. No presumed WAL/SHM stability and no
normalization or retry may turn change into a pass. If the pinned runtime
legitimately changes a sidecar, record the raw differing category and defer
a reviewed oracle revision. Do not claim this proof for that composition. The after-cut witnesses before exit and after reap must
match the exact original path presence, inode, length and digest without
opening SQLite in the broken process or in the parent before those
comparisons. Confirm the seeded Journal and MDK identities through a fresh
stock reopen with the original material under the reacquired lease, and
verify the pre-seeded Ready binding and exact Journal account/schema
binding, unchanged empty Journal Command recovery and independently opened
engine recovery with the original derived stock key and an empty invocation
store. No job helper, Command publication, or delivery occurs during either
measured run. Stock reopen comes after byte
comparisons and may do its normal writes. Exercise an altered byte or missing
sidecar on a disposable deferred copy to verify the witness comparator
refuses it. This evidence concerns the listed file image and durable seeded
state at observed boundaries. It cannot exclude a write that restored every
witnessed byte before observation.

## Acceptance

Qualify with the existing pinned lock and exact installed helper artifacts.
Run both candidate subprocess cases single-process-owned against private
loopback WSS, the preserved healthy registered subprocess and Command round
trip, relevant startup protocol and physical owner tests, and strict native
Clippy in candidate and stock configurations. Verify by normal build
inspection that the controller, parser observer and substitution are absent.
Record only case categories, counts, timings, wait disposition and root
comparisons. Preserve no paths, helper process IDs, keys, file bytes, digests,
or live sockets in tracked evidence. Do not turn the expensive private
subprocess fixture into default CI as part of this phase.
