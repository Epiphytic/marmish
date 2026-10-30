# Managed bootstrap and setup: design review

Independent Claude Code Sonnet, `--permission-mode auto`, read-only review.

## Initial review

**Verdict: not yet approved.** Items 1–5 below need precise corrections before phase 1 implementation. With those made, I would approve conditional on the phase 0 gates at the end. The scope is sound: bootstrap ships without the unified daemon or Both mode, and the legacy format-1 and old-archive paths are handled. I changed nothing and ran nothing.

## Corrections required

1. **The final-path runtime slot is created before any validation.**
   - **Contradiction:** The doc says the shell does only routing, download, hash and uv. But the slot lives at `prefix/runtimes/…` and must never be moved after install.
   - **What Python does today:** Root refusal, `safe_path`, `owned_directory(private)` and the flock are all in the Python body (`install.sh:797-813`, `:440-468`). With the design as written, uv could write into an unvalidated or symlinked prefix, possibly as root.
   - **Uninstall:** `--uninstall` "uses the same bootstrap path", so it would create a new ~100 MB slot inside the prefix being removed.
   - **Fix:**
     - The shell checks non-root only.
     - The installer runs under a scratch-hosted interpreter, installed with the same pinned procedure and discarded afterwards. `--uninstall` uses it too.
     - The Python body validates the prefix, takes the flock, then runs the pinned uv to create the final slot under the lock.
   - **Simplification:** This removes the "concurrent bootstraps prepare separate slots / clean own incomplete slot" machinery. A receipt-less slot found under the lock is provably dead and can be removed.

2. **The environment split is unspecified.** uv gets a scratch HOME, but the installer body derives the default prefix, launcher directory and quickstart identity from `Path.home()` (`install.sh:730`, `:803-804`). The doc restores the real HOME only for "application/setup execution", so the installer would resolve everything under scratch. State that the installer body runs with a filtered environment and the real HOME, and that only uv gets scratch directories. The shell must never compute the default prefix.

3. **Two capabilities are conflated: setup API v1 and a managed helper runtime.**
   - **Old archives:** `.1` and `.2` still hardcode `/usr/bin/python3` in `_doctor` (`marmish_installed.py:158`) and in the sandbox (`marmish_transport_sandbox.py:285`, `:400`, `:554`). On a Linux host without it, doctor reports `system_python_3_11: false`. The legacy quickstart's first step is `--doctor`, so it fails.
   - **Needed:** A separate readable constant for the managed helper runtime. The installer's closing message and the quickstart preflight must disclose or fail early for legacy archive plus Linux plus no system Python.
   - **New-archive behaviour:** A new archive launched under a non-managed interpreter, such as its own `bin/marmish` which still uses `python3`, must refuse. It must not fall back to a system interpreter.
   - **CI evidence:** Split it. The published `.2` archive under a masked `python3` should expect exactly that one doctor check to fail. Only a newly built archive can prove independence.
   - **Phase 0 harness:** It requires `doctor ok` with the system Python present, and records `system_python_dependency`. It cannot serve as independence proof.

4. **Acceptance for a new archive through the real shell entrypoint is not possible before publication.** Production origin and hash overrides are forbidden. Existing tests inject a local archive by text-replacing `install_release` in a copy of the script (`test_marmish_quickstart.py` ~l.120). The doc should say:
   - Real-shell evidence uses the published `.2` archive.
   - New-archive evidence uses the test-copy wrapper with a local archive.
   - A real native build in PR CI is either limited to the release-build job or explicitly out of scope.
   - It should also name how `/usr/bin/python3` is masked on hosted runners. `ubuntu-24.04` restricts unprivileged user namespaces, which may block the bwrap approach. If masking can't be done, mark the target blocked.

5. **Argument parity is underspecified.**
   - The existing argparse parser accepts abbreviations, `--opt=value` forms and `-h`.
   - The shell cannot call Python for help before bootstrap, so it needs its own help text.
   - Decide explicitly: set `allow_abbrev=False` in the body, and have the shell accept exact `--opt value` and `--opt=value`. Document the abbreviation break.
   - Test both parsers against one table, including the mutual exclusions, `--version` with `--uninstall`, and a non-absolute prefix.
   - Root refusal must also happen in the shell before any network access.

