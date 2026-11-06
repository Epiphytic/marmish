# Streaming primitives: integrated code review

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Scope: phase 1 crate, workspace/lock registration, native CI test list and
packaging impact, against the approved phase 1 design. The reviewer could run
bounded local tests and scratch property checks; no live transports or edits.

## Round 1

**Verdict: blocking changes needed. I can't APPROVE yet.** The crate itself is sound. One of the plumbing edits breaks existing tests.

**Blocking**

1. `tools/marmish_release_licenses.py:34`: adding `'marmish-stream'` to `ROOTS['native']` breaks two existing native-license tests.
   - `validate_graph` at line 398 now requires every root to appear in the manifest's package list.
   - The `test_marmish_native_licenses.py` fixture (lines 20-25) lists only `marmish-native`, so the check fails.
   - Failing tests: `test_assembles_validated_source_inventory_and_original_texts` (`missing_production_license_root`) and `test_refuses_tampered_notice_before_creating_asset` (wrong error, same cause).
   - I ran it with `python3 -m unittest discover -s tools -p test_marmish_native_licenses.py`. The result was 1 failure and 1 error. `test_marmish_release_licenses.py` passes, 10 of 10.
   - `marmish-native` does not depend on `marmish-stream`, so the root adds nothing to the shipped binary's bundle.
   - Fix one of two ways:
     - Update the fixture and tests to include a `marmish-stream` package, if the native bundle should list it.
     - Revert the `ROOTS` edit until phase 2 wires the crate into native.

**Non-blocking, worth fixing before phase 2a**

2. `crates/marmish-stream/src/wire.rs:261-285`: `validate_response` ignores `cancel_requested` because of the `..` at line 269. A response with `cancel_requested: false` and `terminal: Cancelled` encodes and decodes fine. The design says cancellation responses distinguish an accepted request from a confirmed terminal cancellation. Reject `Terminal::Cancelled` when `cancel_requested` is false, and add a test.
3. `crates/marmish-stream/src/wire.rs:319-331`: `decode_request` returns an Open without the age or skew check. Only `validate_at` enforces it, so a caller can skip it. This matches the doc, but consider a type or doc guard.
4. `crates/marmish-stream/src/codec.rs:36-49`: `validate` accepts a `Zlib` block whose data is at least as long as `raw_len`. The doc says the encoder uses zlib only when it is smaller. This is not a safety issue, since decode is bounded and hash-checked. It is a small canonical-form gap that you could reject with `data.len() < raw_len` for zlib.

**What I verified**
- **Tests and lint:** the 14 crate tests pass. Fmt and `clippy --all-targets -D warnings` are clean (offline, `--locked`).
- **Randomized check:** I ran a scratch crate in `/tmp`, with no repo edits.
  - 300k mutated requests: no panics, and every accepted one re-encoded byte-identically.
  - 200k random zlib frames through `decode`: no panics.
  - A model check of `ReceiveWindow` over 2000 rounds. Delivered bytes always matched the model, buffered bytes stayed within `WINDOW_BYTES`, and retries returned only `Ok(false)`, `OldRetry` or `Conflict`.
- **Raw and encoded bounds:** `raw_len` and the encoded data are both capped at 8 KiB. Base64 length is checked before the decode allocation.
- **Decompression bombs:** the output buffer is `raw_len + 1`. The decoder requires `StreamEnd`, `total_in == data.len()` and `total_out == raw_len`. FDICT is rejected, and truncation and trailing frames are covered by tests.
- **Closed canonical wire:** `deny_unknown_fields` is set, the hex is lowercase-only, and base64 is strict. `decode` re-encodes and compares to the input bytes.
- **Worst-case size:** the worst-case test covers every variant. By my arithmetic the worst Open (128 non-empty args) is about 22.4 KB, which fits under 32 KiB, so that is not a gap.
- **Offsets, EOF and ACK:**
  - Offsets are capped at `i64::MAX` with checked arithmetic.
  - EOF is only at the accepted end, and matching EOF is idempotent.
  - ACKs must not go backward or past the produced offset.
  - Failed boundary calls do not change the window.
- **Receipt cache and memory:** receipts are evicted only after consumption, at most 256 chunks are queued, and reclaim happens only after the admission checks pass.
- **flate2:** it is pinned at `=1.1.10` with `rust_backend` (miniz_oxide). The workspace resolves the same version and backend, so the lockfile shows no feature-unification surprise.
- **CI:** adding `-p marmish-stream` to the test line in `v02-native.yml` is correct.
- **Scope:** `Cargo.lock` and `Cargo.toml` changes are limited to registering the new crate. I did not review the phase 2 runtime doc.

After finding 1 is fixed, I expect to approve. I made no edits to the repo, and I ran no network, live or release steps.

## Disposition

The native license root addition was reverted: the unlinked primitives crate is
not part of the shipped native binary yet. Both existing license suites pass
(19 tests). This avoids manufacturing a fixture that misstates production roots.
The cancellation-state inconsistency and nonbeneficial zlib representation now
fail validation, with regression tests. `decode_request` explicitly documents the
required trusted-clock `validate_at` call for new Open admission, in addition to
authentication and the durable replay fence. Structural decoding does not claim
remote authority. All 16 crate tests pass after these fixes.


## Round 2

