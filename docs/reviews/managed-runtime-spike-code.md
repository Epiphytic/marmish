# Managed runtime packaging proof: code review

Independent Claude Code Sonnet, `--permission-mode auto`.
Scope: harness, tests, frozen pins and installer workflow; no production change.

## Initial review

**Verdict: APPROVED** to commit and push the validation infrastructure so real native CI evidence can exist. There are no blocking findings. This is not phase acceptance. The phase stays open until every gate has native evidence.

## Blocking findings
None. I found no false-pass path in the offline code, no unsanitized export path in the JSON report, and no production file touched. The only tracked changes are `.beads`, the phase doc and the workflow, and the workflow diff is 37 additions with no deletions. The original `installer` job is untouched.

## Fidelity to the approved design
These match the design and I confirmed them in the code:
- **uv command:** the exact install command, `--no-config`, a fixed allowlisted environment and `UV_PYTHON_CACHE_DIR` unset (`tools/marmish_managed_runtime_spike.py:56-65,233-236`).
- **Downloads:** the manual five-hop redirect loop with a per-hop host check, no `-L`, and the size limit applied three ways (`:107-137`).
- **Extraction and metadata:** a single-member read of uv (`:165-181`) and local metadata with exactly one entry and a mandatory hash (`:184-191`).
- **Archive handling:** the archive is re-hashed and made read-only before uv runs (`:194-200`).
- **Interpreter discovery:** discovery with a link-escape check (`:220-230`), so the darwin/macos naming is handled.
- **Installer and launcher reuse:** the installer Python is extracted byte-exactly using the markers at `scripts/install.sh:6,850` (`:239-246`). Launchers are generated from `launcher()` with only the interpreter token swapped (`:304-317`).
- **Offline retention:** the proof removes uv and the downloads, then removes slot 1 and relaunches slot 2 (`:489-505`).
- **Sanitization and home snapshot:** the report holds only fixed categories, basename/line/function frames, and identifiers matched by a strict regex (`:473-480,597-604`). Raw suite output goes only to an explicit `--failure-log` opened `O_EXCL|O_NOFOLLOW` at mode 0600. The real-home snapshot reads only lstat metadata and symlink targets (`:320-339`).
- **Disclosures:** the Linux `/usr/bin/python3` dependency is recorded explicitly (`:560`). macOS is labelled `uv_offline_only_not_independently_enforced` (`:392`). `wnd` is probed with `--help`, with `native_version_flag_supported=false` (`:442,558-559`).

## Nonblocking findings, in the order I'd fix them
1. **Uncaught exception types lose the diagnosis.** `main()` catches only `Refused`, `OSError`, `ValueError`, `SubprocessError` and `TarError` (`:597-604`).
   - A `KeyError` or `TypeError` escapes. Examples are `json.loads(raw)['ok']` at `:436`, or an `identity[...]` lookup at `:212`.
   - The JSON artifact then says `ok:false, failure:null`, and the raw traceback goes to the CI log.
   - Add a final `except Exception` that records a fixed category plus the `error_frames` already built for the other branch.
2. **Several uv-negative proofs are vacuous.** `uv_negative_acceptance` (`:363-378`) only checks for a nonzero exit.
   - An unrelated uv failure would equally "prove" digest rejection.
   - The report assertion at `:534` therefore overclaims. Match a sanitized hash-mismatch signature from uv's private captured output.
3. **Several listed gates have no executable check.** The design lists them at phase doc lines 131-147.
   - Not covered: interrupted extraction; an end-to-end wrong-architecture archive (there is only a header unit test); an unexpected current directory (`cwd` is always `HOME`, `:86`); and a preexisting install beyond the slot-exists refusal.
   - Not covered: end-to-end hostile ambient `UV_/PIP_/PYTHON` variables or a user-home uv config (only a unit test and invalid TOML in the cwd and its parent, `:513-514`). The assertion text at `:540` sounds stronger than that.
   - Not covered: substituted-archive-after-rehash.
   - Record these as unmet gates, or add tests, before acceptance.