## Non-blocking corrections

6. **Receipt binding is inconsistent.**
   - The receipt fields omit app version and target, yet the sandbox section says the runtime is "bound to the verified application version/target". Only the marker holds those, and the app doesn't parse the marker.
   - Either drop the version binding and bind on `realpath(sys.executable)` plus architecture, or define an app-side marker reader.
   - State that the receipt is tamper-detection only. Receipt, marker digest and runtime are all user-writable.
   - Scope the tree digest to the PBS distribution directory. uv leaves `.lock`, `.cache` and minor-link entries beside it.
   - Define per-confined-start verification cost: full rehash at install, bounded check at start, measured in phase 0.

7. **Slot identity is undefined, so "exclusive create" and "reuse" contradict each other.** Name the slot deterministically from the target, CPython, PBS and uv digests and the receipt format. Otherwise reuse never fires and every install leaks about 100 MB.

8. **The overall plan is internally inconsistent at lines 238–244.** It says increment 1 "retains existing frontend behavior" and also "replaces foreground-only authorization with durable managed configuration". It also says "version-owned environment" where phase 1 uses install-owned slots. Phase 1 correctly keeps invocation-only authorization. Record that it supersedes those sentences, so reviewers don't demand durable-config migration tests in increment 1.

9. **Drift and seam notes.**
   - Pins must be literals in `install.sh`, since `curl | sh` can't read `tests/fixtures/managed-runtime-assets.json`. Add a fixture-equals-script test.
   - The body can't import `marmish_release`, because it isn't in the archive. "Shared generator logic" has to mean duplicated bytes with a parity test. The spike's single-token replacement assertion is the template.
   - Keep `inflate`, `extract_ustar`, `validate_manifest` and the begin/end markers stable. The spike drives them.
   - macOS Python runs only host-side. The sandbox profile grants reads for `EXEC`, `HOME`, `/System` and `/usr/lib`. Replace "verify without extending read rules" with an assertion that no profile change is needed.

10. **Optional simplifications.**
    - The transaction record can store `(format, version, target, slot)` triples. Launchers, marker and selector are deterministic from them, so byte-level "descriptions" are unnecessary.
    - Read the setup API constant with `ast` rather than importing the app module, so no archive code runs before setup.

## Phase 0 gates this approval depends on

The conditional approval holds only if these are met. Nothing here has passed yet.

- **Hash and uv gates:** The uv and CPython digest and mandatory-hash gates pass on all three native targets. The negative uv cases must be rejected, and the offline retention runs must pass.
- **Test suite:** The full `tools` suite passes under 3.13.13.
- **Missing evidence to add to the phase 0 report:** `tools/marmish_managed_runtime_spike.py` has no `readelf` or `otool` output. Add the following:
  - NEEDED libraries for the PBS `python`, `libpython` and every extension module
  - tree file count, bytes and symlinks
  - the resulting directory layout

  Phase 1's `/marmish-python` mount and receipt cost depend on this.
- **Confinement:** Phase 0 proves packaging only. Confinement stays a separate phase 1 gate.

## Revised review

**Verdict: APPROVED, conditional on the Phase 0 native gates.** I found no concrete blockers. I only read the docs, the initial review, and the relevant code. I made no edits and ran no tests, CI or downloads.

## The 5 blockers

| # | Blocker | Status | Evidence |
|---|---|---|---|
| 1 | Final slot created before prefix validation, and uninstall creating a slot | **Fixed** | `managed-bootstrap.md:49-54` limits the shell to a root refusal and scratch-hosted uv/CPython. It never computes the default prefix. `:129-139` has the scratch-interpreter body do root refusal, `safe_path`, `owned_directory(private)` and the flock first. Under that lock it recovers, reuses or exclusively creates the final slot. A receipt-less slot is removed only under the lock, and never by elapsed time. Uninstall uses scratch Python and creates no slot. |
| 2 | Environment split | **Fixed** | `:73-80`: only uv gets scratch HOME/XDG/TMPDIR. The installer body and the app get real HOME with a filtered environment, so `Path.home()` at `install.sh:730,803-804` still resolves existing defaults. |
| 3 | Setup API conflated with the helper runtime | **Fixed** | `:169-174` adds a separate `MANAGED_HELPER_RUNTIME_API = 1`. A new archive run under a non-managed Python refuses rather than falling back. `:236-249` reads both constants with `ast`, treats absent as legacy, and fails early for a legacy archive on Linux with no system Python. The hardcoded paths it cites are real: `marmish_installed.py:158` and `marmish_transport_sandbox.py:285,400,554`. |
| 4 | New-archive acceptance and `/usr/bin/python3` masking | **Fixed** | `:286-309` splits the evidence three ways. The real shell uses the published `.2` archive and expects only the `system_python_3_11` doctor check to fail. A test-copy wrapper with a local archive is labelled as wrapper evidence. Native builds run in the release jobs, and no fake helper counts as independence proof. |
| 5 | Argument parity | **Fixed** | `:56-67` has one acceptance table for both parsers, sets `allow_abbrev=False` and documents the break. Both parsers reject bad input before any network access, and the shell refuses root before the network. `install.sh:781-786` shows the current argparse flags the table covers. |

