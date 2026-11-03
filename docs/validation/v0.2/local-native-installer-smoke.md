# Local native installer smoke

On 2026-11-03, Linux AMD64 ran the real native shell installer against a
locally packaged diagnostic bundle. Its three executables were Rust debug
builds with debug information stripped from private copies. The build reported
baseline commit `e8d630eb0eb9db15a4a726ec72d661736cac9103`, but included dirty
acceptance tooling. This is diagnostic evidence, **not release qualification**.

The bundle used the verified 643-package license inventory. The generated
bootstrap pinned the Linux manifest; the other two platform pins were zero
and deliberately unusable. The bootstrap was not published.

Observed results:

- The actual shell installer verified the manifest and assets, probed the
  native runtime and completed installation, reporting version `0.2.0`, target
  `x86_64-unknown-linux-gnu`, active generation 1.
- With `PATH=/nonexistent`, the installed launcher reported
  `marmish-native 0.2.0`; `release-info` reported native version, target, source
  and format versions successfully.
- With the same environment, installed `setup --role both --no-grants` created
  a disposable named identity using explicit file storage. It reported
  `inbound_enabled=false`, `identity_created=true`, `owner_opened=true`, and
  successful engine, MDK and runtime shutdown.
- Repeating the identical setup retained the same public identity, changed no
  configuration and opened no owner. The no-grant profile had no `commands`
  entry after shutdown. Native `install-status` independently reported version
  `0.2.0`, active generation 1, under the same Python-free environment.

This smoke does not test HTTPS fetching, default-auto native keychains,
interactive terminal selection, macOS, Linux ARM64 or remote execution. Those
remain separate installed acceptance gates. No private keys or profile state
are included in this evidence.
