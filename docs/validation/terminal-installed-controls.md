# Installed Terminal CLI control validation

The ignored `terminal_installed_runtime_native_controls` test on Linux AMD64
ran the actual CLI twice, with separate controlling PTYs, the real Terminal
worker, real tmux and the authenticated loopback WSS fixture. The first CLI
propagated a local window size of 31 by 91 to the remote PTY. A literal Ctrl-C
interrupted a remote shell process whose SIGINT trap printed a distinct
marker. A subsequent command confirmed the interactive shell survived.

The line-start tilde-dot caused a clean local detach. The second CLI
reattached as a default writer and checked the same tmux socket path and
inode. A newly synthesized marker recovered a shell variable set before
detach. Both CLIs exited successfully with empty stderr and restored the
four original termios flag words, control bytes, speeds and Linux line
discipline. The first PTY preserved its resized geometry with unchanged
pixel fields. The second preserved its original geometry. Ordinary Command
passed before and after this sequence.

| Linux AMD64 payload | Total | Raw attach | Key echo | Raw resume |
| --- | ---: | ---: | ---: | ---: |
| Cargo debug, exact test source `014d754` | 29.46 s | 6.871 s | 149.113 ms | 7.700 s |
| Shell-installed optimized package, source `d091cc8` | 28.76 s | 6.858 s | 137.629 ms | 6.958 s |

The installed run used the [exact optimized build](mdk-master-3548.md#optimized-linux-installer-and-private-terminal).
Installation took 0.888 s and all three installed executable payloads
matched their package inputs. The harness used the package build stamp,
the same MDK lock and the exact reviewed test files. Timings are
single-run wall samples on loopback. The longer
[backend benchmark](terminal-installed-runtime.md#benchmark-with-the-refreshed-upstream-mdk-lock)
reports distributions.

The separate positive foreground regression passed in 20.32 s and the
quiet AUTH refusal passed in 16.98 s. Both reused the normal debug
binaries. The normal build took 20.25 s and strict all-target Clippy
with `installed-checks` passed in 35.28 s. The core CI selection is
unchanged. To reproduce with matching binaries already built, run:

```sh
cargo test --offline --locked -p marmish-native --lib \
  terminal_installed_runtime_native_controls -- --ignored --nocapture
```

The relay uses test-only TLS trust and prepared local owners. This run
covers neither a standalone daemon on an external relay nor activation
on macOS or Linux ARM64. Bracketed paste and takeover are covered
separately by the local frontend and backend fixtures. The cooked Busy
response is a distinct failure checkpoint for default resume in this
test. It did not occur in either run.
