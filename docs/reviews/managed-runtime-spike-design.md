# Managed runtime packaging proof: design review

Independent Claude Code Sonnet, `--permission-mode auto`. Read-only design review.

## Initial review

**NOT APPROVED as written.** The pins, the trust model and the overall approach hold up. The changes below are needed before implementation. I did read-only research: pinned uv source at `135a363` and the release API metadata. I downloaded no binaries and ran nothing. I made no edits, git changes or CI runs. I deleted my own scratch directory.

## Verified

- **uv pins:** all three uv archive SHA256s match the 0.11.16 release API, and the tag resolves to commit `135a363`.
- **CPython pins:** the three keys exist in the pinned `download-metadata.json`. They are PBS 20260510, `install_only_stripped`, with variant null. Their hashes match the doc.
- **uv interface:**
  - `--python-downloads-json-url` accepts a local path or `file://`.
  - `file://` archive URLs are read from disk with no HTTP client.
  - `--install-dir`, `--no-bin` and `--no-registry` exist on `uv python install`.
  - `--no-config`, `--offline`, `--no-cache` and `--managed-python` exist as global flags.
  - A hash mismatch returns `HashMismatch`, and the temp extraction is dropped.
- **Release limits:** the doc's 512MiB archive and 2MiB metadata limits match `install.sh` (`MAX_DOWNLOAD`, `MAX_MANIFEST`).
- **Overall plan:** the doc fits the approved plan and the "stop and ship the interpreter in the archive" fallback.

## Necessary changes

1. **uv does not require a hash.**
   - `sha256` is `Option` in uv's metadata, and hashing only happens `if sha256.is_some()`.
   - State that the harness enforces a non-null hash and exactly one entry in the generated JSON.
   - Add a gate that a missing hash is rejected by the harness, not by uv.
2. **Step 5 describes the wrong order.**
   - uv verifies the hash after extraction. Your step 3 already verifies the archive before uv runs, so "extraction before digest completion" is misleading.
   - Say the harness verification is primary and uv's check is a second layer.
   - Keep the archive in the 0700 directory, read-only, and re-hash it just before uv reads it.
   - `UV_PYTHON_CACHE_DIR` must stay unset. With it set, uv moves the unverified download into the cache before hashing and leaves a bad file there. `--no-cache` does not cover this path.
3. **The target name differs on macOS.**
   - The metadata key uses `darwin`. uv's own install directory and minor-link names use `macos`, and `Os::from_str` accepts both.
   - Do not derive the install directory from the key. Discover it.
   - Add a gate that the request string selects the single entry on all three targets.
4. **uv creates minor-version symlink directories.**
   - uv creates a link such as `cpython-3.13-<os>-<arch>-<libc>` in the install directory, and the venv can record it. `--no-bin` does not suppress this.
   - Record `pyvenv.cfg` home and the interpreter path. Require both to resolve inside the version slot.
   - Version slots must never be moved or renamed.
   - The unseeded venv adds only this indirection. Either justify it or have launchers use the base interpreter directly.
5. **curl cannot do what step 3 says.**
   - curl has no redirect host allowlist. Use a manual redirect loop with `-w '%{redirect_url}'`, no `-L`, a per-hop host check and a hop limit.
   - Use `--proto =https`.
   - `--max-filesize` is unreliable without a known length. Also use `ulimit -f` or a post-download size check.
   - Name the hash tool per platform: `sha256sum` on Linux, `shasum -a 256` on macOS.
6. **Private and scratch state contradicts "retain real HOME".**
   - Run the spike with scratch HOME, XDG and TMPDIR.
   - Snapshot the real `~/.cache/uv`, `~/.local/share/uv`, `~/.config/uv` and `~/.local/bin` before and after. Require no change.
7. **The path rule fails on macOS.** `/tmp` is a symlink to `/private/tmp`, and `TMPDIR` is under `/var/folders`. Use a realpath'd owned root and reject symlinks only below it.
8. **Network denial is not portable.** Enforce it on Linux with a network namespace or bwrap. macOS has no equivalent. Record it as "uv `--offline` only, not independently enforced" rather than claiming a pass.
9. **Reuse of the verifier needs a defined method.**
   - The verifier lives in an `exec python3 -I -B -` heredoc in `install.sh`. Specify a byte-exact extraction of the marked region, or a scratch `PATH` whose only `python3` is the managed interpreter.
   - Generate the scratch launcher from `marmish_release.launcher()` and assert it differs from the original only in the interpreter token.
