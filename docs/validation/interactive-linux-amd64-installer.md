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

A second independent run used the documented `marmish SERVER_NPUB -- …`
form. The client used its configured White Noise US relay without a host
suffix. The same literal was returned byte-for-byte with empty stderr and
exit 0 in 9.581 s. The client and server reported active in 903 and 916 ms,
respectively. Both daemons exited 0 on shutdown. These two ordinary-command
timings are individual samples.

## Copy and streaming regressions

On the same Linux AMD64 source revision, the local `rsync_cli` selector
passed both command parser and bootstrap tests. `copy_integration_tests`
passed 14 tests with two opt-in cases skipped. `stream_cli` passed all 12
cases. The rsync selector checks command handling. It does not launch an
end-to-end rsync transfer through the installed client and server.

## Core workflow replay

All 32 Cargo selectors from `.github/workflows/interactive-core.yml` ran
against `a1c655914a2f9ea31b7f8cba0a443dc29b21c5bd` in a detached Linux
AMD64 checkout using the local build cache. Each matched at least one test.
The suite recorded 149 passing test executions. Some selectors
overlap. These numbers cover a local replay of the workflow test commands.

## Current upstream MDK checkpoint

The optimized Rust 1.97.1 binaries were rebuilt from
`a9780fd8c875524a0f489222aef29705a3a36cad` with upstream MDK master
resolved to `122ad45ece73c1d6f687a4e92a541b9b2febde34`. The two-job
incremental release build took 259.74 s on Linux AMD64. Each executable
required glibc 2.39 or earlier, reported the expected native provenance,
and was packaged with the inventory generated from that lockfile.

| Installer check | Observed result |
| --- | --- |
| File-backed quickstart, verified binaries, repeated setup, and payload tamper refusal | Passed on both attempts |
| Typed closed-loopback startup refusal and cleanup | Passed in 11.991 s |
| Active installed daemon on White Noise US, profile-lock refusal, and clean shutdown | Passed in 13.750 s |

Both runs used a fresh installation root and the repository's shell installer
rendered with locally packaged 0.3.1 assets. The installed executable and
utilities ran without a Python runtime. The positive path checked ordinary
daemon reachability. It sent no Peer enrollment or heartbeat records.

The separate Linux AMD64 native suite built with the same MDK revision ran
954 tests successfully in 1,364.77 s, with 107 opt-in tests ignored. The
runner used an immutable copy of the native test executable and stable
sibling fixture binaries. The
[MDK preflight evidence](mdk-master-schema-213.md) documents a prior
inconclusive run caused by replacing the executing harness.
