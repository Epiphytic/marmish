# Native stream runtime design review

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Phase: 2a; no implementation began before the round-2 technical approval.
Direct user implementation authorization applies; this is not shared queue work.

## Round 1

**REQUEST_CHANGES.** The overall shape is sound and implementable, but five areas aren't specified precisely enough to code against. B1–B5 are blockers. I made no edits or builds, and I didn't read the phase 1 code. I read the three phase docs and the engine/native contracts they depend on:
- `Engine::{authorize, resolve_exec, check_execute_request}`
- `launcher::spawn_child`
- `worker.rs` `ChildOwner` and its shutdown logic
- `command_wire`, `mdk_adapter` publication validation and fanout protection
- `ipc.rs`
- `command_limits.rs` and `command_store` admission budget

## What checks out

- **Quota:** a single combined count is feasible. The native journal already holds the finite admission budget (`command_admission_budget`, `command_store.rs:1371`), so streams can be counted in the same SQLCipher transaction. The engine's own `active_jobs=32` stays as defense in depth.
- **Spawn path:** `spawn_child` (`launcher.rs:1144`) does not itself call the restriction check. The design correctly calls `check_execute_request` first, then spawns the resolved exec.
- **Token lifetime:** this was a phase 1 concern and doesn't apply. The live path doesn't use `submit`, so the 60 s `ExecuteToken` and `fresh_for ≤ 60 s` limits don't constrain a long stream.
- **PATH:** it is fixed at `/usr/bin:/bin` by `child_environment`, and spawn requires an exact match. Homebrew rsync works through `ResolvedExec` with an absolute path, as the doc says.
- **Publication:** claiming the frame durably before send, and failing on an ambiguous send, matches the existing command pattern. Frame app-event IDs are content-derived, so retrying identical bytes is idempotent.

## Blockers

**B1. The idle scheduling contradicts itself, and the idle cost is undefined.**
- The doc says the server replies *immediately* with whatever is available, and that there is a 5-minute keepalive.
- With stop-and-wait, an idle client either re-polls immediately or waits up to 5 minutes for the next output.
- Immediate re-polling means an unbounded message storm. Each exchange is two MLS messages, each with a journal write and a relay publication.
- A 5-minute wait means rsync output is delayed by up to 5 minutes.
- Pick one policy and write it down. The minimal choice is client-side adaptive backoff:
  - send immediately while either direction made progress or local stdin is ready;
  - after empty responses, back off to a stated cap (for example 250 ms up to 2–5 s);
  - wake early on local stdin or Ctrl-C.
- Server-held responses would also work, but they need a pending-exchange slot and a way for a new stdin push to supersede a held exchange. That is not minimal.
- Also state a per-stream server-side exchange rate limit and the per-frame journal retention and pruning bound.

**B2. The cancellation wire and state semantics are incomplete.** The phase 1 envelope gives every request an exchange sequence, so a Cancel sent while Exchange N is outstanding has no defined sequence. Specify each of these:
- **Control lane:** Cancel and Status use a separate control idempotency key, or sequence 0 with a digest. They don't consume or require the exchange sequence.
- **Latch:** Cancel is an irreversible server-side latch, so later Exchanges cannot un-cancel.
- **Cancel before Open:** relay reordering is possible, and so is Ctrl-C while the Open is pending. The server must write a cancelled tombstone so a late Open never launches.
- **Cancel during Preparing:** it ends as Cancelled-never-started, with no spawn.
- **Cancel during Launching:** the runner re-checks the latch after `spawn_child` returns. If the latch is set, it kills the child immediately and reports Cancelled with the child started.
- **Cancel while Running:** record Cancelling durably, then send TERM, wait the grace period, then KILL.
- **"Confirmed cancellation" definition:** the final KILL is sent while the leader is still unreaped, then the leader is reaped, then the terminal state is recorded. After the leader is reaped, never signal `-pgid` again, because the PID can be reused once the group is empty. `ChildOwner.signal` already encodes this check, and the design should say the new API does too.
- **Cancel retries:** Cancel (and Status) must be exempt from "an ambiguous send fails the stream, never blindly resend". They are idempotent, so resending identical bytes is safe and necessary. Otherwise an ambiguous Cancel publication leaves the remote child running while the client reports failure.
- **Local failure:** after any local failure that ends the attachment, the client makes a best-effort Cancel.
- **Lost client records:** Lost marks the local attachment only. `marmish cancel --stream ID DEST` must still send Cancel for a Lost client record until a remote terminal state is known. Otherwise a client-daemon restart makes cancellation useless.
- **`marmish streams` source:** the phase 1 wire has no List request. State that `streams` is a projection of the client's local journal with last-known remote status. State that cancel from another terminal works through that local record's sealed binding.

