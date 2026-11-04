# Native command lookup design review, round 2

Actual Claude Code `--permission-mode auto`, model `claude-sonnet-5-5`.
Session `8d6a6823-6d22-47b3-b259-d52d798db1e3`. Verdict APPROVED before implementation.

**APPROVED**

I found no remaining blockers. I read the design, review r1 and the current source, and ran no code or tests.

The six r1 blockers are each closed by the frozen sections, and the source supports every fix:
1. **argv[0] and multi-call binaries:** `ResolvedExec{executable, invoked_path, argv}` with `arg0(argv[0])` fixes this. The current launcher at `launcher.rs:1099` uses `Command::new(argv[0])` with no `arg0`, so the change is needed and fits there. The digest covers all fields.
2. **Alias deny rules:** deny layers are matched against both the canonical target and `invoked_path`. Allow layers are matched against the canonical target only.
3. **Token leak:** `check_execute_request` runs before `issue_execute_token`. The existing code at `command_engine.rs:565` issues the token before `submit`. The design requires a test of more than 128 consecutive denials.
4. **Policy failure and re-dispatch:** `command_policy_error` is a durable refusal with no detail. This removes the `_ => Missing` fallthrough at `command_engine.rs:585`.
5. **Launch classification:** `spawn_child` currently returns `NotStarted` at 9 sites (`launcher.rs:1071-1122`). The design gives `ChildSpawnError::NotStarted` a classified payload. It makes spawn ENOENT/ENOTDIR ambiguous (reason 2 or 7), maps canonical mismatch to reason 6, and keeps cancellation and the `attempts >= 3` path as `NotStarted`.
6. **Persistence and wire:** `store.finish` at `store.rs:986` currently allows `Running && !NotStarted`. The design replaces this with `is_startup_failure`, legal only from `Launching`. The wire carries a single `launch_failed` kind with an explicit 1..7 value map, and `validate_error` becomes an exact code/detail allowlist. The design states there is no downgrade guarantee.

Non-blocking notes for the implementer:
- Rust's spawn error pipe carries only an errno, so the `pre_exec` tag has to be a reserved synthetic errno value or an equivalent encoding. It cannot be a side-channel flag. Tests should show that a `pre_exec` EPERM or ENOENT is never reported as reason 1 or 2.
- The early "Resolution contract" and "Policy" sections still describe the superseded canonical-argv replacement. The "Round 1 correction" section marks them superseded, but the implementer should follow the frozen sections.

Reviewer: Claude Code, Sonnet 5.5 (`claude-sonnet-5-5`), read-only.

Implementation disposition: frozen sections govern. Finite wire error and launch
value tables are fixed; engine and wire/CLI agents have disjoint ownership.
Root integrates native admission, durable completion diagnostics and dispatch.
