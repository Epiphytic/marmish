# Native command lookup design review, round 1

Reviewer: actual Claude Code auto, `claude-sonnet-5-5`.
Session: `65b25207-91c8-4953-98f4-2aac2badad03`. Verdict: NOT APPROVED; no implementation started.

**Verdict: NOT APPROVED.** Six corrections are needed before coding. Reviewed by Sonnet 5.5 (`claude-sonnet-5-5`), read-only. I ran no Cargo or live tests.

## Blocking

1. **Canonical argv[0] breaks multi-call binaries.**
   - The design replaces argv[0] with the canonical path. The launcher uses `Command::new(argv[0])` with no `arg0` (`launcher.rs:1099`), so the program name and argv[0] are the same value.
   - Symlinked applets such as busybox, and uutils coreutils where `/usr/bin/ls` points at `coreutils`, dispatch on argv[0]. After canonicalization they would run as `busybox` or `coreutils` and fail.
   - Bare-name lookup makes these the headline use case.
   - Fix: pass the resolved path as the program and keep the requested spelling as argv[0]. That needs a request or `ChildSpec` change plus `arg0()` in the launcher, with the digest and policy semantics defined. If you decline that, document the breakage explicitly.

2. **Deny rules on alias paths stop applying.**
   - Today a non-canonical spelling is refused outright (`require_canonical_exec`), so it fails closed. A deny rule for `/bin/rm` therefore never has to match.
   - Under the design, `/bin/rm` resolves to `/usr/bin/rm`, and only the canonical path is matched.
   - Existing deny rules written against alias spellings silently stop matching.
   - Fix: also evaluate deny layers against the requested absolute spelling (deny on either spelling, allow on canonical), or require a policy-validation migration step. Either way, add a test.

3. **Policy denials leak execute tokens.**
   - Today the native precheck stops denials before `issue_execute_token`. The design removes that precheck, so denials reach `submit` after the token is issued (`command_engine.rs:565`).
   - A failed submit leaves the token live for 60s. The per-owner cap is 128 and the global cap is 1024 (`store.rs:679`).
   - A principal producing 128 denials or `command_not_found` refusals within 60s hits `QuotaExceeded`, which is persisted durably. That can starve legitimate commands, including those from other principals once the global cap is reached.
   - Fix: run policy and lookup before issuing the token (add an engine `check_policy` API), or revoke the token on a failed submit. Test the denial flood.

4. **The policy-budget failure has no wire code, and its engine mapping would retry forever.**
   - The design wants pattern-exhaustion failures to stop looking like `CommandDenied`, but the wire table has no code for them.
   - In `execute_operation`, any `submit` error other than `CommandDenied`, `InvalidRequest` or `QuotaExceeded` maps to `Missing` (`command_engine.rs:585`). A new `EngineError` variant would therefore be re-dispatched indefinitely.
   - Fix: define the wire code and detail, such as `command_resolution_failed` with a `policy_budget` or `policy_invalid` detail. State whether it is a durable refusal or an uncertain operational failure. Make the `execute_operation` match exhaustive.

5. **Launch-failure classification can't be done from spawn errno alone.**
   - Rust's `spawn` reports chdir, `pre_exec` and exec failures through one errno pipe. An ENOENT can mean a missing cwd, interpreter or target. A `pre_exec` EPERM looks like a filesystem permission failure.
   - So the `invalid_cwd` reason is not derivable as written.
   - Fix: preflight the cwd separately, and fold ENOENT/ENOTDIR into one ambiguous reason. Map `pre_exec` failures to `other_startup`, for example by wrapping them so they can be recognised.
   - Also specify the other pre-spawn `NotStarted` returns. The canonical-mismatch check at `launcher.rs:1071` happens when a symlink is swapped after admission. Under the current design it would still return NotStarted and exit 255 silently. Give it a `target_changed` reason.
   - `ChildSpawnError::NotStarted` needs a payload.
   - Shebang `#!/usr/bin/env x` failures under the sanitized `PATH=/usr/bin:/bin` will be common. The design says to keep that environment, so document that the reason will surface as "missing interpreter".

6. **Persistence and wire format aren't pinned down, so replay and rollback are unsafe.**
   - Engine rows are JSON with `user_version=1` (`store.rs:300,529`). A new `ExitOutcome::LaunchFailed` makes an older binary return `StorageFailure` on terminal rows. The design needs a schema version bump or a documented no-downgrade statement.
   - `store.finish` currently allows `Running && !NotStarted` (`store.rs:986`). As written, `LaunchFailed` would pass that branch. It must be rewritten to allow `LaunchFailed` only from `Launching`.
   - `wire::Outcome` is `deny_unknown_fields` with a required-nullable `value: Option<u8>`. Choose one representation: a new `launch_failed` kind plus an optional reason field, or an encoding in `value`. Whichever you pick, a new optional field must be omitted when absent so stored response bytes and hashes stay identical on replay.
   - `validate_error` (`command_wire.rs:560`) currently permits `detail` only on `NoncanonicalExecutable`. It must become an exact code/detail allowlist.
   - `validate_status` and the `command_cli.rs:442,466` exit mapping also need updating. The prototype crate (`lib.rs:612`) has an exhaustive `ExitOutcome` match too.

## Should fix (non-blocking, document or test)

- **Filesystem existence oracle.** Lookup runs before policy, so a restricted client can tell `command_not_found`, `command_not_executable` and `command_denied` apart for paths outside its allow list. Record this as an accepted disclosure, or order the checks to reduce it.
- **Hung PATH entries stall the engine thread.** There is no timeout, only a component bound, so an NFS or FUSE entry blocks all status operations too.
- **Transient lookup failures become permanent.** A durable `filesystem_io` refusal replays forever for that invocation. Say so, or treat EIO as uncertain.
- **Admission is now asynchronous for denials.** Denied submits are now accepted first and refused later through `status`, `poll` or `wait`. They consume admission budget and engine queue capacity until cleanup. Test that quota and cleanup cover refusal rows (`validate_cleanup_row` already allows them).
- **PATH can change between submit attempts.** If the daemon restarts with a different PATH before the engine has recorded the invocation, resolution can differ on retry. This is safe because the engine has no record yet, but state it.

Everything else holds up against the source:
- The engine status lookup before dispatch protects known invocations from re-resolution.
- The native digest uses the original argv.
- The existing refusal path (`Fact::Refused`) is reusable.
- `NoncanonicalExecutable` is retained for decoding.

Re-review after the design is corrected.