**B3. Child lifetime across a daemon crash or restart, and orphans from a vanished client, are not covered.**
- **Orphans after a crash:** "Lost on restart" leaves a blocked child (for example `sleep` or a stuck rsync) running with no way to find or cancel it. Children are not killed when the daemon dies. They only see EOF or EPIPE if they happen to touch their pipes.
- **Persist identity at spawn:** commit the child's `{pid, process_identity, pgid}` to the journal immediately after `spawn_child` returns, before writing any stdin. Expose an identity-checked kill through the opaque engine API (see B5).
- **Startup reap:** reap before reading pending native events. Mark a stream Lost with a disposition of ChildKilled, ChildAbsent or ChildUnknown. A record with Launching and no recorded identity is UnknownLaunch, which is the existing state name, and the doc says so honestly.
- **Vanished client:** "Client disconnect does not cancel" currently also covers a client that never sends another message. Its child then blocks forever on a full output window and holds a quota slot. Add a server-side peer-idle expiry (longer than the client's 11 minutes, for example 15 minutes) that runs the cancel path and records a terminal ClientAbandoned.
- **Leader exit with a live group:** if the leader exits but a grandchild holds the stdout pipe open, EOF never comes. Reuse the engine's `drain_grace` TERM, then `cancel_grace` KILL after leader exit. State the setsid-escape limitation.

**B4. Daemon lifetime and count-drain are not reconciled with streams.**
- The service daemon has a hard `MAX_UPTIME` of 1800 s (`command_limits.rs`), and `max_commands` count-drain.
- A 30-minute cap silently kills any long rsync, which would be reported as Lost.
- Specify the behavior:
  - At expiry, Open stops admitting new streams.
  - Live streams are drained for a bounded grace period.
  - Then TERM/KILL, with a distinct terminal state ServerStopped rather than Lost.
  - Whether an Open counts toward `max_commands`.
  - The `count_exit_ready` and `publications_terminal` drain gates must include live streams.
- Alternatively, raise the stream-bearing daemon's uptime ceiling explicitly.

**B5. The exact engine live-child API is not defined, and revocation of live streams is not specified.**
- `ChildOwner` is private to `worker.rs` and is bound to the worker process identity (`self.worker`), so the shared safe ownership can't be reused as the doc implies.
- Define the minimal opaque `LiveChild` surface:
  - `spawn(prepared) -> LiveChild`;
  - `pipes()`;
  - `identity()`;
  - `signal_term` / `signal_kill` with the identity re-check;
  - `try_exit` without reaping;
  - `reap_last`;
  - `from_recorded_identity` for the startup reap in B3.
- Add a test that a child spawned from the multi-threaded daemon inherits only fds 0–2. `platform::pre_exec` already sweeps CLOEXEC, but it was written for the dedicated worker process and has not been verified in the daemon.
- Revocation: "check configuration on each operation" says what happens to new operations, not to a running child. State one rule, for example: an authorization failure on a live stream refuses the operation and also runs the cancel path, ending in a terminal AuthorityRevoked state.

## Non-blocking

- Extend `command_publications` with a protocol discriminator rather than adding a parallel table. `CommandProtection.publications` and the published/failed/refused/invalidation dispatch sites all key on `PublicationKey`, so a second table risks missing a site. `validate_command_publication` currently hard-codes the command KIND and TAG, so it needs a per-protocol variant.
- The IPC uses a 15 s `IO_TIMEOUT` and a 64 KiB frame. Say that stream exchange IPC returns within a bounded wait, well under 15 s, with an empty result if nothing is ready.
- Name default numbers for the per-principal and global live-stream quotas.
- Give the CLI exit status for a locally confirmed cancel and for an uncertain one (for example 130 and 255).
- The doc's title and acceptance list still describe rsync, which is 2b. Move the rsync items (push/pull/update/delete and rsync denial) under 2b. 2a should be proven with a native `cat`/`sleep`/`yes` helper, covering at least the cancellation and bounded-memory cases already listed.
- Add acceptance tests for B1–B5:
  - the idle message-rate bound;
  - cancel before Open;
  - cancel during Preparing and Launching;
  - a TERM-ignoring child killed after the grace period;
  - a grandchild holding the pipe open;
  - a crash after spawn followed by the startup reap;
  - peer-idle expiry;
  - uptime expiry mid-stream;
  - revocation mid-stream.

Once B1–B5 are in the doc, a re-review should be quick. This is my technical assessment only. I did not verify the stated authorization to implement.

## Round 2

**Technical verdict: APPROVE for phase 2a under the stated scope.** I made no edits or builds, and I didn't re-review phase 1. Each of B1–B5 is either resolved or honestly disclosed as a limitation, and the interfaces are precise enough to code against. Nothing here relies on the human queue gate, which you've said doesn't apply.

I checked the contract claims against the code:
- **B4 matches the config code.** `RawLimits::default` has `max_uptime: None`. `apply_limit_overrides` (`command_config.rs:282-300`) supplies `MAX_UPTIME` only when `max_commands` is set without an uptime. Explicit uptimes above 1800 s or equal to zero are rejected. The doc's description is accurate, including the count-limited test-service caveat.
- **B3's ownership argument holds.** `ChildOwner` (`worker.rs:53`) is private and tied to a worker identity. `signal` sends `kill(-pgid)` and refuses once reaped. Persisted identity can't safely authorize a signal after the daemon dies, because the probe followed by a numeric `kill(-pgid)` has a TOCTOU window. Declining a `from_recorded_identity` signalling API, and reporting a matching orphan as `ChildUnknown`/live-unowned, is the sound choice. Holding quota is a reasonable bound on repeated crashes.
- **B1, B2 and B5 are covered.**
  - **B1:** backoff of 250 ms → 2 s with an early wake, plus stated token buckets and slot caps.
  - **B2:** sequence-0 control lane, irreversible latch, tombstones, Preparing/Launching/Running rules, unreaped-leader KILL ordering, idempotent Cancel/Status retries, and `streams` described as a local projection.
  - **B5:** the `LiveChild` surface is specific enough to code against, with prepared exec, pipes taken once, diagnostic identity, `try_exit` without reaping, owned TERM/KILL, and `reap_last`. The multithreaded daemon FD-hygiene test is required.

I accept your semantics. Commands keep running on disconnect, with no best-effort cancel on local failure and no peer-idle auto-kill. Two consequences are recorded in the doc, and I read them as accepted rather than defects:
- A vanished client can leave a child blocked on its full output window. It holds one of the principal's four active slots until explicit cancel or server stop.
- A daemon crash can leave an orphan that the runtime won't reap.

**Non-blocking items to settle during implementation.** None of these needs another design round.
1. **Reconciliation command name.** "Explicit local administrative reconciliation" for `ChildUnknown` records needs a concrete CLI/IPC name. Decide it in the phase 2a code, and make sure its acknowledgement records possible surviving execution.
2. **Cancelling a Lost server record.** A remote Cancel for a Lost record with an unknown live child should return "cancellation uncertain" (line 153) and keep credit. Add that case to the acceptance tests.
3. **Cancel before Open under a full retained quota.** The doc has no reserved credit for a pre-Open tombstone. The client sees the refusal, so there is no false claim, but the test should confirm a refused tombstone never reports success.
4. **Stale wording.** Line 3 still reads "Draft revision responding to Claude B1–B5; not implementation approval". Update it on approval.
5. **Publication discriminator.** Line 214 says "preferred" for the protocol discriminator on shared publication machinery. Pick one in code, and make sure every published/failed/refused/invalidation site is covered if you use a separate store.
