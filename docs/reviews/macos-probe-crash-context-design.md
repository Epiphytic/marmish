# Owned probe crash-context design review

Independent Claude Code, Sonnet, permission mode auto; inspection only.

**Verdict: not yet APPROVED.** The design is sound on privacy, gating, and not guessing grants. Six changes are needed first, and the first two decide whether the run can find the report at all.

**Necessary changes**

1. **Also accept reports named for `sandbox-exec`.**
   - `daemon._command` runs `/usr/bin/sandbox-exec ... <probe>`. An abort before the exec, or in `sandbox-exec` itself, produces `sandbox-exec-*.ips` with `procPath=/usr/bin/sandbox-exec`.
   - The hosted evidence includes `/usr/bin/true` aborting, so a failure before the probe's own code runs is plausible.
   - Your probe-basename prefix filter would miss this and report "no report", which is a false negative.
   - Allow exactly one fallback prefix, `sandbox-exec`. Keep the exact PID match and require `procPath` to be either the owned probe or `/usr/bin/sandbox-exec`. The report should record which one matched.

2. **Constrain the unique basename.**
   - The current `marmish-probe-XXXXXXXX` is 22 characters.
   - Crash filenames use the process name, which macOS may truncate. I'm not sure of the exact limit, so cap it at 15 characters or fewer.
   - Use a fixed charset such as `[a-z0-9-]`, for example `mp-` plus 8 hex characters from the fixture's random name.
   - PID and `procPath` stay authoritative, so a prefix mismatch only costs a missed report and never a wrong match.
   - The existing `EXEC` profile parameter takes the path, so renaming is safe.

3. **Allowlist the dyld/ASI fields explicitly.**
   - For dyld and sandbox-style aborts, the reason usually sits in the report's application-specific-information section (`asi`, e.g. dyld "Library not loaded" or "missing symbol"). It is often absent from `termination` details.
   - Without it the run will likely be inconclusive.
   - Add `asi` string values, bounded and redacted like the termination text, to the extracted set.
   - Name the exact JSON keys in the design and ignore all others. Exclude `byProc`, `coalitionName`, bundle and identity fields.

4. **Specify how the report directory is handled.**
   - Open `~/Library/Logs/DiagnosticReports` once with `O_DIRECTORY|O_NOFOLLOW`.
   - Check that the current UID owns it.
   - Enumerate it in memory only and open candidates with `dir_fd`. Never emit or log non-matching names.
   - A directory that doesn't exist yet is a fixed category (`reports_dir_absent`), not an error. Do not create it.
   - Re-check after the abort, because ReportCrash may create it then.
   - Snapshot before the confined run, not before the control run.

5. **Redact first, then bound.**
   - `redact_stderr` is hardcoded to the 4096-byte `LIMIT` and its tuple return shape.
   - Don't change its behavior for the existing driver. Add a thin wrapper or a limit parameter.
   - For the 1024 and 256 character caps, redact the whole string first, capped on input at something like 64 KiB, and slice afterwards.
   - Slicing first could leave a partial home, checkout or username token.
   - Add an offline test with a token straddling the cap.

6. **Spell out the workflow.**
   - Give the new steps their own ids and run them with `if: always()`. That includes the prep step for the second private directory (`umask 077`).
   - Use a distinct artifact name.
   - Keep `if-no-files-found: error` and the existing pins, permissions and timeout.
   - The second driver must write a report even when it exits nonzero.

**Minor, no design change needed**
- Reuse the existing gates (Python 3.11 or newer, Darwin, non-root) and the fixed failure categories in the new driver.
- Add fixed outcome categories for no report, a late report, a PID/path rejection and a parse failure. Also record how many new prefix-matching files were seen, as a count only.
- Dropping the `system true` case here is fine. The abort is already shown to be generic, so one probe case gives the cleanest attribution.

Once these six are folded in, I'd approve. If the run still yields no report, that stays an honest "inconclusive", as your last paragraph already says.

## Disposition

All six requirements incorporated before implementation. Broad logs and raw
crash uploads remain excluded; only PID/path-validated synthetic child evidence
may be extracted.

## Revised approval before implementation

**APPROVED.** I read `docs/phases/macos-probe-crash-context.md` only. I made no edits and no live runs.

All six prior requirements are now in the doc:

1. **sandbox-exec fallback and exact PID/path:** The doc matches either the owned probe basename or the fixed `sandbox-exec-` prefix. It requires the payload PID to equal the owned Popen PID. It requires `procPath` to be the canonical probe path or exactly `/usr/bin/sandbox-exec`. It records the matched executable as a fixed category, not a path.
2. **11-character basename:** The name is `mp-` plus 8 hex characters, which avoids process-name truncation.
3. **Explicit ASI keys:** Six named libraries are allowed. The limits are string values or lists of strings, at most six values, and 1024 characters each.
4. **Nofollow owned directory:** The reports directory is opened with `O_DIRECTORY|O_NOFOLLOW`, UID ownership is checked, and candidates are opened relative to `dir_fd`. An absent directory is recorded as `reports_dir_absent`, rechecked after the abort, and never created.
5. **Redact before caps:** A thin wrapper redacts each full input (64 KiB cap) before the smaller character limits apply. The existing helper's behavior is unchanged.
6. **Independent always-run workflow steps:** The prep and run steps have separate ids and both use `if: always()` with `umask 077`. The upload uses a distinct artifact name and `if-no-files-found: error`. The existing permissions, timeouts and pins stay as they are.

I found no concrete blockers.
