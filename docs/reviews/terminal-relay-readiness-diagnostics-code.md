# Terminal relay readiness diagnostics code review

The reviewed change reports the fixed local category
`terminal_relay_auth_challenge_timeout` when the verified Terminal bootstrap
inbox completes its full wait without observing a NIP-42 challenge. The
approved [phase design](../phases/terminal-relay-readiness-diagnostics.md)
keeps existing authenticated Terminal eligibility and explicit relay selection.

## Initial review

- Claude Code `--permission-mode auto`, `claude-opus-5-5`, session
  `e4decb26-5a06-4e1c-be1f-85b7816f241f`.
- Exact source `c1f5c22dde704ae692dea84bd3d9649b192855dc` compared with
  `01255066ce44a54b4e3455ab6d6f3c7c65839248`.
- **APPROVE** with no blocking findings. Claude inspected all changed files,
  traced the current source and deadline check, verified each raw
  observation inhibitor and checked the fixed category through IPC and
  post-request takeover ambiguity. It checked formatting for the changed
  Rust files and inspected the author's local WSS logs.
- Incorporated the suggested explicit precedence for an observed AUTH and
  documented the enclosing attach deadline and mixed-version behavior.

## Combined review

- Claude Code `--permission-mode auto`, `claude-opus-5-5`, session
  `3eae2b7d-dfc7-4f1c-9589-94a5174b6c73`.
- Exact code `a7c26e7ae433f5c75ce4a2ada5c01dfa7ecd794e` includes the two
  openpty portability changes in `cbe1cd0` and `79513cb` and the follow-up
  to the initial review.
- **REQUEST CHANGES**. The existing standalone frontend PTY passed a
  const null termios pointer to `openpty`. The Darwin declaration also
  requires mutable termios, independent of the corrected winsize argument.
  Claude checked the mismatch with a standalone Rust metadata compile.
  It affirmed the installed readiness fixture, AUTH precedence, mixed-version
  text, existing auth eligibility and no confidential diagnostic data.

## Final review

- Claude Code `--permission-mode auto`, `claude-opus-5-5`, session
  `6bac6faf-2425-46a8-ae8b-d20c06536224`.
- Exact source `795afcc74e5a75409ddb0d485834a5b4dc19f3f1` includes
  `828ed2c` for the termios pointer and `795afcc` for an explicit
  observed-AUTH assertion and incomplete-AUTH usage text.
- **APPROVE** with no blocking findings. Claude checked all `openpty`
  argument positions, window-size ioctl and descriptor ownership. It
  compiled the exact installed fixture PTY helper with the locked libc
  source as standalone Rust metadata for Linux x86_64 and both tested
  Darwin architectures. The fixed helper type-checked on all three.
  The old const null termios pointer was rejected by both Darwin types.
  Claude also checked the AUTH precedence assertion and the revised usage
  text against the existing timeout mapping, verified the changed files
  with rustfmt and inspected the matched Linux test logs.

The actual [author validation](../validation/terminal-relay-readiness-diagnostics.md)
covers the native private WSS fixture, same-user IPC, the installed CLI
and controlling PTY. Additional direct transport cases for an unprocessed
replay marker, unrelated source, partial AUTH or caller EOF at the exact
timer boundary remain outside the live WSS fixture's scope. Existing
deterministic and control coverage is described in the validation report.

## Integrated upstream MDK review

Claude Code Sonnet 5.5 in Auto mode (session
`b5bbad2e-2b2b-405b-86e3-34b2ec5639d2`) approved integrated source
`d8c85de` using MDK `b3cd2cb1`. The reviewer inspected the code and read
the controller's private WSS test and Clippy results, without running
duplicate tests. The relay fixture module declaration in `mdk_adapter.rs`
is explicitly test-only.

Readiness available at the timer poll wins that decision. If readiness
arrives after the timer has already fired and before the final provenance
check, the expired attempt returns the existing timeout. The client
cannot use that late readiness to reopen the expired attempt.
