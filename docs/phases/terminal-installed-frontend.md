# Positive native Terminal foreground qualification

The Linux private WSS writer fixture already covers the real Terminal
transport, PTY backend, tmux and same-user socket. Its attach and input
come from a framed test client. The quiet AUTH test runs the actual
`marmish terminal` CLI and checks early refusal. Add one ignored
positive test that traverses the foreground CLI on this same fixture.
This increment changes only tests and evidence. Keep the core self-hosted
selection unchanged.

## Fixture and assertions

- Extend the existing `prove_writer` workload matrix. Reuse the fixture
  client/server accounts, exact Command room, TLS roots, launch paths,
  named config, same-user socket and worker, plus existing session and
  tmux cleanup. Keep ordinary `cat` as the before/after control. Set the
  fresh client config's explicit single WSS route and pinned server key
  as in the quiet test. The test TLS roots stay in the fixture, with no
  route policy change or new trust environment variable in the CLI.
- Reuse `open_cli_pty` and a common private CLI spawn helper. Start the
  real sibling `marmish` with `--state-dir`, `-i c` and `terminal` for
  the exact fixture server npub. Child stdin/stdout share the slave.
  Claim it as controlling TTY using the existing `setsid` / `TIOCSCTTY`
  sequence. The parent retains its slave descriptor only to inspect
  terminal modes. Start without `--debug`. Drain stderr concurrently with a fixed
  memory limit. Require empty stderr on success. Own the child's handle
  and reap on success and after a bounded timeout or fixture failure.
- Set the parent master nonblocking and use Tokio `AsyncFd` read/write
  readiness against the owned file. Push writes with partial-write
  handling and fixed deadlines. Scan each output step within 32 KiB. Use distinct step markers and
  clear the scan state after each match. Keep draining the master until
  the child is reaped, including after sending the exit command. Do not
  print raw terminal bytes on failure.
- Wait for both the one expected tmux socket to exist and the foreground
  to apply raw TTY mode before sending input. Replace the remote login
  shell with the existing deterministic fixture `/bin/sh -i` command
  and match its split prompt marker. Send a printable single key such
  as `x` after observing local raw mode with ECHO and ICANON disabled.
  Confirm return of that key from the remote PTY, then clear remote
  input and print a synthesized marker containing spaces, shell
  punctuation and UTF-8. Assert that the complete output marker does
  not occur in the typed command. Run the ordinary Command control during or
  after the attachment. Send `exit 0`, require success from the front
  end, and compare all four termios flag words, the control bytes, the
  line discipline on Linux, both speeds and the winsize to the
  original values. Existing writer tests retain separate restart and
  takeover checks. No new runtime behavior depends on this test.

## Validation

Run the new ignored selector once against native built binaries and
once with the three matching executables installed from the shell script
and checksum-verified against package inputs. The frozen test executable
can sit under a private `deps/` directory beside the installed payload,
matching the existing test lookup. The latter remains a private WSS test
fixture with injected TLS trust and prepared local owners. It is not
external relay qualification or macOS validation. CLI-level resize,
interrupt, detach, paste and takeover remain outside this test.
Record pass counts and individual attach/key-echo wall times as samples,
plus any failure disposition. Rerun the exact quiet refusal fixture after
sharing its CLI spawn path. Run strict native installed-checks Clippy and
have Claude Code Auto review the testing diff and results before publish.
