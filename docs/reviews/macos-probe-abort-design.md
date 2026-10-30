# macOS probe abort diagnostic design review

Reviewer: independent Claude Code (`--model sonnet`, `--permission-mode auto`).
Scope: diagnostic design and exact production profile, command builder and C probe.
No edits or live execution authorized.

## Initial review

**Verdict: CHANGES_REQUIRED**

I read the doc against `PROFILE` (`tools/marmish_transport_macos.py:286`), `_command` (509), `_preflight` (662) and `tools/native/marmish-probe.c`. I made no edits, live runs or git changes.

**Confirmed sound**
- The scope is diagnostic-only. No production profile, helper or gate changes.
- It claims no fix and keeps "no weakened fallback".
- The probe's `read` and `tcp` modes return 0 on success and 2 on socket failure. So a -6 exit is not a normal probe failure. It points to the loader, the sandbox, or `sandbox-exec`.
- Running confined `read /dev/null` before TCP separates "process can't start" from "network rule". The production `check('broker')` runs TCP first and can't tell these apart.
- The privacy handling is adequate: bounded stderr, a fixed environment, no crash-report or log scraping, no identities, Keychain or external network, and read-only workflow permissions.

**Blockers**

1. **Build isn't pinned to the production recipe.** The doc says "the native compiler" and "ad-hoc sign". Production and the spike both use `clang -Wall -Wextra -Werror -O2 -mmacosx-version-min=15.0 tools/native/marmish-probe.c -o …`, then `/usr/bin/codesign --force --sign -`, then `/usr/bin/codesign --verify --strict` (`tools/build_mdk.py:122-128`, `tools/marmish_native_spike.py:207-211`). Different flags or a different deployment target would give evidence about a different binary.
   - Change: specify those exact argv lists and reuse the same strings. Put the flags in the report as a fixed field.

2. **How `_command` is used without starting the broker is unspecified.**
   - `_command` reads `self._broker.address[1]`, `self._directory`, `self.home` and `self.socket`.
   - The doc must say the driver builds a `DarwinConfinedDaemon` and sets `_directory` and a stub `_broker` with `.address = (127.0.0.1, listener_port)`. It must not call `_prepare` or `start`, because those take the control lock, verify the manifest and start `SocksBroker`.
   - The confined TCP case must target that same port. Any other port is an expected deny, which exits 2 and isn't discriminating.
   - Mirror production topology: HOME and the exec/profile directory are separate siblings. Probe, `profile.sb`, CONFIG and ACK go in the directory outside HOME. Otherwise HOME's `subpath` grant could make the exec path pass for the wrong reason.
   - Write `profile.sb` from the imported `PROFILE` byte-for-byte, and assert equality before hashing.

3. **The evidence may not identify the cause.**
   - A SIGABRT with an empty stderr can't tell a loader or runtime denial from a signing or location problem. That would leave the user needing another round.
   - Change: add a fifth sequential case using the production `_command` and unchanged `PROFILE`, with `EXEC=/usr/bin/true`. The result narrows it down:
     - If `/usr/bin/true` also aborts, the cause is the profile or loader, independent of the probe's build, signature or location.
     - If only the probe aborts, suspect the signing, location or architecture of the probe.
   - Run all cases even if an earlier one fails. If an unconfined control fails, mark the confined results `control_invalid` rather than skipping them. The doc's "four sequentially" doesn't say whether to continue after a failure.
   - Optionally (a policy call for the user), allow one time-windowed `/usr/bin/log show` on the ephemeral runner. Filter it by the fixture's probe process name and sandbox denials, cap and sanitize the output. It is the only source of the actual denied operation. If you decline it, the doc should say a null stderr is expected to be inconclusive and name the next step.

**Required clarifications (smaller)**
- **Redaction order.** Read 4097 bytes, sanitize and replace the full buffer, then cut to 4096. Cutting first can split a path at the boundary and leak part of it. Also redact the checkout and workspace path, the resolved `/var/folders` temp path and the username, not just the owned root and runner home.
- **Environment parity.** Use `env={'PATH': '/usr/bin:/bin'}`, `close_fds=True`, `stdin=DEVNULL` and `timeout=5`, matching the `invoke` call in `_preflight` (line 689). The doc says only "fixed PATH".
- **Workflow:**
  - Trigger on `workflow_dispatch` only, never `pull_request`, on `macos-15` hosted. Don't use the self-hosted `laptop-ci` runner.
  - Use `persist-credentials: false` and `if: always()` on the upload.
  - Write the report outside the fixture tree, before cleanup.
  - State the Python version requirement and make a missing or old Python a fixed failure category.
- **Packaging.** `marmish_release.py` uses `rglob` over the built tree, not `tools/`. Confirm the new driver isn't picked up by any release or installer file list, and keep it out of `DARWIN_NATIVE`.
- **Tests.** Add assertions that the command argv equals `_command(...)` output, that the profile bytes equal `PROFILE`, and that the report is written when the child is killed by a signal or by timeout.

Apply 1–3 and the clarifications, and I'd approve. The actual hosted run remains the acceptance gate.

## Disposition

The design now fixes the exact native build recipe, construction-only production
command builder fixture, sibling HOME/control layout, fifth system executable
case, full case continuation, control validity, bounded redaction and reporting
contracts. System log scraping is deliberately excluded. Empty stderr will be
reported as inconclusive and investigated separately.

## Revised review before implementation

**APPROVED.** I found no remaining concrete blockers after checking the revised doc against the production code.

- **Recipe:** The `clang -Wall -Wextra -Werror -O2 -mmacosx-version-min=15.0 … -o`, `codesign --force --sign -`, then `/usr/bin/codesign --verify --strict` sequence matches `tools/build_mdk.py:122-128`. It also matches `marmish_native_spike.py:207-211`.
- **Construction topology:** The `DarwinConfinedDaemon.__new__` stub needs only `home`, `socket`, `_directory` and `_broker.address`, which is what `_command` (`marmish_transport_macos.py:509-516`) reads. HOME and the exec/profile directory as separate siblings, with CONFIG, ACK and ACKTMP outside HOME, mirrors `control_dir` and `_prepare`. The `BROKER=localhost:port` define and the TCP probe to `127.0.0.1` match the production `broker` preflight check. The subprocess options (DEVNULL, `env={'PATH': '/usr/bin:/bin'}`, `close_fds`, 5s timeout) match `invoke` at lines 689-693.
- **`/usr/bin/true` discriminator:** The profile allows `process-exec` and `file-read*` on the `EXEC` param. `/usr/bin/true` is therefore a valid confined target through the same builder and profile, and it separates a loader/runtime abort from a probe-specific one.
- **Probe behavior:** The `read` and `tcp` modes at `marmish-probe.c:49-106` return 0 or 2 or exit 64 on bad arguments. A -6 exit therefore can't come from the probe's own logic, which is consistent with the doc's framing.
- **Privacy and scope:** The stderr bounds and redaction, the report handling, and the exclusion from release lists and `DARWIN_NATIVE` are specified, and no production file changes.

Three minor, non-blocking notes:

1. `/usr/bin/true` has no unconfined control. Say so in the report, or add an unconfined run, so a failure there isn't over-read.
2. The "five subprocesses" count excludes the compile and codesign steps. Their failures are covered by the fixed categories.
3. An implementer should keep `PROFILE` byte equality (doc line 28) as an assertion, not just a hash record.
