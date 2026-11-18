# Upstream MDK recovery refresh

Source `d091cc81125036a1bfcb04723d44e05c47ea9594` updates the ten MDK
entries in `Cargo.lock` from `b3cd2cb1` to `bdf9833e`, the upstream
`master` head checked during validation. The workspace manifest still
follows `master` and all other resolutions are identical. The upstream
change covers empty polling and route recovery status in `marmot-app`,
which is absent from Marmish's locked dependency graph.
[Upstream MDK commit](https://github.com/marmot-protocol/mdk/commit/bdf9833e340909f29c628f4fc2f9a3d6873f1cf5)

Claude Code Sonnet 5.5 in Auto mode approved the exact source in session
`cf00a1bb-35d0-4289-b29c-b4961797c34b`. The reviewer independently checked
the lockfile diff, upstream branch head and public compare, and linked
package names. The review was static. The following tests were run by
the integration author on Linux AMD64 with locked offline dependencies.

| Check | Result |
| --- | --- |
| Locked metadata and normal native binaries | Passed. Binary build 44.34 s |
| Native relay-qualification library cases | 15 passed in 10.19 s |
| Native controlling PTY qualification against private WSS | Passed in 0.59 s |
| Recipient Offer malformed-route cursor regression | Passed in 0.50 s |
| Strict installed-checks Clippy, all targets | Passed in 53.49 s |
| Native interactive writer and ordinary Command against private WSS | Passed in 70.39 s |

The writer fixture used real tmux, Linux PTYs, SQLCipher and separate
loopback TLS and IPC sockets. This run measured cold and warm attach at
8.198 s and 8.965 s, key echo at 161.508 ms and ordinary Command times
of 5.819 s before, 3.510 s during and 4.230 s after the attachment. Each
metric has one sample. Other compilation was active on the test host,
and the local relay does not represent internet latency.

The 36 test filters defined in `.github/workflows/interactive-core.yml`
were replayed locally using the workflow's script. All filters matched
passing tests. The 160 total executions completed in 169.21 s, including
local compilation. This was a local replay of the self-hosted workflow's
selection. macOS and Linux ARM64 were outside the run.

## Optimized Linux installer and private Terminal

The optimized package built from the same source and lockfile on Linux
AMD64. All three packaged executables reported a highest required glibc
symbol version of 2.39. The package was 82,076,776 bytes. The native
release build took 292.341 s and the locked license inventory 15.302 s.

| Installed check | Result |
| --- | --- |
| Shell installer and quickstart with local release assets | Passed in 8.454 s. Verified the three executables, repeat identity setup, tamper refusal, typed closed-relay failure and cleanup |
| GNOME Keyring in a private D-Bus session | Passed in 23.809 s. Verified default provider selection, two named identities with four secret items, fresh reload, locked refusal, reload after unlock and file storage when the provider was absent |
| Terminal writer and concurrent ordinary Command | Passed in 54.17 s |
| Named Terminal takeover, resume and observer | Passed in 86.63 s |
| Terminal restart and reattach | Passed in 42.91 s |

The Terminal fixture installed the package using the shell installer and
compared all three executable payloads byte for byte with the build
inputs. The debug test harness used the installed worker, real tmux and
PTYs, private WSS with an injected CA, and prepared in-process local
owners reached over same-user IPC. The writer run measured 6.606 s cold
attach, 8.246 s warm attach and 145.249 ms key echo. Ordinary Command
took 2.786 s before, 3.060 s during and 2.629 s after attachment. Each
metric has one sample. This run covered the native Terminal backend
through the framed fixture client. The positive foreground CLI,
standalone daemon relay setup and external relay behavior require
separate evidence.

An earlier full Linux AMD64 library run on
`ffb9b219cf9c4e3a24634bcae4c4f4428c2b1bb6` with MDK `b3cd2cb1`
passed 1,018 tests with zero failures and 118 opt-in tests skipped in
1,415.69 s using two test threads. This run predates the Offer,
qualification issuer and quota changes. The focused tests reported
above cover the later MDK refresh.
