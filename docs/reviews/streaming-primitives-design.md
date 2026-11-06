# Streaming plan and primitives design review

Reviewer: Claude Code, claude-sonnet-5-5, permission mode auto.
Scope: overall streaming-transfer-plan.md and phase1 streaming-primitives.md.
Direct user task; no BTQ queue pickup. Existing user/AGENTS authorization covers
implementation after technical review. Queue-only human approval is inapplicable.

## Round 1

**Verdict: REQUEST_CHANGES.** The overall shape is sound and phase 1 is a reasonable first increment. Four blockers need fixing in the docs before coding starts. I read the two docs and the engine and native contracts they depend on, and made no edits or builds.

## Blocking issues

**B1. The wire size budget doesn't fit the transport.**
- The phase 1 doc sets a 128 KiB envelope, 256 KiB windows and encoded blocks of up to 20 KiB.
- The existing Marmot adapters cap application content at `MAX_CONTENT = 32768`. The cap is enforced at `command_wire.rs:11`, `foundation.rs:10` and `command_adapter.rs:398/168`. The MLS app-event ceiling is `MAX_CONTENT + 4096`.
- A single `Exchange` response carries independent stdout and stderr blocks. At 20 KiB each, base64 in JSON makes that about 54 KiB, which is over the cap.
- Phase 1 needs a derived ceiling: the maximum encoded message, the encoding (JSON/base64 or binary), the number of data blocks per message, and a worst-case-fits test. If you intend to raise the adapter ceilings instead, say so, and require evidence that the default White Noise relays accept those event sizes.

**B2. The schema is "closed" before the exchange model exists.**
- Exchange scheduling is deferred to phase 2, but `Exchange` (stdin block, stdin final offset, consumed cursors, output blocks) is the scheduling model. Poll-driven versus push, and one exchange in flight versus pipelined, change its shape.
- Either sketch the exchange state machine and a goodput/latency model in phase 1, or mark `marmish-stream/1` provisional until phase 2 is approved. The plan currently lists it under "Frozen interoperability choices".
- Several terms are undefined:
  - "timestamp-bound" stream ID: which clock, what skew, what replay window.
  - The "bounded recent receipt cache": its size and eviction rule.
  - "consumed", "accepted" and "delivered" cursors: how each differs from an ACK.

**B3. The doc doesn't relate the primitives to existing engine primitives.**
- The engine already has an idempotent `write_stdin` and `stdin_eof` keyed by generation and offset. It also has a cursor-based `read_output` limited to 16 KiB, `InputStatus` with accepted, delivered and credit, and an on-disk spool that records gaps at its cap.
- `SendWindow` and `ReceiveWindow` are a parallel in-memory abstraction. The doc never says which engine API each maps to.
- Phase 2 also needs "full buffers block pipes". That is a worker and spool behavior change, which conflicts with "command/1 unchanged". It brings schema or `ENGINE_FORMAT` migration, and old detached workers surviving an upgrade.
- Add a short mapping table (primitive to engine call) and list the engine changes phase 2 will need. Without it, the crate API risks rework.

**B4. The plan understates what allowing rsync grants.**
- It says allowing rsync grants "ordinary same-UID filesystem access". Restrictions are executable globs only (`allow_exec` and `deny_exec` in `policy.rs`), with no argv constraints.
- A peer allowed to run rsync can write a file and then run `rsync --daemon --config=…` over stdin. A daemon config can use `pre-xfer exec`, so this is effectively arbitrary same-UID command execution.
- Correct the plan to say that. Phase 2 must either validate the argv shape for an rsync profile (require `--server`, reject `--daemon`/`--config`) or explicitly document that authorizing rsync equals authorizing command execution.

## Useful improvements (non-blocking)

**Phase 1**
- **Dependency:** `flate2` is only transitive today, via arti's `tor-dirclient`. As a direct dependency it should be exact-pinned (the workspace pins sensitive crates with `=`), with `default-features=false` and an explicit backend. `Cargo.lock` has both `miniz_oxide` and `zlib-rs`. Add a test that rejects an FDICT zlib header.
- **Release and CI plumbing:** `tools/marmish_release_licenses.py` `ROOTS` lists crates explicitly. `v02-native.yml:166` runs only `-p marmish-native -p marmish-vault`. Add `-p marmish-stream` there and update the license roots.
- **Compression leakage:** compress-then-encrypt leaks size, so document it. Default to `none` unless the user opts in.
- **Argv limits:** the doc says 128 items and 16 KiB; the engine admits 256 and 64 KiB (`admission.rs:110-122`). Stricter is fine, but state it, and require the NUL and empty-`argv[0]` checks to match the engine's.
- **Purity:** add a check that the crate has no `engine`, `native` or `tokio` dependency, and use an injectable clock for the receipt horizon and stream ID. Add fuzz or property tests on the decoder, and redact argv in `Debug` output, since it can carry secrets.

**Later phases**
- **Split phase 2** into 2a (runtime with a native echo helper, restart and uncertain-loss handling, quotas, idle timeout) and 2b (`marmish rsh` plus rsync interop). As written it is too large for one review.
- **Phase 2 design questions:**
  - Stream quotas against `active_jobs=32`.
  - The 60 s `ExecuteToken` lifetime and the `fresh_for ≤ 60s` input limit. A long-lived stream needs per-exchange reauthorization.
  - The child environment is fixed at `PATH=/usr/bin:/bin` (`launcher.rs:921`), so Homebrew rsync is reachable only through an absolute `--rsync-path`.
