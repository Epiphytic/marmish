# Darwin owned-child correction: local evidence

Task: `marmish-0ud`. The motivating hosted candidate was [41620775751](https://github.com/Epiphytic/marmish/actions/runs/41620775751), source `01e40073c84d9c8fea034fac3bb29761da7c14c6`. Its macOS engine suite passed 100 and failed 5; this document does **not** claim those failures have passed after the correction.

The [approved design](../../phases/v0.2-darwin-owned-child.md) and [actual completed-code review](../../reviews/v0.2-darwin-owned-code-r1.md) cover exact retained-child identity lookup and a bounded terminal-group EPERM proof. Public Darwin process identity and production descriptor ceilings/CLOEXEC checks are unchanged. The isolated descriptor regression now derives its ceiling from the actual inherited descriptor snapshot.

On the local Linux AMD64 host, the exact reviewed five-file source passed:

- `cargo test --offline --locked -p marmish-engine --lib worker::`: **10 passed, 0 failed**, including separate byte streams/exit status, held descendant cleanup, forged identity refusal and queued-EOF expiry.
- `cargo clippy --offline --locked -p marmish-engine --all-targets -- -D warnings`: **passed**.
- Edition 2021 rustfmt check on the five owned Rust files and `git diff --check`: **passed**.

The scoped ripwire quality check reported increased length in the explicit descriptor regression (59 to 81 lines), and cfg-boundary dead-code reports for platform helpers that have actual Darwin callers. No metric suppression or baseline change was applied. Explicit native regression steps were retained rather than concealed behind a generic test framework.

No Darwin source was compiled or executed locally. The required native gate is the complete macOS engine suite plus strict `-p marmish-engine --all-targets` Clippy. It must execute the positive zombie-only proof under a non-root runner. The new fast-exit test verifies captured identity/group and ordinary reap but does not force a particular scheduler race. EINTR injection is not covered; waitid errors fail closed. No acceptance or release qualification is inferred from the local checks.
