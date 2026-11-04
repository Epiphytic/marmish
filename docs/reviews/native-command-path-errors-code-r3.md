# Native command PATH and errors — code review round 3

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Session: `571a9b66-a3f9-4415-a103-cfd5936d02ae`.

# Verdict: APPROVED

This is a static read-only review. I ran no Cargo and no tests, and I started no live sessions. I found no blocking findings. The 480-pass/1-failure run was root's, and root's gates still decide the outcome.

**Model:** Claude Sonnet 5.5 (`claude-sonnet-5-5`), no sub-agents.

## Checks actually performed
- **Read:**
  - the r2 review;
  - the full diffs of `command_store.rs` and `command_service.rs`;
  - `project_completion` in full;
  - `execute_operation` and `execution_refusal` in `command_engine.rs`;
  - `command_begin_operation`'s dispatch barrier and `InvocationRecord::active()`;
  - `command_recovery`, `schedule_active`, `validate_cleanup_row` and `cleanup_counts`;
  - `completion_publications`;
  - the `tests/engine.rs` and launcher test diffs, plus `policy::executable_names` and `inspect_canonical_exec`;
  - the phase-doc disposition text.
- **Formatting:** `rustfmt --check` is clean on the two native files, with edition 2024. It is also clean on `tests/engine.rs` and `launcher/tests.rs`, with edition 2021.
- **Beads:** `marmish-wyl` exists and is open, so the disposition's tracking reference is real.
- **Not done:** I did not run the new test or re-derive the 480/1 result myself.

## The `project_completion` Refused fix is sound
**Where Refused comes from.**
- Only `execute_operation` produces it for a pre-engine refusal.
- That requires an `EngineOperation::Dispatch` with `known == false` and `status() == NotFound`.
- Every other operation returns `Missing`, so no `Refused` can reach an Observe.
- A `Failed(Uncertain)` result never becomes `Refused`. `execution_refusal` sends operational errors to `failed()`.
- A submit that succeeds but leaves no durable facts returns `Failed(Uncertain)`.

**Why clearing uncertainty is safe.**
- A Dispatch is only admitted at `Admission::Accepted` with `!engine_known` and no refusal (the `command_dispatch_barrier`).
- A record that went `Dispatching` and then failed uncertain cannot be re-dispatched. It stays `Dispatching` and is only observed.
- So `uncertain && Accepted` can only come from a failed Observe before any dispatch.
- In that case the Dispatch's own `status() == NotFound` plus a pre-submit refusal is authoritative. It supersedes the old uncertainty.
- That path mirrors the existing `Missing` branch (`Accepted`, `uncertain = engine_known`).

**Real unknown-launch facts are untouched.**
- `Failed`, `Missing` and the engine-known Refused branch (`KnownFound`, unchanged) do not change.
- The pre-existing assertion `r.uncertain && Dispatching` after `Failed(Timeout)` still holds.
- `cleanup_counts` still counts `!engine_known && Dispatching` as uncertain, so the failure was exactly the stale `Dispatching` row. After the fix those rows are `Accepted`.

**Projection usages.** `project_completion` is called from `command_commit_completion`, `completion_publications` (the same pure projection) and the tests. All of them see the same record.
- `active()` is false because `refusal.is_some()`.
- `command_recovery`, which selects `active=1`, therefore excludes the row, and `schedule_active` never re-dispatches it.
- `validate_cleanup_row` accepts `refused_before_engine` with `!uncertain` and `observed_seq >= fence`.
- The ACK decision is `DefinitivelyRefused`, which the worker resolves for `Fact::Refused`.
- The digest, the request slot and replay data are unaffected. The `status()` projection now reports `Accepted` for a refused record, the same as a fresh submit.

## Tests
- **`executable_refusals_reopen_replay_and_release_active_budget`** covers five refusal codes through a real Dispatch with a seeded `uncertain`. It asserts all of the following across a journal reopen:
  - `Dispatching` before the completion and `Accepted` after;
  - `uncertain` cleared;
  - `cleanup_counts` showing zero active and zero uncertain, and `observation_pending` false;
  - empty recovery;
  - the operation row removed;
  - the dispatch barrier still enforced;
  - the digest unchanged and `find_request` still replaying;
  - the per-principal active limit of 1 released.
- **Cleanup fixtures** are correct now that refusal is asynchronous.
  - They pump completions until the refusal is durable.
  - They loop on `command_cleanup_busy` under the existing `deadline`, so the wait is bounded.
  - `validate_cleanup_row` is asserted to fail for a fresh accepted row before the refusal is set.
- **`engine.rs`:**
  - Bare `printf` gives `InvalidRequest`, because `executable_names` rejects a non-absolute first byte.
  - `/usr/bin/false` and Shell give `CommandDenied`.
  - The traversal and symlink spellings, and the nonexistent path, give `InvalidRequest` through `require_canonical_exec` and `Unresolvable`.
  - `EngineError` derives `PartialEq`, so `assert_eq!` compiles.
- **Launcher rename:** `direct_exec_image_uses_exact_envp_without_inheritance` now claims only what it asserts. This resolves r2 item 1.
- **Docs:** The dispositions cover parent-dot normalization and durable Unauthorized/QuotaExceeded refusals. The rest is tracked in `marmish-wyl`, and no EBADARCH runtime claim is made.

## Nonblocking items
1. **`project_completion` does not check the operation kind for a pre-engine Refused.** It accepts `Refused` for any operation, and the service fixture `cancel_fixture_key` commits `Refused` against an Observe. The engine cannot produce that pairing, so there is no real hazard. Requiring `EngineOperation::Dispatch` in that branch would turn an engine regression into an `invalid` error instead of clearing uncertainty. The fixture would then need a Dispatch.
2. **The reopen test covers five codes.** `AuthorityRevoked`, `InvalidRequest`, `QuotaExceeded` and `Unauthorized` use the same branch, so this is a coverage gap rather than a logic gap.
3. **A stale-revision Refused still sets the refusal.** This was already true for the refusal field. Now it also resets `admission` and `uncertain`. I found no path where a revision bump happens with `!engine_known` and a live Dispatch registered, so I could not construct a failure.

## Gates still open (root)
- The full native run. The one failure should be the retained-`Dispatching` case, so it should now pass.
- `cargo test -p marmish-engine`, `-p marmish-prototype`, and `-p marmish-native --all-features`.
- `cargo fmt --check` and `cargo clippy --workspace --all-features -- -D warnings`.
- The live relay validation record and the Linux and macOS CI runs, as listed in r2.

Disposition: nonblocking defensive operation-kind and additional refusal-code coverage remain follow-up work; the production engine only emits this definitive refusal for a new Dispatch. Root owns runtime validation. A subsequent Clippy suggestion replaces lazy Option::and_then with equivalent Option::and.