The non-blocking items from the initial review (6-10) are also resolved:
- **Receipt binding (6, 7):** `:98-110` defines the slot identity H as a full digest. The receipt is scoped to the PBS tree and described as tamper-detection only.
- **Verification cost:** `:177-185` bounds start-time verification at install/reuse versus per start, with a measured limit.
- **Increment 1 vs 3 (8):** `native-setup-unified-daemon.md:238-243` and the row 1 table text now say increment 1 keeps invocation-only authorization and uses install-owned slots. Only increment 3 moves to durable configuration. I found no remaining "version-owned" or "retains existing" contradiction.
- **Literal pins (9):** pinned values must be literals in `install.sh`, with a fixture-equality test.
- **Duplicated launcher (9):** the launcher generator is duplicated with a parity test, because the embedded installer cannot import `marmish_release`.
- **Stable names (9):** `inflate`, `extract_ustar`, `validate_manifest` and the markers stay stable.
- **macOS (9):** the design asserts that no sandbox profile change is needed.

## CI-only privileged mount-namespace fixture

The design explicitly covers each point (`:301-309`):
- It uses `sudo unshare --mount --propagation private`.
- The sentinel is bind-mounted over `/usr/bin/python3` inside that namespace only.
- It drops to the original runner UID before the installer or transport runs.
- It says "Never modify the host mount namespace or run installer as root".
- It says that if Ubuntu policy denies the namespace or nested confinement, the gate is marked blocked and host security is not weakened.
- It says this is not Marmish root execution, and not authorization for privileged Unix-user mapping.

One clarification worth adding when the implementation is reviewed. It is not a blocker, because the stated constraints already forbid the unsafe outcomes:
- **Drop mechanism:** the mechanism is unspecified. The fixture should assert euid/egid ≠ 0, cleared capabilities and supplementary groups, and scrubbed `SUDO_*` variables before the installer starts.
- **Host policy:** it should state that no sysctl or AppArmor change is made, and that the sentinel lives in runner temp.

## Conditions that still hold

None of these has passed, and none can be waived:
- **Hash gates:** the uv and CPython digest and mandatory-hash gates, plus the negative uv tests, must pass on Linux x86_64, Linux aarch64 and macOS aarch64.
- **Test suite and retention:** the full `tools` suite must pass under 3.13.13, with offline retention on all three targets.
- **Dependency evidence:** the harness already runs `readelf` for NEEDED libraries and checks file count. The report must still include `libpython` and every extension's NEEDED libraries, plus file bytes, symlinks and layout. I did not see a macOS `otool` path in the harness, so confirm macOS gets equivalent evidence. The Phase 1 `/marmish-python` mount and the receipt cost depend on this.
- **Confinement:** Phase 0 proves packaging only. Confinement stays a separate Phase 1 per-target gate, and a platform that can't run it stays blocked.
- **Scope of the Phase 0 doctor check:** the Phase 0 published-archive doctor records `system_python_dependency`, so it can't satisfy the new-archive independence gates.

## Disposition

All five blockers and useful clarifications incorporated. Approval is conditional
on phase0 native acceptance; production implementation has not started.
CI privilege is limited to private test namespaces, with nonroot execution and
cleared capabilities/groups/environment before installer or transport starts.
No sysctl/AppArmor changes or host mount changes are permitted.
