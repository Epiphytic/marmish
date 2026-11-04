# Native background daemon (marmish-nqm)

## Contract and scope

`marmishd` and `marmish daemon` detach by default. `--foreground` keeps the
existing attached process, inherited output and signal behavior for supervision.
The option is valid only for daemon operation. Initialization, setup and other
commands retain their existing behavior. This does not install a login service.

Default startup returns zero only after credentials, storage, IPC and transport
activation succeed and the first active status is published. Startup failures
remain visible on the invoking terminal and return nonzero. After success,
ordinary diagnostics and the existing shutdown receipt go to the named identity's
`daemon.log`, alongside its profile, with append semantics and mode 0600. The
profile remains mode 0700. Logs do not deliberately include remote command
arguments, output or credentials. The active log is not automatically rotated in
this increment; administrators can stop the daemon before rotating it.

## Process and readiness design

Before constructing Tokio or enabling debug output, the CLI resolves its native
executable layout and recognizes daemon operation. A background parent prepares
the log using existing profile ownership/file validation, then releases its brief
profile lease before starting the child. The child acquires its normal lifetime
lease; a concurrent start still fails with `profile_in_use`. Never truncate logs,
follow a log symlink or accept a hardlinked, foreign-owned or permissive file.

Re-exec the same validated executable with `--foreground`, null stdin, both
output streams redirected to the private append log, and `setsid` in `pre_exec`.
No Rust allocation or runtime operation runs after fork and before exec. The
child is a separate session, detached from the invoking terminal. A private
inherited Unix stream carries one bounded readiness frame, independently of
stdout/stderr. Restore close-on-exec in the child immediately so helpers and
remote commands cannot keep the readiness channel alive. Reject malformed or
inapplicable internal startup context before accessing providers.

The parent waits for at most 240 seconds (credential preparation permits 30 and
activation permits 150, with additional storage/scheduling margin),
requiring a success frame while the owned child is still running. EOF, malformed
frames, failure or timeout return failure and terminate/reap only that owned
child. Attempt SIGTERM with a 45-second joined-shutdown grace period before a
final kill for missing/malformed frames or timeout. An explicit error frame is
sent only after joined cleanup; wait for normal child exit rather than killing
it while it is cleaning up. SIGTERM before `serve_owner` installs its handler
has the ordinary immediate-termination disposition; this is crash recovery,
not a claim of joined shutdown. Never signal an arbitrary PID read from disk.
A successful parent
exits without killing its detached child. Startup error frames use the existing
finite public error text, not raw provider or relay stderr.

Use a fixed descriptor 3 for the internal Unix stream, selected by a private
environment marker, not a public arbitrary-fd flag. Create close-on-exec ends,
duplicate the child end to 3 in `pre_exec`, and clear CLOEXEC only there. At
single-thread child entry, remove the marker, validate stream/socket type and
peer uid plus parent pid where the platform exposes it, restore CLOEXEC and
close unrelated inherited descriptors above 3. Reject internal context outside
foreground daemon operation. Keep the current directory and raw arguments,
inserting `--foreground` before operation arguments. Successful startup is quiet.
Use a two-way ready/ack exchange: the child treats EOF/missing acknowledgement
as failed startup and performs joined cleanup. Parent death detection is bounded
by activation plus the short acknowledgement deadline, including on macOS.

Use a dedicated append opener (O_WRONLY/O_APPEND/O_CREAT/O_NOFOLLOW/O_CLOEXEC,
mode 0600) with post-open regular-file/nlink/owner/mode validation. Nonblocking
open may be used to reject a malicious FIFO without hanging, but must be cleared
on the validated regular file before redirecting stdio. Never leave the
non-append or nonblocking flags of the existing read/write helper on stdio.
Main retains a reporter clone and sends finite failures for layout/runtime/run
errors. EOF before a frame reports finite early-exit status and identifies
`daemon.log`; never copy log contents to the terminal. Diagnostic output uses
non-panicking writes and degraded tick messages are rate limited.

The child reports ready only after `owner.activate` and publication of the first
active snapshot in `serve_owner`. A startup error propagates through existing
joined owner cleanup and is reported through the private channel. Early clean
termination (for example an uptime limit during activation) is not readiness.
Loss of the startup parent must not leave a newly started unacknowledged daemon.

## Client autostart and integration

Reuse the background launcher for client autostart, including private logs and
readiness/error reporting, without nested detachment. A blocking launch must run
outside Tokio's worker threads (`spawn_blocking`). Align the client's startup
budget with the explicit daemon budget rather than silently timing out after
15 seconds (the startup budget is independent of command `--wait-seconds`). In
autostart only, a `profile_in_use` error from the parent or child means another
starter may be winning: continue status polling until the same shared deadline.
Keep a waiter thread for successfully launched children in a live client.
Preserve `--no-autostart`, explicit sockets, relay policy and named
identity selection. Existing test supervisors add `--foreground` so they retain
their real child handle, output drains and exact shutdown receipts.

The native owned-spawn inventory is `installed_matrix::Daemon::start` (also used
by benchmark/recovery/policy), `installed_autoexit::Daemon::start`, and the
negative incompatible-start case in `installed_version_lifecycle`. The Python
`marmish_installed_runtime.py` and `marmish_e2e.py` exercise frozen v0.1 tooling
with different arguments and retain legacy semantics. Keychain diagnostic
`marmishd init` is initialization, not a daemon launch. README debug instructions
must offer explicit foreground operation for attached diagnostics. Audit worker
and provider stdio inheritance so private remote command output is not logged.

## Implementation and acceptance

After independent Claude design approval, delegate the launch/readiness/log
module and its process tests to one agent. Root owns CLI integration and daemon
ready notification. A second agent updates installed supervisors and associated
documentation with disjoint file ownership. Coordinate shared Cargo ownership.

Test process detachment/session identity, inherited-descriptor closure, bounded
handshake success/failure/EOF/timeout, owned-child cleanup, safe append/private
log rejection, foreground parsing, operation restrictions and real startup
failure visibility. Exercise an installed disposable-key live command with the
new default launch and verify active status, redirected diagnostics and joined
shutdown. Run native suites and installed-tool tests as appropriate. Linux
local evidence does not claim a macOS runner result; wire the same tests into
the existing platform matrix and record actual execution separately.

Update README and agent specification, record sanitized validation and actual
Claude auto code review, resolve blocking findings, inspect exact staged source,
then commit and publish both source mirrors. Keep the currently published 0.2.2
binary behavior distinct until a separate corrective distribution is published.
