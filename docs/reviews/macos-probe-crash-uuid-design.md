# Crash attribution diagnostic extension design review

Independent Claude Code Sonnet, auto mode, read-only.

## Initial review

**Verdict: CHANGES NEEDED.** The design is sound but has one blocking correctness problem and a few security tightenings.

**Blocking**

1. **The redacted-path pattern probably won't match.** The real probe path is `<root>/control/mp-xxxxxxxx`, with `root=/private/tmp/marmish-crash-XXXX` (`marmish_macos_probe_diagnostic.py:105`). That puts two components between `/private/tmp` and the basename. A pattern of exactly `/private/tmp/*/BASENAME` fits only if the OS collapses the whole directory into one `*`. The hosted JSON dropped the path, so this is unverified.
   - Either specify the pattern from the known structure (`/private/tmp/*/control/BASENAME`, or `*/*/BASENAME`), or state that the OS collapses the directory and that the next run will confirm it.
   - Don't accept a pattern you haven't observed. Add the diagnostics in item 2 so one more run settles it.

2. **Rejected-path diagnostics are too thin.** Booleans alone risk a third round with no new information. For a rejected candidate, also record fixed enums and counts, never raw text:
   - the prefix class: `/private/tmp`, `/tmp`, `/Users`, or other;
   - the component count, capped at 8;
   - the number of `*` components;
   - whether the last component equals the basename;
   - whether `procName` equals the basename.

**Security and correctness tightenings**

3. **Redaction match.**
   - Split on `/`, then require exactly one component equal to `*`, no empty, `.` or `..` components, no NUL, and a length bound (for example 4096) before any matching.
   - Do the alias check first, and require an absolute path. `Path.resolve(strict=True)` must equal `probe.resolve()`.

4. **UUID image match.**
   - Match on the image's `name` field. If `path` is present, it must end in `/BASENAME`.
   - Require exactly one `usedImages` entry with that name; if there are zero or several, reject.
   - Reject the nil UUID, any non-string `uuid`, and any architecture other than the one dwarfdump reported.
   - Compare as `uuid.UUID` objects, because dwarfdump prints uppercase and `.ips` files use lowercase.

5. **The `dwarfdump` call.**
   - Use the absolute path `/usr/bin/dwarfdump`. It is an xcode-select shim, so a missing or failing shim should give a fixed category.
   - Require the printed path to equal `str(probe)` and the architecture to equal `platform.machine()`.
   - Simpler alternative, which I'd prefer: parse `LC_UUID` directly from the thin Mach-O in Python, bounded to a 4 KiB header read with `O_NOFOLLOW`. That avoids a subprocess and the shim.

6. **UUID binds the build, not the run.** Identical sources and compilers give the same `LC_UUID` across runs. The real binding is the PID match, the report being new against the `before` snapshot, and the 20 s window. Say so in the doc rather than calling UUID strong attribution. Optionally also require the report's `captureTime` or `procLaunch` to fall after the spawn.

7. **Ordering.** Obtain the UUID after `build_probe` succeeds, before the control run, and fail with a fixed category if it doesn't parse. A UUID failure must not downgrade to the exact-path-only fallback, or relax the existing `PID + exact path` rule.

8. **Tests.** Add cases for multiple `*` components, `*` embedded in a component, a `..` component, an oversized path, and duplicate `usedImages` names. Also assert that the report contains no UUID or path substring.

I read only the design doc, the driver (`tools/marmish_macos_crash_diagnostic.py`) and the fixture helper in `tools/marmish_macos_probe_diagnostic.py`. I made no edits and ran no tests.

## Revised approval

**APPROVED**, with no security or correctness blockers.

The design holds up for these reasons:
- **Attribution:** extraction requires the exact new PID, a pre-spawn new-file snapshot, a unique random basename, and a matching procName. The redacted-path case also requires name, arch and UUID to match the usedImages entry. The doc says UUID binds the build and not the run, which is the correct framing.
- **Wildcard handling:** only four literal shapes are candidates, with exactly one `*` component and no dot components. Anything else is rejected.
- **Scope:** extraction is diagnostic-only, with no raw path or UUID in reports and no product permission changes.

Non-blocking clarifications to fix before implementation:
1. **Mach-O failure behavior:** "UUID failure never downgrades attribution" is ambiguous. State two things explicitly:
   - Does a Mach-O or UUID parse failure abort the run before either child, or only disable the redacted-path branch?
   - Do exact and alias paths still work without a UUID?

   I'd suggest the parse is fatal and happens before spawn, as the build-then-read ordering implies.
2. **UUID comparison:** say both sides are parsed into canonical UUID values and compared case-insensitively. The `.ips` UUID is a lowercase hyphenated string, and the Mach-O value is 16 raw bytes.
3. **Mach-O header fields:** reject fat/universal magic explicitly. Check `cputype` only, so an arm64e subtype doesn't cause a false rejection. Treat a 4096-byte read that doesn't cover the load commands as a fixed-category failure. The doc already implies this.
4. **Alias check:** require the strict resolution to be a read-only `realpath`/`samefile` comparison. Also require that no resolution failure text reaches the report.
5. **Typos:** "at most4096", "capped8" (twice), and "collapsedmiddle" in your message, if it also appears in the doc.

## Disposition

All blocking changes incorporated before implementation; nonblocking UUID parse
fatality, CPU/header bounds, canonical UUID and read-only alias behavior clarified.
