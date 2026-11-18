# Prepare Command on the registered candidate owner

This bounded adapter supplies the Command startup turn in
[daemon composition](peer-candidate-daemon-composition.md),
[registered service admissions](peer-candidate-run-admissions.md), and
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md).
Bead `marmish-dwt.4.12.2` tracks the wider composition. Apply this source
against `10c4249`. The Linux x86-64 candidate build can compile the entry
point before any daemon caller is mounted.

## Startup and ownership

- Retain `MdkOwner::enable_commands` and its existing connector mode and URL,
  account, profile, and duplicate service validation. Add an internal candidate
  entry point of the same type with an extra owned `command_engine::Admission`.
  Only a future registered caller supplies that admission, derived from its
  sole original registration. Reuse it across frontend and worker, with a
  clone of the same observer for `CommandEngine::start_candidate`.
- Use one shared setup body. Independently recheck before each of
  `command_historical_principals`, `command_configure`,
  `command_bind_engine`, and, for an engine, `command_recovery` and
  worker spawn. Preserve the order and error behavior of command policy,
  inventory policy snapshot, command identity derivation and canonical
  native helper resolution. This phase supplies the same canonical
  current-executable layout resolver in both wrappers. There is no new
  opener, namespace, connector, relay activity, storage format or privilege.
- A refusal reports the existing typed
  `Error::Storage("candidate_dispatcher_lost")`. The normal setup calls retain
  their own error mappings. An already-admitted synchronous call can finish.
  A successful spawn may have started the worker before any later frontend
  check. Install the returned engine into the new Command service and assign
  that service immediately to the owner, with no intervening fallible action.
  Then perform a final frontend admission. The client-only path still admits
  its journal calls and final exposure, with no launcher or recovery.
- On any startup error the future mounted caller must keep the original owner
  and its existing `ServiceLifetime` through physical disposition. A refusal
  after installation can still leave an engine to extract via
  `take_engine_for_stop` and join through `into_fatal_join`; the adapter itself
  does not close SQLCipher, detach the worker, downgrade to stock, tear down
  state or extend any lifetime. If registration is lost at a rejected durable
  call, the shared Fatal remains the authority for selecting physical shutdown.
  No daemon mount, startup report, Terminals or Peer wiring is added here.

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
outside the accepted layout. Exercise both actual entry points without a
worker for the stock/ordinary candidate client case. Keep tests local and
bounded; physical default VFS displacement and actual installed Command
execution through WSS remain qualification of the assembled daemon.
Run focused tests, candidate and stock compile/lint checks, and independent
Claude Code Auto review of the exact source before composition.
