# Qualify the registered daemon startup report

The [Command service increment](peer-candidate-run-admissions.md) needs a
parent that can distinguish a provisional readiness exchange from committed
startup. This private Linux AMD64 increment shares the existing config,
install layout and identity preparation. No installed entry point selects
registered service. The public launcher retains its existing Unix socket
protocol, timeout and cleanup semantics.

## One registered startup owner

Share the stock prefix from ServiceLifetime origin through directory creation,
ProfileLease acquisition, command config load, validation, limit overrides
and relay selection. Inspect Ready and effective relay policy read-only
after config and before identity prepare in the private path. The exclusion
probe creates no connector and mutates no profile. Effective Tor,
Initializing, and other reviewed exclusions stop candidate preparation at
this point. The stock continuation receives the same lease and lifetime
origin and runs identity prepare, connector construction, elapsed-lifetime
check and native preflight in their existing order. An eligible private path
uses the same identity prepare and actual connector construction, then
confirms effective policy before registration. Prove the lease interval,
Arc::try_unwrap behavior, uptime origin and relay-binding, identity and
preflight error precedence.

Only confirmed registration enters registered ownership. A proved Untouched
result drops temporary witnesses and unwraps the same lease for the stock
continuation, reusing the prepared identity and connector. Registration
LocalUnavailable means conflict, calls no competing witness and reports
the fixed diagnostic. Indeterminate retains the original lease and
witnesses through process exit. Neither failure enters a stock continuation.

Create registered custody as soon as registration succeeds. The exact
Admission and Fatal, original Arc, and single-use Command fence feed guarded
preflight, native open, the independent read-only check of the Ready binding,
and checked Command setup. Install the fatal derivative before cloning the
connector. Retain an acquired resource before the next admission check.
Do not prepare Terminal. Once custody exists, route any error through the
same [owned stop](peer-candidate-owned-stop.md). On a startup error before
the listener or worker exists, explicitly record that obligation as
NeverStarted. Process exit does not substitute for proof that a returned
resource stopped.

## Readiness transaction

The existing stock parent treats the provisional R byte as readiness after
sending A. The private wire exchange uses R as an offer, A as an
acknowledgement and C as the single-byte commit. The private parent reports
success only after C. It never accepts an R or A by itself. The child
uses owned, nonblocking I/O in the service future with a finite deadline
and no detached blocking writer. The dedicated qualification executable
selects this exchange before creating Tokio using a distinct private
marker value on the same inherited descriptor. The private selector is
compiled only with the Linux AMD64 candidate and stays inside the private
test driver. It removes the marker on the single-threaded entry and
validates the same descriptor and parent credentials. Normal public
take_from_env still rejects every unknown value including the private
one. The existing launch, launch_with_child_mask, spawn_child,
await_startup, ready, failed and take_from_env keep their stock behavior.
Any common launcher helper is additive. Verify stock builds exclude the
private constructor and report API.

Take the channel once into registered startup custody before beginning
the exchange. Its sole stream owner grants no write to retained
StartupReporter clones. Dropping or revoking custody closes it. Run the
exchange inline after activation and listener bind, before any dispatch,
request receive, owner tick, room turn or Terminal step. Keep listener
Status in starting until C commits. In the foreground set active after
the equivalent commit checks.

For both R and C, check the exact admission immediately before attempting
the write, and fence each resumed write poll with the existing raw socket
fence. Give the entire child R, A, C exchange one ACK_TIMEOUT (5 seconds).
The private parent bounds the exchange by STARTUP_TIMEOUT (240 seconds)
and its A write by the remaining budget capped at ACK_TIMEOUT. Its test
budget must leave more than a child's budget after observing R except in
an explicitly forced timeout test. Race the exchange in priority order
Fatal, shutdown, lifetime expiry, listener exit, exchange. Keep the
shutdown, expiry and listener observations in owned state and independently
recheck them alongside exact Admission after A and before the C write.
Observe a ready listener join once and keep its outcome. Any ready stop
signal revokes the channel and enters owned stop with no later C poll.
Validate A as an exact single protocol byte.
Loss ahead of an attempted C forbids that write.
Admission cannot make a Unix socket write atomic with the registry
transition. A successful one-byte C write already admitted before loss
may reach the parent. Treat that as a committed startup followed by live
failure, with immediate closure of all service authority. There is no
second success write. Record this point before any post-write fatal
handoff. Foreground startup uses the same proof checks and marks entry into
the live loop as its commit in the absence of a parent channel.
The private parent can decide failure while C is buffered. It closes
the channel when it decides failure and never changes its report. The
child may be live in this case under the harness's retained ownership.

Before an owned stop can hold the process indefinitely, drop or revoke
the provisional channel so the private parent reports failure without
waiting for process exit. Before R, a sanitized failure frame of at most
259 bytes may use one nonblocking best-effort write. Drop on partial or
failed write and never wait for readiness. The private registered child
uses owned disposition and never reports again through main.
After R, EOF is sufficient. The registered test parent treats premature
EOF, invalid messages or timeout as startup failure. It transfers child
wait ownership to the qualification harness on both success and failure.
Reuse spawn_child to close the parent's copy of the child socket, choosing
the private marker only in the added qualification parent entry. Extract
the std::process::Child from its launch guard immediately after
successful spawn and before any protocol I/O or error return. Transfer
that owned Child to the qualification harness regardless of protocol
result. The private parent does not wait after F, EOF or timeout, and the
stock OwnedChild retains its semantics. Keep explicit cleanup for disposable processes at the
end of the test, and only assert successful owned stop after positive
reap proof. Once the private parent reports failure it cannot later report
success from delayed bytes.

## Acceptance

- Exercise the installed layout with the private harness, with and without
  parent reporting. Check the exact one-profile and one-listener custody,
  successful stock fallback for excluded effective Tor, and stock launcher
  protocol regressions with the existing test_channel socket, child_ready,
  child_fd_three, fixed_fd_source_collision_clears_close_on_exec and
  inert_daemon_reporter_wait_prevents_release_on_failure_or_stop.
  Test a healthy remote finite command after commit
  and a second independent stock open after healthy shutdown.
- With a real dispatcher replacement, cut before the provisional reply,
  after reading R but before A, and after A but before the C write.
  The parent reports failure and any still-owned live resource continues
  through the held disposition. Anchor a second boundary test after the
  admitted C write and verify the allowed commit followed by service loss.
- Hold the parent exchange until timeout or a malformed A. Check closure,
  process disposition and absence of late success. Verify that a startup
  failure with an unresolved owned resource wakes the private parent early
  and keeps the child lease held until proof or test cleanup. No early exit
  is inferred from EOF. Obtain exact Claude Code Auto review.
- Cut during a pending R write poll and close the parent between R and A.
  Test a private parent receiving F before R and F or garbage after R.
  Exercise SIGTERM, lifetime expiry and listener exit during A. Force
  parent timeout with C already buffered and prove the still-owned
  private child is cleaned up under the harness.