4. **The `checksum_sources` comparison is not automated or evidenced.** The fixture carries those URLs, hashes and sizes (`tests/fixtures/managed-runtime-assets.json:2-18`), but the harness only copies them into the report (`:582`). The design requires uv and CPython hashes to be compared against the checksum assets, with disagreement blocking the trial. Fetch and compare them, or record that the check was done at freeze time.
5. **CI will fail on Linux if `bwrap` is missing or restricted.** The workflow does not install or probe `bubblewrap` (`.github/workflows/installer-validation.yml:64-69`). Ubuntu 24.04 can restrict unprivileged user namespaces.
   - Both `network_prefix` and the full suite's sandbox tests depend on it.
   - A missing `/usr/bin/bwrap` raises `FileNotFoundError` from `run()` at `:399`. That surfaces as a generic `local_io_or_schema_failure` after every other stage has run (`:505`), not as the intended category.
   - Consider a setup step plus a catch that maps this to `network_namespace_unavailable`.
   - On Linux, also consider running the uv install itself under the `bwrap` prefix. Today uv's `--offline` is the only network guard for the `missing-local` "no fallback" claim.
6. **Skipped tests are not recorded.** The full suite can end `OK (skipped=N)` (`:473-474`), for example when `bwrap` or the Rust toolchain is absent. Record the skipped count and fail if it is nonzero, or list it as evidence.
7. **`test_compiler_directory` assumes a rustup shim is a symlink** (`:446-464`).
   - It only takes the `rustup which` branch when the resolved name is `rustup`. On hosts where the proxy is a hardlink, `rustc` resolves to the proxy.
   - The proxy would then run under the scratch HOME and could try to fetch a toolchain.
   - The lookup environment also omits `RUSTUP_HOME` and `CARGO_HOME`. Either could fail on a CI image that sets them, and I haven't verified the runner images.
   - Locally it is a symlink to rustup, so it works here.
8. **`run()` only kills the process group if the direct child is still alive** (`:101-103`). Orphaned descendants of a child that exited are not reaped, which is narrower than the design's wording. The 1GiB expansion cap is polled every 50ms (`:92-93`), so it is a best-effort monitor, not a hard bound. Don't describe it as enforced in the evidence.
9. **Small items.**
   - The `finally` at `:594-596` assigns the same `pending` value that is already set, so it does nothing. Remove it or make it meaningful.
   - `test_embedded_verifier_byte_exact` (`tools/test_marmish_managed_runtime_spike.py:109-113`) repeats the function's own split logic. Assert against the heredoc delimiter lines instead.
   - The workflow `paths` filter omits other `tools/*` modules that the full suite exercises (workflow lines 5-15).
   - The helper hashes are verified before `shutil.move` but not re-checked after placement (`:258`), apart from the macOS `codesign` check.

## Tests run
- The 21 focused tests passed (`python3 -B -m unittest discover -s tools -p test_marmish_managed_runtime_spike.py`). I wrote no bytecode and the git state is unchanged (10 status entries, the same as at the start).
- I ran one loopback-only `curl` probe. It showed an empty-body 302 still leaves a `body` file under curl 8.5.0, so the redirect loop's `body.stat()` and `body.unlink()` calls are safe on this version. I didn't check other versions. The probe used a throwaway temp directory, which I deleted.
- I read `install.sh`, `marmish_release.launcher` and `bootstrap_ci_python.sh` for fidelity. I checked the fixture against the design's hash tables by reading, not by running anything.

## Limits
- I did not run the harness, the full `tools` suite, the downloads, uv, `bwrap` or CI.
- I made no claims about Trial10, native macOS or aarch64 results, or the pin comparison against the upstream checksum assets.
- The macOS `darwin-aarch64-none` request selection, `otool` and `codesign` paths, the `libignition` SIGABRT and the `doctor` and `wnd --help` behavior on macOS are unexecuted here.
- The Linux `/usr/bin/python3` dependency is untouched by this proof, as the report says.
- No confinement claims follow from any of this.

## Disposition in progress

Approval permits publishing validation infrastructure, not phase acceptance.
The independently enforced offline trial exposed a namespace ancestor-ownership
issue after this review began. Its harness correction and useful findings are
being addressed and will receive another review. Required native results and
unimplemented acceptance cases remain explicit gates.

## Integrated re-review

**APPROVED** to commit and push the validation infrastructure so native CI can run. This is not phase acceptance. I found no blocking findings. I read the actual integrated files, not just the diff.

## Scope reviewed
- `tools/marmish_managed_runtime_spike.py`
- `tools/test_marmish_managed_runtime_spike.py`
- `tests/fixtures/managed-runtime-assets.json`
- `.github/workflows/installer-validation.yml`
- `docs/validation/managed-runtime-spike.md`
- `docs/validation/managed-runtime-checksums.json`
- `docs/phases/managed-runtime-spike.md`
- `docs/reviews/managed-runtime-spike-code.md`

