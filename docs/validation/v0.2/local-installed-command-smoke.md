# Installed native command diagnostic

On 2026-11-03, the [local diagnostic bundle](local-native-installer-smoke.md)
ran installed `marmish command-check` through the explicit
`wss://relay.us.whitenoise.chat` relay, Tor off, disposable file-backed identities
and `PATH=/nonexistent`. The process exited 0 after 20,759 ms. Its report marked
engine quiescence, engine/MDK/runtime shutdown and private-state cleanup complete.

| Internal check stage | Verified stdout | Exit | Stage time |
| --- | ---: | ---: | ---: |
| Running command recovery | 27 bytes | 7 | 8,025 ms |
| Literal arguments | 53 bytes | 0 | 1,593 ms |
| Binary stdin | 16,384 bytes | 0 | 2,177 ms |
| Stderr and exit | 0 bytes, 12 stderr bytes | 7 | 1,566 ms |

Recovery observed execution once, verified recovered output and accepted an
identical Submit while refusing local and remote conflicting retries. The
binary stdin stage reported integer output goodput of 7,524 bytes/s. These are
existing internal-check stage scopes, not network RTT, detached public CLI
latency or percentile estimates. The total includes additional setup and cleanup.

[Normalized observations](local-installed-command-smoke.json) retain the measured
values and limitations. This is an internal command check through actual
installed Rust products built from a dirty diagnostic source. It does not qualify
the final immutable release, public CLI matrix, default-auto keychains, HTTPS,
macOS, Linux ARM64 or Tor. The shell installation and product execution required
no Python runtime. No private state is published.
