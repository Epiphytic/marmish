# Interactive branch Linux AMD64 installer validation

The installer checks used source revision
`3721bf52aff085de9203d31b75d3f8f76ca45967`. The temporary local payload
manifest and the candidate's `release-info` both identify version 0.3.1
and target `x86_64-unknown-linux-gnu`. The installer used its local asset
directory rather than a network download.

## Installer and quickstart

| Check | Observed result |
| --- | --- |
| Quickstart with an explicit relay | Passed in 37.655 s. All five PTY prompts completed. |
| Quickstart with the default relays | Passed in 35.568 s. All five PTY prompts completed. |
| Identity reuse | Both repeat setups returned the same public identity without reopening the owner. |
| Default role and grants | Both quickstarts configured both roles and left inbound execution disabled without an explicit grant. |
| Same-length payload tamper | The qualification harness recorded a tamper refusal. |
| Closed loopback relay | The installed foreground daemon refused startup and the qualification harness confirmed runtime cleanup. |

The quickstart runs used the file key backend and a runtime PATH without
Python. Their setup selected relay URLs and completed without network
activation. They removed their owned state. This evidence covers local
asset installation on Linux AMD64. The download path, keychain, Tor,
Linux ARM64 and macOS ARM64 remain separate validation steps.

For interactive transport, see the
[Linux AMD64 native WSS fixture](terminal-installed-runtime.md) and
[Observe and Takeover fixture](terminal-observe-takeover-runtime.md).
Those fixtures exercise native runtime IPC, PTY and tmux with a private relay.

## Installed ordinary command

A separate installation from the same temporary local asset directory ran one ordinary Command
through `wss://relay.us.whitenoise.chat/`. The two foreground daemons used
fresh file-backed identities. The server granted the client's npub and allowed
only `/usr/bin/printf`. The client explicitly trusted the server and supplied
its canonical npub with `-u`. Both daemons reported active, at 903 ms for the
client and 913 ms for the server. The installed client ran
`/usr/bin/printf %s` with one literal argument containing quotes, shell
substitution syntax, wildcard characters, a backslash, a newline and Unicode.
It returned the exact UTF-8 bytes with empty stderr and exit 0 in 5.270 s.
Both daemon processes exited 0 on shutdown, and the test removed its local
profile state. The timing is one successful cold ordinary-command sample.
An earlier invocation was rejected with `invalid input: command_separator`
before command execution. The successful invocation used the explicit
`-u` form.

## Core workflow replay

All 32 Cargo selectors from `.github/workflows/interactive-core.yml` ran
against `a1c655914a2f9ea31b7f8cba0a443dc29b21c5bd` in a detached Linux
AMD64 checkout using the local build cache. Each matched at least one test.
The suite recorded 149 passing test executions. Some selectors
overlap. These numbers cover a local replay of the workflow test commands.
