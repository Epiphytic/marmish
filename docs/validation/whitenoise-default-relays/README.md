# White Noise defaults validation

Date: 2026-11-05. Base: ee4d109f5d3994c920c9e5886835a4facebf3443 plus
reviewed working changes. Local Linux AMD64; this is source qualification, not
a new binary release. Published installer pins remain v0.2.3.

## Actual client/server execution

Both disposable profiles were created with `--tor off setup --no-grants`, with
no relay flag. Each config stored exactly EU then US White Noise URLs. Updated
server grant and client trust independently, then started two foreground native
daemons. Ran `marmish SERVER_NPUB -- printf ...` twice, using no hostname, no
relay option and no alias. [Finite results](live.json) record exact byte checks
for quotes, backslashes, literal shell-like input, Unicode and embedded newlines,
then a second command reusing the default room. Remote execution returned 0 for
both. `commands`, `wait --latest` and `output --latest` also accepted the bare
npub. Owned daemons joined on SIGINT and private test state was removed.

Observed elapsed command times: 4.965 s for 64 bytes and 4.559 s for 19 bytes.
These two samples include command transport and execution; they are correctness
checks, not representative throughput benchmarks or Tor measurements.

## Actual installer quickstart

Built all native binaries locally, copied and stripped debug symbols, packaged
with the native release tool, and substituted the candidate AMD64 manifest hash
into a private copy of the bootstrap. These are local fixtures, never published
assets or replacements for the immutable v0.2.3 release. A utility-only PATH
excluded Python/uv/Cargo/rustc from installer/runtime children; Python supervised
the PTY only. Both [explicit relay](quickstart-explicit.json) and
[blank/default relay](quickstart-default.json) cases completed all five prompts,
created an offline file-backed identity, stored the expected relay set, repeated
setup with the same identity and unchanged configuration, stopped the owner and
removed private state. The default case saved both canonical White Noise URLs.
No transport command was invoked by these fixtures. `network_activation=false`
describes the expected offline setup path and owner-stop evidence, not packet
capture or an enforced network firewall. Actual networking was tested above.

The release candidate workflow now runs both PTY cases on Linux AMD64, Linux
ARM64 and macOS ARM64 when next qualifying a release. Those new remote jobs
have not been run as part of this source-only increment.

## Checks and independent review

- Python quickstart supervisor tests: 18 passed.
- Python installer tests: 34 passed.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked -p marmish-native --all-targets -- -D warnings`: passed.
- `cargo test --locked -p marmish-native --lib`: **395 passed, 0 failed**,
  327.49 seconds, final acceptance run.
- Actual Claude Code design and integrated code reviews approved; see
  [design](../../reviews/whitenoise-default-design.md) and
  [code](../../reviews/whitenoise-default-code.md).
- Ripwire quality delta and change-safety inspection run. Intentional selector
  branching, added test length and edit churn remain; test functions were
  falsely labeled dead code and an imported Python symbol attributed to an
  unchanged file. Claude reviewed these dispositions; no blocking finding.

An initial full-suite run overlapped rebuilding its test executable after the
last CLI regression test was added. It is not qualification evidence; self-spawn
installation tests in that run failed. The final binary's full suite is the
acceptance run. No product change was made to disguise those initial failures.
