# Credential timeout fixture amendment review

Actual Claude Code, `--permission-mode auto`, model `claude-sonnet-5-5`.
Session: `355bacca-ceb3-46a7-a10e-69dee036778a`.

**APPROVED** as a bounded, fixture-only design, with the conditions below. I read `identity.rs` (`helper_exchange`, `controlled_helper_exchange`, the test) and the ARM log. I didn't run Cargo or any live process.

**What the code says about the failure**
- The panic is at `identity.rs:1291`, the first assertion. That is the 60ms `Timeout` check, so the oversize half never ran.
- The helper loop checks the deadline at the top of every iteration, and `Instant::now() >= deadline` returns `Timeout`. A healthy `exec sleep 10` therefore can't produce anything but `Timeout`, however slow the runner is.
- So the actual `Result` must have been an `Err` returned before the deadline fired, or an `Ok`. An `Ok` is essentially impossible here, so the candidates are:
  - A `spawn()?` error. The most likely is `ETXTBSY` ("Text file busy"). The test does `write_new` then spawns at once, and another parallel test thread can fork while this thread holds the write fd. That is a classic Linux flake, and ARM CI ran 470 tests in about 614s, so there was plenty of concurrency.
  - A `keychain_failed` result. `env_clear()` removes `PATH`, so `sh` relies on its default PATH to find a bare `sleep`. If the lookup fails, the helper exits non-zero with empty output, which `Ordinary` kind reports as `keychain_failed`.
  - A write or read I/O error (`EPIPE` and similar). This is less likely, because `exec sleep` keeps stdin open and a 2-byte write fits in the pipe buffer.
- stderr is `Stdio::null()`, so nothing else would have explained it. That is why capturing the `Result` Debug is the most valuable part of the amendment.

**Why the amendment is sound**
- Production code is untouched, and the test's contract stays the same: 60ms `Timeout`, elapsed under 2s, and `helper_oversize` within the 2s budget.
- Absolute `/bin/sleep` and `/usr/bin/head` remove the PATH dependency, which makes `keychain_failed` less likely.
- Separate helper files for timeout and oversize remove the rewrite of an executable that was just run. That rewrite is a second `ETXTBSY` risk, because `fs::write` opens a possibly-busy file for writing.
- Consuming the request first is harmless, but it is hardening rather than a diagnosed cause.

**Conditions**
1. **Don't claim a root cause.** The change may be diagnostic as much as a fix. If the cause is `ETXTBSY` on the initial write, separate files won't cure it. State that in the review record.
2. **Put the `Result` Debug and `start.elapsed()` in the assertion messages.** The test should not panic with only `matches!` and no value. Do this for both phases. Bind the result first (`let r = helper_exchange(..); assert!(matches!(r, Err(Error::Timeout)), "{r:?} after {:?}", start.elapsed())`). `Error` must be `Debug` for this. Zeroizing bytes in an `Ok` would print, but the content is a fixture and not secret.
3. **Use a request-consuming prefix that can't hang.** `cat >/dev/null` works, because the helper closes stdin once the 2-byte request is written, so `cat` sees EOF. Avoid `read` (the request has no newline) and avoid anything that would wait past EOF. The oversize helper's `head -c 9000` must start only after that consume step.
4. **Write each helper with an explicit close before spawning, and give each its own path.** Use a scope or `drop` on the write handle. If the log shows `ETXTBSY`, the follow-up should be a short bounded retry on spawn in the test only (not production), or `sync_all` plus a unique filename.
5. **Treat a rerun as evidence, not proof.** A pass doesn't prove the flake is gone, so run the test repeatedly under parallel load (for example `--test-threads` at the CI value, in a loop) after the root suite finishes.

Disposition: implement fixture-only request consumption, absolute utility paths, separate closed executable files and result/elapsed assertions. The reviewer lists possible causes, including ETXTBSY; none is established by the original log. Repeated passes are evidence only. Production credential handling and time budgets remain unchanged.
