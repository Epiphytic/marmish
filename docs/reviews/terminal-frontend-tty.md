# Foreground Terminal review

Claude Code auto mode approved the third design review in session
`8d7c9e3b-0cd3-40cf-ab8c-1fb072a211ec`. Scope was the foreground PTY,
closed terminal byte allowlist, paste-aware local detach, signal handling
and bounded termios and display recovery. Daemon and relay wiring belong
to the later [activation phase](../phases/interactive-peers-roadmap.md).

## Code disposition

Claude Code auto session `2a51632f-634e-4349-ade4-a4048a2336a3` first
returned **REVISE** and then **APPROVE** on the frozen corrected source.
The review inspected the foreground loop, display, signal and TTY guards,
input/output parsing, opt-in native fixtures and focused CI selector.
The review findings were:

- A literal tilde followed immediately by a bracketed-paste start now
  preserves the paste transition. Writer and observer regressions check
  exact bytes and detach after the paste.
- Clippy warnings were resolved. The real two-PTY bridge now asserts the
  entire rendered byte stream and fixed cleanup, while the tmux fixture
  checks the blocked control sequences and terminal restore.
- Actual frontend coverage now includes geometry notification, suspend,
  SIGHUP, saturated input plus SIGTERM, and IPC loss. IPC loss abandons
  uncommitted bytes, reports interruption and restores the terminal.
- The result reports an unchanged display as restored before raw mode.
  The one-shot termios restoration and foreground-gated Drop attempt
  remain, with no unbounded terminal operation.

The reviewer independently passed 14 core tests, 21 scoped tests on
three runs and strict Clippy. The [validation report](../validation/terminal-frontend-tty.md)
adds release-profile timing, throughput and Linux PTY syscall counts.

Remaining integration coverage includes mixed foreground process groups,
background SIGTSTP, stale SIGCONT in the active loop, SIGTTOU during
tcsetattr, sustained SIGWINCH scheduling, termination during suspend
cleanup, real partial TTY writes, resize after queued input, descriptor
reuse and real foreground-gated fresh attachment on resume. The exact
bridge stream checks intentionally account for its libtest prelude.
The tmux capture checks representative controls and does not claim
complete terminal emulator behavior. Native macOS and ARM64, authenticated
orderly close and output commit, and full remote CLI latency need live
activation evidence.
