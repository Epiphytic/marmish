# Prepare Command on the registered candidate owner

This bounded adapter supplies the Command startup turn in
[daemon composition](peer-candidate-daemon-composition.md) and
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md).
Bead `marmish-dwt.4.12.2` tracks the wider composition. Apply this source
against `10c4249`. The exact concurrent service-admissions plan is
`e072a5a:docs/phases/peer-candidate-run-admissions.md`. The candidate entry uses
the existing engine gate (`any(test, marmish_peer_receiver_candidate)` on
Linux x86-64). The daemon caller mounts separately. The entry can temporarily
carry a scoped unused-code allowance with that reason.

## Startup and ownership

- Retain `MdkOwner::enable_commands` and its existing connector mode and URL,
  account, profile, and duplicate service validation. Add an internal candidate
  entry point of the same type with an extra owned `command_engine::Admission`.
  Only a future registered caller supplies that admission, derived from its
  sole original registration. Clone that exact Admission for
  `CommandEngine::start_candidate`; both instances observe the same Fatal and
  verification and custody callbacks.
- Use one shared setup body. Independently recheck before each of
  `command_historical_principals`, `command_configure`,
  `command_bind_engine`, and, for an engine, `command_recovery` and
  worker spawn. Preserve the order and error behavior of command policy,
  inventory policy snapshot and command identity derivation. The first admit
  follows the existing connector/account/profile/duplicate checks. Policy
  assembly and key derivation are in memory, and layout resolution performs
  only filesystem reads. These steps need no separately admitted durable turn.
  Both wrappers pass the same lazy `FnOnce() -> Result<PathBuf>` to resolve
  the canonical helper from `native_layout::resolve(&current_exe()?)`. Invoke
  it only after successful bind and only with an engine policy. Construct
  engine settings, admit recovery, read recovery, admit worker entry and
  immediately spawn. A Client with no engine has no layout or recovery step.
  The adapter adds no opener, relay activity, storage format or privilege.
- An explicit pre-call or final frontend check refusal returns
  `Error::Storage("candidate_dispatcher_lost")`. Existing binding validation,
  guarded journal errors (including `dispatcher_admission` where emitted),
  layout errors and actual thread-spawn errors keep their original mapping.
  The caller selects physical shutdown using exact Fatal, regardless of which
  error surfaced. An already-admitted synchronous call can finish.
  A successful spawn may have started the worker before any later frontend
  check. Install the returned engine into the new Command service and assign
  that service immediately to the owner, with no intervening fallible action.
  Then perform a final frontend admission. The client-only path still admits
  its journal calls and final exposure, with no launcher or recovery.
- On any candidate error the owner is stop-only. The caller preserves the
  owner and existing `ServiceLifetime` through disposition and must avoid
  progress, IPC transfer, room work, fallback or retry. On success the caller
  attaches the existing service lifetime before ordinary work, as in stock.
  On failure the physical path leaves the service lifetime unset because
  `take_engine_for_stop` and `into_fatal_join` need only the owned worker and
  Fatal. After successful spawn, the returned engine is extractable and
  joinable even if the final check refuses. No daemon mount, startup report,
  Terminals or Peer wiring is added here.

## Qualification

Run isolated Linux tests with the real profile, identity, MDK owner and
SQLCipher Command journal. A test-owned Admission deterministic cut before
each frontend boundary checks the exact error, durable configuration and
binding prefix, and absence of a later launch. Check the late cut after
actual Command-engine spawn by retaining and physically joining the
owned engine, with an independent barrier observing its scheduled thread.
Preserve unrelated admitted workers, production identity and global process
state. Use the same layout resolver with a private fixture path in that
focused engine test because the Cargo libtest image name is intentionally
outside the accepted layout. The test changes only the lazy input and covers
the development sibling layout. Exercise both actual entry points without a
worker for the stock/ordinary candidate client case. Also check that a failed
helper resolution happens only after successful configure/bind, and that
a healthy stock server starts its real Command engine through the shared body.
Keep tests local and bounded. Physical default VFS displacement and actual installed Command
execution through WSS remain qualification of the assembled daemon.
Run focused tests, candidate and stock compile/lint checks, and independent
Claude Code Auto review of the exact source before composition.