## Tests and checks I ran (offline only)
- `python3 -B -m unittest discover -s tools -p test_marmish_managed_runtime_spike.py`: 25 tests, OK.
- The same suite with `TMPDIR` pointed at a symlinked directory, to simulate macOS: **2 failures** (finding 1).
- Fixture vs checksum evidence: the 9 artifact hashes in the fixture and the 9 in `managed-runtime-checksums.json` are the same set. The three checksum-source hashes also match. This is a one-off check; no test enforces it (finding 6).
- Production scripts and runtime are untouched. The only tracked changes are the workflow, the docs, `.beads` and the new tools, fixture and test files.

I did not run the harness, the full suite, uv, bwrap, downloads or CI. I did not read raw failure logs, home files or Trial11 output, and nothing was modified.

## Confirmed changes since the initial review
- **Offline sandbox:** `network_prefix` builds an owned tmpfs root with read-only system mounts and the scratch tree. It keeps the invoking UID/GID, asserts a different network namespace, root ownership, and a failed external connect. Probe failure fails closed: `offline_acceptance` still raises `linux_independent_offline_denial_unavailable` at the end.
- **uv negatives:** `require_uv_negative_signature` requires the hash-mismatch or missing-local signature, not just a nonzero exit.
- **Compiler discovery:** it goes through `rustup which` with only `RUSTUP_HOME` and `CARGO_HOME` passed through.
- **Skips:** the report records the skip count.
- **Final exception:** it is sanitized to a fixed category plus basename/line/function frames.
- **Cleanup:** the process group is killed even if the leader has exited, and the redundant `finally` is gone.
- **Workflow:** it installs `bubblewrap` and `binutils` on Linux, the paths filter includes `tools/**`, and the original `installer` job is unchanged.

## Remaining findings (non-blocking, in priority order)
1. **Two unit tests will fail on macOS CI, before the proof runs.**
   - `test_compiler_resolves_rustup_without_inheriting_ambient_environment` and `test_minor_directory_link_and_full_version_resolve_to_single_interpreter` compare against unresolved temp paths.
   - The code under test calls `resolve()`. On macOS `/var` is a symlink to `/private/var`, so the comparison fails.
   - My simulation with a symlinked `TMPDIR` reproduced both failures.
   - Fix by resolving `self.root` in `setUp`. Do this before the push, or the macOS job spends a CI cycle failing at its unit-test step.
2. **Report assertions overclaim what was exercised.**
   - `hostile_uv_configuration_ignored_and_environment_allowlisted` (`proof`) is appended although no hostile `UV_*`, `PIP_*` or `PYTHON*` variable is ever injected. Only an invalid `uv.toml` in the scratch HOME and root is tested.
   - `offline_retention_after_uv_download_cache_removal_and_version_deletion` is appended before the final `require` on denial availability.
   - The unmet cases are documented only in the markdown. The uploaded JSON has no `unmet_acceptance` list, so it could be misread on its own. Add a fixed list, or rename the assertions.
3. **Substitution check ordering (`install_python`).** The archive is hashed and then `chmod 0400`. The design says make it read-only and then rehash. Swapping the order is trivial. uv's own digest check backstops this, but nothing tests the substitution case.
4. **Linux uv install has no network denial.** Only `--offline` guards uv's install and the `missing-local` "no fallback" claim. The phase doc already notes this; the validation doc doesn't state it.
5. **Likely Linux CI fragility.**
   - Ubuntu 24.04 may restrict unprivileged user namespaces. `bwrap` may then fail, and the failure would surface only as `linux_independent_offline_denial_unavailable` after every other stage has run.
   - That would be a real native-evidence failure, not a harness bug. I could not verify the runner behavior offline.
6. **Fixtures are not machine-checked against each other.** Nothing tests `managed-runtime-checksums.json` against the fixture. Nothing checks `python_entry` against the pinned `metadata_source`. The design's 2 MiB metadata cap is not in `CAPS`, and the pinned metadata is 2,752,303 bytes. The harness never downloads it, so this is harmless but inconsistent.
7. **Process-group cleanup after the leader is reaped.**
   - `killpg` runs after `child.poll()` has reaped the leader, so a recycled pid could in theory be signalled.
   - A `PermissionError` there would skip `child.wait()`.
   - The window is tiny; I'd catch `OSError` and still wait.
8. **Smaller items.**
   - The 1 GiB monitor measures the whole scratch tree, not one runtime. It is polled and counts `st_size`, so it is not a hard quota.
   - The network probe treats any `OSError`, including a timeout, as denial. The namespace-ID assertion carries the real proof, so checking the errno would be cleaner.
   - The uv hash-mismatch signature (`Caused by: Hash mismatch for`) matches only synthetic text in unit tests. A real Trial11 run is needed to validate it. A mismatch would fail closed.
   - The doc's claim that "No test was skipped" for Trial 10 is not checkable from the artifacts I read.