10. **The `ast.dump` note is stale.**
    - `tools/test_marmish_release.py:44` already has `stable_ast_dump`.
    - Reword the note to "run the full `tools` test suite under 3.13.13 and record failures."
    - If tests fail, that blocks acceptance. Do not change the Python version silently.

I did not look at the Marmish release or tag data, so item 9 is untested beyond the code I read.

## Revised approval

**APPROVED** for the disposable packaging proof. All ten changes from my first review are in the revised doc, and I found no blocking correction. I read the repo code and ran nothing, with no edits, downloads or CI.

## The ten changes

| # | Change | Where | Check against the repo |
|---|---|---|---|
| 1 | The harness enforces the hash, because uv does not | Install contract step 4 and the uv-specific gate | Consistent with my read of uv's optional `sha256`. |
| 2 | Verification order, with `UV_PYTHON_CACHE_DIR` unset | Step 2 and step 5 | The harness hash is primary and uv's is a second layer. The archive is read-only and re-hashed before uv reads it. |
| 3 | `darwin` vs `macos` naming | Step 6 and the single-entry gate | The interpreter directory is discovered, not derived from the key. |
| 4 | Minor-version links | Steps 6 and 8 | Links must resolve inside the slot, slots are never moved, and launchers use the base interpreter directly. |
| 5 | curl handling | Step 3 | `--proto =https`, a manual redirect loop with a per-hop allowlist, and a size cap layered three ways. The 512MiB and 2MiB limits match `MAX_DOWNLOAD` and `MAX_MANIFEST` in `scripts/install.sh:36,39`. |
| 6 | Scratch HOME/XDG/TMPDIR and before/after snapshots | Step 2 | Covers the real uv cache, data, config and `~/.local/bin`. |
| 7 | Realpath'd temp root | Step 1 | Symlinks are rejected only below the realpath'd root. |
| 8 | Network denial | Offline-retention gate | Enforced on Linux; macOS is recorded as uv `--offline` only. |
| 9 | Byte-exact heredoc extraction | Step 7 | The markers are unique. The begin line is `scripts/install.sh:5` (`exec python3 -I -B - … <<'MARMISH_INSTALLER_PYTHON'`) and the end is line 851. `launcher()` at `tools/marmish_release.py:52` has a single `python3` token to swap. |
| 10 | `stable_ast_dump` | Existing at `tools/test_marmish_release.py:44` | The note now says to run the full suite under 3.13.13, record failures, and block on them. |

## Non-blocking notes to record in the evidence

These need no doc change before implementation.

1. **Linux still depends on system Python.**
   - `tools/marmish_installed.py:158` runs a `system_python_3_11` doctor check against `/usr/bin/python3`.
   - `tools/marmish_transport_sandbox.py:285,400,554` hard-codes `/usr/bin/python3`, including inside bwrap.
   - The spike's claim is only "no uv/download/PATH Python at launcher startup", which is accurate. Don't read a doctor pass on Linux as proof that system Python is gone.
   - Increment 1 must decide how the sandbox and doctor get their interpreter.
2. **The archive's `bin/*` launchers are manifest-hashed.**
   - They come from `launcher()` and are hash-covered by the release manifest.
   - The installer's own wrappers (`launcher_bytes`, `scripts/install.sh:470`) point at `version_root/bin/name`.
   - Replacing the archive launchers after verification is fine for a disposable proof. Increment 1 must resolve how a managed-interpreter launcher stays inside the signed or hashed set.
3. **The CI workflow needs changes.**
   - `.github/workflows/installer-validation.yml` has a 10-minute timeout.
   - Its `paths` filter does not cover the new harness or fixture files.
   - Both need adjusting once the three-target downloads and the full `tools` suite run there.
4. **The status line is stale.** Line 4 still says "implementation waits for Claude approval", so update it after this approval.

## Disposition

All ten corrections incorporated before implementation. Record remaining Linux absolute system-Python dependency explicitly; increment1 must address it. Preserve manifest-hashed archives and use installer-owned outer launchers. Existing native installer CI remains mandatory alongside packaging proof. User explicitly authorized uv implementation and installer GitHub Actions coverage.
