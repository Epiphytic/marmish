# Credential timeout fixture amendment code review

Actual Claude Code, `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `94baa049-0c53-418e-b37e-5e445b23dbf3`.

**Approved.** The final amendments meet the approved conditions in `/tmp/marmish-8r4-fixture-design.json`. I found no blockers. This was read-only: I ran no Cargo, tests or live processes, and I'm not claiming a root cause. I'm Claude Sonnet 5.5 (`claude-sonnet-5-5`), in a fresh-context session.

**`native_helper_timeout_and_oversize_are_bounded`** (`identity.rs:1285-1321`)

| Condition | Result |
|---|---|
| Timeout and oversize use separate helper files | Met: `timeout-helper` and `oversize-helper`. Neither file is rewritten after being executed. |
| Request-consuming prefix that can't hang | Met: both scripts start with `/bin/cat >/dev/null`. `controlled_helper_exchange` drops stdin once the 2-byte request is fully written (`identity.rs:886-888`), so `cat` sees EOF. The `head -c 9000` and `sleep 10` steps only start after that. |
| Absolute binary paths | Met: `/bin/cat`, `/bin/sleep` and `/usr/bin/head` are used, so nothing depends on `PATH`. `env_clear()` means no `PATH` is set. All three exist on this host. |
| `Result` Debug and `start.elapsed()` in assertions | Met: the result is bound first and printed in the timeout `Timeout` assertion, the `< 2s` assertion and the oversize assertion. `Error` derives `Debug` (`error.rs:3`). |
| Contract unchanged | Met: 60 ms `Timeout`, elapsed under 2 s, and `helper_oversize` within a 2 s deadline. |

- **Oversize path:** `head -c 9000` produces more than `HELPER_LIMIT` (8192). The read loop rejects it at `identity.rs:902-904`, so `helper_oversize` is the expected result.
- **Timeout path:** `exec sleep` is killed and reaped by `OwnedHelper::drop` (`identity.rs:826-827`).

**`write_new`** (`identity.rs:595-605`)
- It opens with `create_new`, `O_NOFOLLOW` and `O_CLOEXEC`, then runs `write_all` and `sync_all`.
- The handle `f` is dropped when the function returns, so it is closed before the caller does `set_permissions` and spawns. That satisfies the explicit-close condition.

**`helper_exchange` EOF contract** (`identity.rs:830-914`)
- The deadline is checked at the top of every loop iteration.
- Stdin is closed once the request is fully written.
- Completion needs both stdout EOF (`Ok(0)`) and an exit status.
- An ordinary helper that exits non-zero with empty output returns `keychain_failed`.

**`command_cli.rs`**
- `.and(status.outcome.as_ref())` is equivalent to the previous lazy closure. The argument is a cheap `as_ref()` with no side effects. `terminal` is still consumed, as before. The `LaunchFailed` filter and the `.and_then(|o| o.value)` chain are unchanged.

**Non-blocking notes**
- The comment at `identity.rs:1288` says `write_new` closes the handle "before either helper is spawned". That isn't literally true, because the oversize helper is written after the timeout helper has already run. What holds is that each file is closed before its own spawn. The comment is cosmetic.
- A residual `ETXTBSY` window may remain. Another test thread's `fork` could briefly hold a copy of this thread's write fd before that thread's `exec` clears `O_CLOEXEC`. The separate files don't remove that window. If the repeated parallel runs show `Text file busy` in the assertion output, the follow-up is a test-only bounded spawn retry or a unique filename, not a production change.
- A pass from the repeated runs would be evidence, not proof, that the flake is gone.

Disposition: approved. Correct the cosmetic close-before-spawn comment; retain diagnostic uncertainty and original timing bounds. Repeated fixture tests are root-owned.