**Verdict: not APPROVE yet. There is one blocking finding, a trivial clippy failure in the new test code. Everything else from my prior review is fixed.**

**Blocking**

1. `crates/marmish-stream/src/codec.rs:200`: `encode(0, &vec![b'a'; 128], Codec::Zlib)` in the new `rejects_nonbeneficial_zlib_representation` test trips `clippy::useless_vec`.
   - `cargo clippy --locked --offline -p marmish-stream --all-targets -- -D warnings` fails with `could not compile marmish-stream (lib test)`.
   - Library-only clippy (no `--all-targets`) passes.
   - Your handoff said tests pass, but it didn't mention clippy. The repo rule (`/home/openclaw/CLAUDE.md`) requires strict clippy to be clean, and the phase doc lists strict clippy under acceptance.
   - Fix: change it to `&[b'a'; 128]`, then re-run clippy with `--all-targets`. I made no edits, as instructed.
   - I expect to approve once that is done. I found nothing else blocking.

**Prior findings, checked against the final files**

| Prior finding | Result |
| --- | --- |
| 1. License `ROOTS` edit | Fixed. `git diff -- tools` is empty. This is correct while the crate isn't linked into native, and the `agent-spec` paragraph says so. |
| 2. `Cancelled` without `cancel_requested` | Fixed at `wire.rs:284`. The regression test `cancelled_terminal_requires_accepted_cancellation` covers both the decode path (JSON edited to `false`) and the encode path. |
| 3. `decode_request` and `validate_at` | Fixed by documentation (`wire.rs:322-328`). It states the caller MUST call `validate_at` before admitting a new Open, and that recovery uses persisted identity. That is acceptable for a pure crate. Enforcement belongs to phase 2a, so please keep a note on that item. |
| 4. Zlib representation not smaller | Fixed at `codec.rs:48`. The regression test checks `validate()` and `decode()` at `raw_len == data.len()` and one below. |

**Against `docs/phases/streaming-primitives.md`**

- Limits (8 KiB blocks, 256 KiB window, 256 records, 128 argv items, 16 KiB argv, 32 KiB envelope, `i64::MAX` offsets) all match the code.
- The closed schema, canonical re-encode check, strict base64 and lowercase hex all match. Every variant has a worst-case fit test.
- The zlib codec rejects FDICT, truncation, trailing bytes and frames, and bad length or hash. The output buffer is `raw_len + 1`.
- The windows behave as specified:
  - Acknowledgements are monotonic and bounded.
  - Retries return `Ok(false)`, `OldRetry` or `Conflict`.
  - EOF is only valid at the accepted end, and matching EOF is idempotent.
  - Receipt reclaim happens only after all admission checks pass.
- The stream-ID age check applies only to Open, with an injectable clock.
- The flate2 pin matches the doc (`=1.1.10`, no default features, `rust_backend`). The CI test line adds `-p marmish-stream`.
- The `Cargo.toml` and `Cargo.lock` changes only register the crate.
- The `docs/agent-spec.md` "Streaming initiative (source only)" paragraph is present. It says phase 1 is not connected to the daemon or shipped CLI. It also says runtime, cancellation, rsync and transfer are planned, not supported release capabilities. That is accurate.
- `docs/build-plan.md` has a matching "planned until implementation evidence" note.

**Commands actually run** (offline, no network, no native builds, no repo edits)
- `cargo test --locked --offline -p marmish-stream`: 16 passed, 0 failed.
- `cargo fmt --check -p marmish-stream`: clean.
- `cargo clippy --locked --offline -p marmish-stream --all-targets -- -D warnings`: **fails** (finding 1).
- `cargo clippy --locked --offline -p marmish-stream -- -D warnings`: passes.
- `python3 -m unittest discover -s tools -p 'test_marmish_*licenses.py'`: 19 passed.
- `git diff --quiet -- tools`: unchanged.

I did not re-run the earlier randomized and fuzz checks. I re-read the changed code paths instead, and the changes are small and local.

No human or queue gate was assumed, per your note.


## Round 3

**APPROVE.** I found no blockers in phase 1 of `marmish-stream`.

**The fix.** `crates/marmish-stream/src/codec.rs:200` now reads `encode(0, &[b'a'; 128], Codec::Zlib)`. That was the only finding in the prior review.

**Validation I ran** (offline, no edits, no native builds):
- `cargo clippy --locked --offline -p marmish-stream --all-targets -- -D warnings` is clean.
- `cargo test --locked --offline -p marmish-stream` passed 16 tests with 0 failures.

**Prior review.** I read `/tmp/marmish-stream-primitives-code-r2.json`. It checked the other findings against the final files and found them fixed:
- The license `ROOTS` edit is reverted, and `tools` is unchanged.
- The `Cancelled` terminal state now requires an accepted cancellation, with a regression test.
- The `validate_at` requirement is documented.
- The limits, schema, codec, window and `flate2` pin all match `docs/phases/streaming-primitives.md`.

**Not re-run.** I did not re-run `cargo fmt --check`, the license tests under `tools`, or the earlier randomized and fuzz checks. The prior review ran the first two and found them clean. It re-read the changed code paths rather than re-running the fuzz checks. The `codec.rs` change since then is a single test line, and I did not check the rest of the tree beyond `git status` for anything else that moved.
