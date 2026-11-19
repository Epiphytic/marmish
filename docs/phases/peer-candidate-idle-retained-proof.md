# Qualify registered idle stop with retained Command results

This private Linux AMD64 increment extends the accepted
[idle stop design](peer-candidate-idle-command-stop.md#qualification). The
working registered client case at `62665aa` already proves no-engine startup
and stop. Published `a35afc6` includes the startup from `dc5a1d7` and the
reviewed idle prelude. Use the same source-matched installed candidate for existing
no-engine and new Both checks. The prelude review covers `0429ed1` and its predecessors. This increment adds tests and evidence.
The earlier phase still owns interrupted FinalObserve and timeout matrices.

## Positive installed case

Use the existing registered startup qualification harness and the shared
qualified Both fixture. Launch one real candidate in a private installed
layout using the existing qualification example, installed daemon and helper.
Use a separate root and prepare the sender's file-backed profile identity
first, via the existing fixture helper. Register that key on the ordinary
Command relay. Prepare the same profile lease, identity and exact Manual qualification as the
no-engine case. Implementation starts only with the dispatcher-loss task's
committed helper `9048a1d09a12b0cd9de9d39b467a784b83fd216b` as an ancestor
of the retained test worktree. This private design tree already includes
that ancestor. The dispatcher-loss task owns all three helper files and
the shared `qualified_manual_fixture` refactor. The retained task will
reuse the exact `qualified_both_command_fixture(command, third, parent,
ordinary_relay, executable)` without copying its source or taking files
from a working tree. The committed helper pairs Manual Peer receive with
`role: "both"`, sets both the ordinary execution and trust lists to the
same sender, permits only the supplied canonical executable and checks
that the Command relay differs from the Peer origin.
Both permits Command serving while keeping Peer Manual. Execution is scoped
to one client key and one path. Give the candidate distinct WSS relays for
Peer qualification and ordinary Command, and configure its one authenticated
Command client to use one canonical executable. Start that sender through the
ordinary markerless qualification-example client path, with Peer off in its
own profile and the explicit disposable Command relay trust root. Register
the candidate on the same Command relay, then configure the sender to trust
that candidate account. Drive the sender through its real IPC socket. Bound
both test processes and reap them on every outcome. Keep trust roots and keys in each test's private state.

Reuse the ordinary Command helper's real Prepare, Submit, Response, Poll and
Consume path. Preserve its existing `cat` caller by passing its byte-vector
argv and payload through an internal test-only seam. The common helper
checks the exact new invocation handle in inventory. Only the original
wrapper asserts a one-entry inventory and uses its original payload. In this case invoke the canonical
`tee` with `-a` and the exact private marker path as separate argv values. Use
two distinct fresh invocation IDs and two different byte payloads containing
binary and shell-sensitive values. Compare each returned byte stream and
closed Code(0) result. Return only the exact invocation ID to the test so rows
can be matched across the actual subprocess restart. Confirm the independent
marker has exactly one copy of each payload in submission order. On success
both invocations must be terminal and both outputs consumed before sending
SIGTERM. Allow up to five seconds to observe a 500 ms quiet interval on
the disposable Command relay between final consume and the single SIGTERM.
Relay quiet stabilizes fixture scheduling. Actual timely exit zero proves
clean shutdown, and the fixture fails on a nonzero exit without retry.

Observe actual clean child exit, IPC socket removal and release of the exact
profile lease before starting the stock reopen process. Leave Peer config and
ordinary identity unchanged across that reopen.

Build and privately install both Linux AMD64 package classes from the same
source, each with its own same-class packager, daemon, worker and manifest.
Use separate prefixes, launcher directories, HOME and XDG paths. Verify
both install receipts. Build the two
source-matched Linux AMD64 libtest drivers using
`cargo test --locked -p marmish-native --lib --no-run`. The candidate driver
uses the reviewed private candidate cfg and runs from the installed version's
`deps` directory. Use separate warmed `CARGO_TARGET_DIR` values for the
two drivers. Build them from the same exact worktree commit with the same
explicit nonzero `MARMISH_BUILD_COMMIT` and without `installed-checks`.
The stock build must also unset the candidate flag from target Rust flags.
Record both test binary SHA-256 digests and pass the stock binary's
absolute canonical path to the candidate controller as
`MARMISH_TEST_REGISTERED_STOCK_LIBTEST`. Supply the distinct stock installed
daemon as `MARMISH_TEST_REGISTERED_STOCK_DAEMON`. The controller passes its
canonical path to the child, which validates and uses that stock layout.
The controller runs
`--ignored --exact peer_invitation::daemon_wss_tests::registered_stock_reopen_child
--nocapture`. This selector is compiled only on stock Linux AMD64. Follow
the existing exact ignored-child subprocess pattern. Pass the absolute
owned state root, installed daemon path, WSS URL and disposable CA path
through test-only environment inputs and retain the same profile name and
configuration. Bound the process, clear inherited startup markers and keep
the ordinary WSS fixture running until after it reaps. Child input is one
fixed byte from the parent's owned stdin pipe. A blocking reader sends
the existing `PeerFixture.stop` oneshot on that byte. EOF or a different
byte is a child failure. No listener besides the released same-UID
Unix socket is opened by the child. The parent waits for normal same-UID
readiness, queries ordinary IPC, writes the stop byte, and requires reaped
exit zero **and one passing test** in the bounded stdout summary. Zero
matched tests therefore fail the proof. Missing inputs, failure or timeout
are hard test failures.

The child's test-only `daemon::run_peer_fixture` uses `bootstrap: None`,
the separately installed, source-matched stock utility layout and
`RelayConnector::for_loopback_fixture` for the isolated relay's exact
temporary CA. Follow `peer_daemon_wss_tests.rs`'s existing
`Round::start` config construction. Use a file backend, 24-hour retention,
no test command or uptime caps and the same direct one-relay policy.
Set the connector's Arti directory names under the owned profile as
`arti_state` and `arti_cache`. The daemon's standard stock startup
acquires and releases its own lease. Parent owns no lease while it runs. It reaches stock `open_stock` and `serve_opened`. The stock resolver refuses the candidate manifest class. Require that exact
refusal before using the independently installed stock utilities. Use the same state root, exact identity and untouched configuration
across both processes. This
proves Linux file-backed stock cold reopen in a separate process under
fixture trust. The
shipping stock connector uses bundled roots and the markerless candidate
would reselect Registered for this unchanged profile. Query
`CommandEvidence` through the existing administrative projection.
Sort the two retained rows by their request-independent durable
cursor and correlate the exact invocation IDs to the earlier client calls.
Assert the exact authenticated principal, known engine, no refusal, terminal
Code(0), correct lengths, known exited worker, clean authority and a nonzero
retained observation sequence. Bound evidence at exactly these two rows and verify an empty next page. Stop stock normally and compare the
marker before stop, after candidate exit and after stock reopen to check for a
second launch. Require Peer kind 445 and 7435 counts of zero and no Peer subscriptions
beyond the previously captured qualification counts. Require positive
Command kind 445 and zero kind 7435. Counts use actual relay publications. No SQL, output bytes, addresses or
identities may enter committed validation evidence.

## FinalObserve and internal ACK proof

The registered active `finite_dispatch` exposes neither `CommandEvidence` nor
`CommandCleanupStatus`. Keep that boundary. The installed proof does not
concurrently open SQLCipher from a second owner, configure an administrative
side channel or add a stop option to the shipped daemon. The clean-exit proof
necessarily passes through existing `command_cleanup_fresh` at the newly
allocated fence, and the real engine only emits the consumed `Quiesced`
result after its pending/awaiting/unresolved state has drained. Stock cold
readback corroborates the exact retained rows and the independent marker checks
launch count.

For per-row evidence, run a separate real Command engine test nested in the
idle prelude module. Reuse WSS `terminal_command_fixture` room, client and
Command harness and real `marmish-worker`. Start this server with
`CommandEngine::start_candidate` and `Admission::new(&trip, || true)`,
following `mdk_owner_physical_cut_tests`. Keep one exact live admission for
engine and `IdleFence`. Run two requests. Before Begin, drive bounded normal
server ticks until the known Journal idle counts and
`CommandService::cleanup_observation_pending` show no active, uncertain or
pending work. Stop new admission with the existing `close_admission`.
Call Begin once and fail on busy. Snapshot sole-owner Journal evidence
before Begin and again after same-fence `trusted_turn` polling returns
Quiesced. Require each retained row's `observed_seq` to meet or exceed the
allocated fence and to advance beyond its starting value. Require no
lingering service registration and no pending engine observation from
`take_engine_for_stop`. Join the worker using the bounded existing
`stop_started_engine` test pattern. Its actual same-fence Quiesced, which
requires a drained pending/awaiting/unresolved state, proves the internal
ACK effect. This checks the actual `trusted_turn` and per-row operation
effects. The installed proof exercises `RegisteredStop` sequencing,
the real production budget and the full physical drain. Both checks run
without a recorder or new admin projection.

Time the private setup separately from the two finite invocations, quiet settling,
SIGTERM to actual child reap, and stock reopen. Report counts and elapsed intervals as
one opt-in fixture run. The installed test remains ignored by default. Run the
existing no-engine case and stock unit regressions around `command_service`
and `idle_cleanup_`. Require exact candidate and stock strict Clippy, fresh
source-matched candidate and stock installations, and independent Claude Code Auto code
review before reporting the positive proof complete.
