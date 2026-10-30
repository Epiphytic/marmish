# Managed installation and unified service overall review

Independent Claude Code Sonnet, permission mode auto; read-only design review.

## First review

**Verdict: not APPROVED as written. It is approvable with the four blockers below fixed. The direction itself is sound.**

I checked the plan against `scripts/install.sh`, `tools/marmish_release.py`, `tools/marmish_app.py`, `tools/marmish_policy.py`, `crates/marmish-engine/src/admission.rs`, and `docs/agent-spec.md`. Nothing was edited or run.

## What holds up

- **No full Rust rewrite.** The plan does not require one. It keeps the Python live transport (the Rust MDK adapter is offline-only, per the plan) and extracts a service from it.
- **uv is feasible for the runtime.**
  - The release launcher in `tools/marmish_release.py` already runs `python3 -I -B` against a bundled `tools/` directory. A fixed absolute interpreter in a version-owned environment is a small change from that.
  - Python is not packaged today. There is no `pyproject.toml` or `setup.py`, and `install.sh` hard-requires a system `python3` 3.11+ (lines 4, 140, 799).
  - So "archive plus pinned interpreter" is the lowest-risk first layout. A platform wheel can wait for the spike evidence.
- **The existing guards are acknowledged.** The plan keeps the key-session and transport locks. They are the per-profile lock in `acquire_profile_home_lock`, the `KeySession` catalog binding, and the no-file-fallback rule for native keychains.
- **The macOS confinement abort is kept out of scope.** It does not claim packaging fixes it.

## Blockers

1. **Empty authorization cannot be represented today.**
   - `server_main` declares `--allow` as `required=True`.
   - `Server.__init__` requires `1 <= len(allow) <= 32`.
   - The Rust engine rejects empty `allowed_principals` in `admission.rs:12`, and `allowed_principals` is baked into the engine init policy (`_server_policy`).
   - So "Both with no npub, inbound disabled" cannot mean "run the engine with an empty list". It must mean the inbound path and engine frontend are not started at all.
   - The plan also leaves allowlist edits unspecified. Is a change an engine re-init, and how does that interact with the persisted policy comparison and the config version-2 marker? It needs one stated rule.

2. **Policy keys must stay a subset of the allowlist.**
   - `validate_policy` requires every per-key `principal` to be in `allowed`.
   - Removing the last peer, or a no-peer setup with an existing policy file, would either invalidate the policy or silently drop restrictions.
   - Decide whether policy is stored and checked independently of the enabled state. It must never be dropped, and re-enabling must reuse it, not loosen it.

3. **Durable enablement needs a single authority, and the plan does not name it.**
   - The plan puts "requested role", "server enabled" and "allowlist" in new versioned config.
   - Today the allowlist is a CLI argument (`--allow`), not stored state.
   - Name the one source the service reads at start and on reload. Old `marmishd --allow …` invocations must not be able to override a durable disable.
   - State the migration for existing profiles, including whether an existing `--allow` deployment counts as already enabled. That choice is a compatibility decision, not an implementation detail.

4. **The uv bootstrap trust chain is unspecified.**
   - The plan keeps the archive verifier but does not say how the uv binary and the managed CPython are authenticated.
   - Requirement: pinned versions with digests held in the reviewed script or manifest, no `latest`, no ambient `UV_*` or index configuration, and no network at service start.
   - Without this, the new bootstrap is weaker than the current installer's bounded, hash-checked path.

## Risks

- **Python version.** `distribution.md:344` records CPython 3.13+ `ast.dump` behavior breaking a test. The pin must be an explicit tested version, and the plan's "tested release" wording needs a concrete choice in increment 0.
- **Single-identity scheduling.**
  - The same account is both client and server. `Server.__init__` requires `profile.account not in allow`, which is fine.
  - The existing room and session bindings (`sessions` and `server_groups` in one profile) already coexist, so shared state is not new.
  - The real risk is the blocking `Client._exchange` and the `Server.tick` polling loop sharing one RPC and history window. The plan covers this, but increment 2 should make a bounded per-peer scheduler a hard design-review item.
- **Setup runs through the app.** A bootstrap that calls app-owned setup means `--doctor` and `--debug` ordering, root refusal, and quickstart's current "authorization lasts for the foreground invocation only" behavior must be carried over or explicitly replaced.
- **Increment ordering.** Role persistence (increment 3) depends on the owner API (increment 2). If the unified daemon slips, increment 1 alone still delivers "no system Python". Say that increment 1 ships independently.

## Useful changes

- Add an explicit table row or sentence for Client with no npub being valid and never touching the server path.
- Settle disablement semantics (response delivery, status and cancel access while disabled) at the design level before increment 3. The plan correctly defers this to a detailed design.
- Keep `-i` and the current `marmish`/`marmishd` flags working through at least one release, with the new `setup` and `service` commands added alongside them.

Fix the four blockers and I would approve the plan for per-phase design review.

## Revised approval

**APPROVED for detailed phase planning.** All four earlier blockers are now explicit requirements, and I found no remaining high-level blocker. This is a design review only. I did not check that any of the described code exists.

Each blocker is covered:
1. **Empty-peer gate:** empty inbound authorization never starts the inbound Server or engine frontend. Live allowlist edits stay unexposed until a reviewed engine-policy transition exists, and the engine is never reinitialized or its marker edited.
2. **Dormant restrictions:** inactive per-key restrictions are kept and validated. Active policy is built from active principals only, with removal and re-addition tests required.
3. **Single authority:** the owned 0600 configuration is the only role and allowlist authority. A pending-generation record closes admission on inconsistency. Conflicting CLI flags are rejected. Migration is explicit through `marmish setup`.
4. **uv and CPython trust:** versions and per-target SHA256s are held in reviewed release data, with config discovery and `UV_*`/`PIP_*` overrides disabled. A concrete patch release is chosen, and installation is refused if uv can't enforce the digest.

These items should go into the detailed designs. They don't block approval of the overall plan:
- **Transition API placement:** the engine-policy transition API and the config/engine crash-consistency proof aren't assigned to an increment. Increment 3 needs them for enable, disable and last-peer removal, so name them as Increment 2 or early Increment 3 deliverables. Increment 3 should then list them as a dependency.
- **Re-enable path:** enabling again after peers were removed has an existing engine with a stale policy. That goes through the transition API, not the "first inbound engine" creation path. State this in the role design.
- **Bootstrap trust root:** the curl script and the release-data manifest that holds the hashes need their own integrity anchor, such as a pinned digest, signature or release channel. Increment 0 should define it.
- **Interpreter digest:** if uv can't enforce a CPython digest, the fallback is to ship the interpreter in the release archive. Decide that in Increment 0, since the plan currently only says to refuse.

The detailed phase reviews still need to cover:
- the role-disablement semantics for response delivery, status and cancel
- the bounded scheduler
- the IPC schema
- the migration design

## Disposition

All four blockers addressed before detailed phase work. Follow-up clarification
assigns engine-policy/recovery API to increment2, requires it for role edits, and
identifies interpreter-in-archive fallback if uv digest enforcement cannot be
proven. This approves the overall plan, not implementation or runtime support.