- **`rsh` behavior:** define exit-status mapping like ssh (255 for transport failure, 128+n for signals). Define stdin half-close and early-stdout-close (SIGPIPE) handling. When both are given, require `-l NPUB` and `HOST` to match or reject them. Add a guard against missing `-s`, because rsync would otherwise send backslash-escaped args that a non-shell exec reads literally.
- **Phase 4:** record rsync versions per platform. From memory, which I did not verify, newer macOS ships openrsync with fewer features.
- **Goodput:** state an expected goodput model, since 16 KiB per relay round trip is latency-bound unless exchanges are pipelined. That gives phase 4 something to measure against.

## Scope and process notes
- I did not inspect the worker's pipe-backpressure internals or relay event-size limits. B1 and B3 rest on the adapter constants and engine APIs I read.
- This review is only the second-LLM design leg. Per the queue protocol, human design approval is still required. The plan's line "The user authorizes implementation…" is the author's claim and I haven't verified it.
- Re-review is cheap once B1–B4 are addressed in the docs. Implementation should wait for that.

## Round 2

**APPROVE.** I found no remaining blockers in either doc, for phase 1 or for the overall plan. This is my technical assessment only. I did not verify the plan's line that the user authorizes implementation, and nothing here stands in for a human approval. I read only the two docs and the first review. I did not re-check the adapter or engine constants, so the 32 KiB cap and the 36 KiB app-event ceiling are taken from that review.

## First-review blockers

- **B1, size budget: resolved.** The arithmetic holds.
  - An 8 KiB block is 10,924 bytes in base64, so two blocks are 21,848 bytes. That leaves about 10 KiB under the 32 KiB envelope for metadata, which is ample.
  - The envelope is 32 KiB and the MLS wrapper is at most 36 KiB, so both match the adapter caps.
  - Open argv is about 22 KiB, which also fits.
  - The doc requires a worst-case serialized-fit test for every variant. That test is the real guard, so keep it in the acceptance list.
- **B2, provisional schema and cursors: resolved.**
  - The schema is now explicitly provisional until phase 2a approves it.
  - The exchange model is specified: one outstanding data Exchange per stream, and a separate Cancel/Status control path.
  - Accepted, delivered and consumed are defined, and only the consumed cursor authorizes an output ACK.
  - The receipt cache is 256 entries, evicted oldest-first only after consumption, and older retries fail explicitly.
  - The stream ID has a defined clock (Unix seconds), a 7-day age limit and a 60-second skew limit, checked with an injectable clock.
- **B3, relation to the engine: resolved.**
  - The mapping table is there.
  - The plan commits to a separate live-child path, with no finite spool mode, no `ENGINE_FORMAT` change and no change to existing detached workers.
  - Streaming state is memory-backed and a daemon restart marks it Lost without relaunching, which is a coherent recovery rule.
- **B4, rsync authorization: resolved.** The plan now says plainly that authorizing the rsync executable effectively authorizes arbitrary command execution as that UID. It also says executable restrictions are not an argv or filesystem sandbox.
- **Compression and cancellation: resolved.**
  - Compression defaults to none, and the plan documents the size-leak risk of compress-then-encrypt.
  - It uses exact-pinned `flate2` with default features off and an explicit `rust_backend`. It rejects FDICT, bounds input and output, and updates the CI package list and licence roots.
  - The cancellation section covers owner-bound idempotent cancel, an authenticated cancel independent of a blocked exchange, and never signalling a recycled PID. It also says disconnect is not cancel, and it lists the test cases.

## Non-blocking points for the docs or phase 2a

1. **Idle polling.** The scheduler says the server replies immediately with whatever data or status is available. During a quiet period, such as rsync computing checksums, that makes the client loop at relay RTT and flood the relay. Phase 2a must define a long-poll or backoff. While the schema is provisional, consider a server-side wait hint or retry-after field.
2. **Heading wording.** The plan section is titled "Frozen interoperability choices", but its first sentence says the schema is provisional. Rename the heading.
3. **Stream ID age check.** State that the 7-day age check applies only at Open. Otherwise a long-lived stream could be rejected on a later exchange.
4. **Backend feature unification.** The lockfile already has both `miniz_oxide` and `zlib-rs` through arti's `flate2`. Cargo's feature unification could change which backend is actually used. Pin to the lockfile's resolved version, and test that frames decode correctly with either backend.
5. **Phase 2a items from the first review.** These still need explicit coverage in the 2a design: the stream quota against `active_jobs=32`, the 60 s `ExecuteToken` lifetime for long-lived streams, and the fixed child `PATH`. 2b still needs `rsh` exit-status mapping (255 for transport failure, 128+n for signals) and stdin half-close and SIGPIPE handling.
6. **Idle timeout.** The plan says network loss never cancels a stream, so the phase 2a idle timeout is the only thing that reaps a vanished client's child. Make its default and bounds explicit there.

Implementation of phase 1 can proceed once a human approves the design. Phases 2 to 4 still need their own design reviews, as the plan already says.

## Disposition

All four technical blockers were corrected before implementation. Phase 1
implementation starts after round2 APPROVE. Renamed provisional heading; age
validation is for new Open only. Idle/backoff and lifecycle deadlines remain
phase2a design requirements. No independent human approval is claimed.