## Limits and unmet gates
- **Documented unmet acceptance cases:** interrupted extraction, substituted-after-rehash, and complete hostile ambient/cwd combinations. They remain unmet.
- **Other limits:** the Linux `/usr/bin/python3` dependency, macOS offline-by-uv-flag only, unresolved macOS confinement, and the polling monitor all stay open. The docs state them accurately and do not claim production uv support.
- **Not evidenced:** no native macOS or aarch64 results, and no real-uv-output validation yet.
- **Out of scope:** an untracked `docs/validation/managed-runtime-linux-amd64.json` has appeared. It is not in the requested scope and I did not read it, so it needs its own sanitization check before any commit.
- **Git index:** it shows many files already staged. I didn't stage them.

## Follow-up disposition

Before publication, correct symlinked temporary-directory test expectations,
add fixed unmet-acceptance fields and narrow assertion names, rehash after
read-only publication, and machine-check checksum-evidence parity. Repeat focused
tests with a symlinked temporary root and the actual local proof, followed by
a bounded final review. Native namespaces may remain a CI platform blocker;
no host policy changes or silent gate waiver are authorized by this review.

## Final bounded review

**APPROVED** to commit and push the validation infrastructure and the sanitized Trial11 report. This is not phase acceptance or production uv support. I found no blocking findings.

**Checked in the actual files**
- **Temp-root tests:** `setUp` resolves the temporary root (`tools/test_marmish_managed_runtime_spike.py:19`). The focused suite passes with a normal `TMPDIR` and with a symlinked one, 26 tests each, which fixes the macOS failure from the last review.
- **Parity test:** it covers all 9 artifact hashes and filenames and the checksum-source records against `managed-runtime-checksums.json`, and it requires `checksum_matches` and `verified`.
- **Runtime report:**
  - The fixed `unmet_acceptance` list is seeded at the start of the report: interrupted extraction, substituted archive after rehash, and complete hostile ambient/cwd combinations.
  - The assertion is renamed to `invalid_cwd_uv_configuration_ignored_and_environment_allowlisted`.
  - The offline-retention assertion is appended only after the required Linux denial check (`:537-538`).
- **`install_python`:** `chmod 0400` now runs before the final `verify_asset` (`:200-201`).
- **Docs:** the phase doc separates the 2,752,303-byte metadata freeze research (16 MiB bound) from the 2 MiB runtime local-metadata cap. The validation doc states the `--offline`-only uv guard and that independent denial covers application startup only. It also records the quality-tool false attribution to unchanged files.
- **Trial11 JSON:** it parses and has no home paths, usernames, tokens or emails. Scratch paths are placeholders. It records `ok:true`, 435 tests with 7 skipped, `network_namespace_enforced`, `cleanup:complete` and `home_unchanged:true`.

**Findings (non-blocking)**
1. **Trial11 JSON is stale in two ways.** It has no `unmet_acceptance` field. It also still lists the old assertion name `hostile_uv_configuration_ignored_and_environment_allowlisted`, which overstates what was tested. The validation doc calls it the Trial11 report, and your note says Trial12 replaces it. Commit it only if the final replacement doesn't land first. Otherwise wait for Trial12 and commit that. Either way, state that it predates those changes.
2. **Test-gap items from the earlier reviews still stand.** The doc says the metadata cap sits outside `CAPS`, and the uv hash-mismatch signature hasn't been validated against real uv output. Trial11 passing is the only evidence on that.
3. **Linux CI fragility.** User namespaces may be restricted on Ubuntu 24.04, which surfaces as `linux_independent_offline_denial_unavailable`. That is a platform blocker, not something to waive.

**Tests run:** only the focused suite above, offline, with a throwaway temp directory that I deleted. `git status` shows the same 15 entries as at the start.

**Limits:** I did not run the harness, the full suite, uv, downloads or CI. I did not read home files or raw logs. I verified neither the Trial11 numbers against raw output nor anything about macOS, aarch64 or Trial12. The unmet gates and the unresolved macOS confinement stay open.

## Publication disposition

All three code reviews approve publishing infrastructure, not closing the phase.
Trial12 completed successfully before publication and replaces Trial11 evidence.
Root compared reports: only the narrowed assertion, the added fixed unmet-case
list and the full-suite count (436, with seven skips) changed. Artifact records,
network denial, cleanup and home-snapshot results match; no new private strings
were introduced. The final reviewer explicitly allowed that replacement.
Required native results and disclosed design cases remain outstanding.
