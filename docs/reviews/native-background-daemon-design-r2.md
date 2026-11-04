# Native background daemon design review r2

Reviewer: actual Claude Code `claude-sonnet-5-5`, auto permission mode.
Session: `02067852-207d-4f0a-8f09-0f553f711770`.

**Verdict: APPROVED.** I found no unresolved blocking flaws, and implementation may proceed.

**Reviewer:** Sonnet 5.5 (`claude-sonnet-5-5`), a fresh-context session reviewing the revised design (design only, no edits, no Cargo). I did not read the prior session `f0877561…`. I worked from your B1–B6 summary and checked each against the doc and the code. I did not write the verdict to the bead. Say if you want it recorded there.

## B1–B6 against the code

| Item | Result | Evidence |
|---|---|---|
| **240s startup / 45s cleanup** | Resolved | `daemon.rs:open` budgets 30s for credentials. `serve_owner` budgets 150s for `activate`, and `stop_owned` allows 30s. 30+150 leaves about 60s margin under 240s, and 45s covers the 30s stop with slack. The doc admits that SIGTERM before the handler is installed in `serve_owner` just kills the child. |
| **Autostart concurrent winners** | Resolved | `ProfileLease::acquire` (`identity.rs:~105`) fails fast with `profile_in_use` from a non-blocking `try_lock`. The design continues polling on that error in autostart only. The listener binds before activation and reports `"activating"`, so `command_socket` already treats a second starter correctly. |
| **Append flags / nonblocking cleared** | Resolved | `identity.rs:open_private` uses `O_NONBLOCK` and no `O_APPEND`, which is exactly the hazard the doc names. A dedicated opener is the right fix. |
| **fd 3 channel** | Resolved | Close-on-exec ends, `dup2` to 3 in `pre_exec`, marker removal, and socket-type, peer-uid and parent-pid validation. The two-way READY/ACK closes the parent-death gap. `ProfileLease::drop` already does an explicit `unlock`, so a leaked fd cannot hold the flock. |
| **Early-main finite errors** | Resolved | `cli::main` has finite `eprintln!` exits for layout and runtime build before and after the point where Tokio is constructed. Retaining a reporter clone covers those. |
| **Supervisor inventory** | Resolved | Verified spawns: `installed_matrix.rs:130`, `installed_autoexit.rs:618`, and `installed_version_lifecycle.rs:~1909` (the negative case, with `--max-uptime 15 daemon`). The Python tooling is frozen v0.1, so excluding it is correct. |

## Nonblocking refinements (address during implementation)

1. **fd 3 collision.** If the socketpair child end already lands on fd 3, `dup2(3,3)` is a no-op and leaves CLOEXEC set, so exec closes the channel. Handle that case with an explicit `fcntl` clear, as the doc already says to "clear CLOEXEC only there". Add a test that forces the source fd to 3.
2. **Client budget.** The worst case is 240s plus a 45s grace, about 285s. State the autostart deadline as a number and make sure it exceeds the parent's own bound, so the client doesn't give up while the parent is still cleaning up.
3. **Ack read in the child.** Specify the short ACK deadline as a number. Do the wait on a blocking thread or with an async timeout, never as a bare blocking read on a Tokio worker.
4. **Readiness placement.** The first active snapshot is published inside the `serve_owner` loop, after `command_count_exit_ready()` is checked. Notify readiness only after that first `send_replace`. Exiting from the exit-ready break before it, as with `max_commands`, must be reported as not ready.
5. **Errors from `open()`.** `run` returns early with `?`, without calling `stop`. "Joined owner cleanup" there means whatever `MdkOwner` drop already does. Add a test with an `enable_commands` failure and confirm the error frame is sent only after the owner is dropped.
6. **Behavior change to document.** Today an autostarted daemon survives the client dying mid-startup. Under the new ACK rule it will be torn down. This is the intended outcome, but put it in the README and spec.
7. **Closing descriptors above 3.** macOS has no `close_range`. Specify a `/dev/fd` scan or an rlimit loop that doesn't allocate after the `fork` boundary, or do it at single-thread child entry as the doc says.
8. **Worker and provider stdio.** The vault helper at `identity.rs:851` already uses piped stdin/stdout and null stderr. Confirm the same for `marmish-engine/src/launcher.rs` (not read in this review), and add a log-content assertion with a sentinel command output.

## Implementation disposition

Approved before implementation. ACK deadline is 5 seconds on a blocking worker; client startup budget is 300 seconds, covering 240-second launch and 45-second cleanup. The fixed fd3 collision and private-output sentinel are acceptance cases. Source documentation distinguishes published 0.2.2 semantics. Worker/provider stdio audit confirmed null or piped descriptors, not inherited daemon output.
