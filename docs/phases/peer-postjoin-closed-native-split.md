# Closed native PostJoin split proof

Qualify the public MDK continuation used by the
[private convergence phase](peer-candidate-postjoin-convergence.md).
Show that a real SQLCipher recipient can close after its first native
`PossiblyExposed` result and settle the exact frozen request after a cold
reopen. The proof uses two independent native devices and a local typed
acknowledgement stand-in. Actual relay provenance is a separate qualification.

## Source and ownership

- Add `crates/marmish-native/src/peer_postjoin_split_tests.rs` as a private
  test child of `crates/marmish-native/src/peer_postjoin_proof_tests.rs`.
  Keep existing proof entry points, adapter and tests compatible. Use one
  opt-in test selector, `exact_postjoin_settles_across_closed_native_turns`.
- Reuse `reach_activating`, `prepare_history`, the witnessed stage reopening
  in `open_native`, founder native ingest and exact `peer_postjoin_verify`
  checks. The existing `open_native` selects `Gate` and immediate timing.
  Keep those defaults for its original caller. Share only its checked store
  opening and route construction with the new no-network test adapter. Use
  normal MDK timing for this selector.
- Bind a fresh private disposable profile, real Manual Activating row and
  stage inode, own group/route and native source. Reuse the checked transition
  with a same-UID `UnixStream` pair in the staged fixture, as in
  `peer_pending_join_wss_tests.rs`. Preserve the original test's Automatic
  policy and reuse local history completion backed by equal live pins. The
  later installed proof separately qualifies authenticated founder history.
  Derive test wall from an anchor plus actual elapsed time and retain the
  monotonic clock. If the existing fixture duration is too short, choose a
  bounded disposable signed window for this selector only. Use the real
  300-second effective cutoff for the later installed proof.

## Qualification

1. Advance the real native obligation to its first authoring turn under the
   defaults. In a single poll, capture one exact typed commit request and
   return `PossiblyExposed` with `pre_send: false` for its sole endpoint. Keep
   the default `commit_size_refusals`. Deny and count known KeyPackage calls
   per sweep. Refuse any other request or pending poll. Check exact native
   semantics and the durable first attempt before closing the runtime.
2. With no recipient runtime, SQLite handle or staged file guard live, open
   a fresh measured readback of the witnessed image and verify the single
   obligation, source pin, signed request, evolution, pending ref, attempt
   count and due time. Close it and confirm the original inode and clean
   sidecars. Deliver the frozen request through genuine native ingest into the
   independent founder and verify its processed ID, stable successor and
   changed recipient leaf. Issue the fixture acknowledgement only after that
   outcome. Give the receipt no installed or physical transport authority.
3. Before native due time, verify that no settlement callback runs. After the
   real persisted due time, reopen the exact recipient stage with a fresh
   bounded adapter that can return the fixture acknowledgement only to the
   same frozen request. Verify one capture and one settlement callback, zero
   socket requests and zero unrelated publications. Allow one fenced native
   maintenance turn to finish the obligation if the API requires it. Close
   again and prove stable recipient and founder pins, exact confirmed native
   signed ID, and unchanged custody charge and Activating decision on cold
   readback. An additional closed verification must produce zero effects.
4. Use small deterministic scope checks to reject a changed request or stage
   witness and an unfinished `Attempting` outcome before any adapter poll.
   Test the recovery decision for interrupted capture and settlement without
   permitting another attempt. Test that a completed matching native rotation
   uses only readback. Keep these checks local to fixture-owned evidence.

No daemon, CLI, layout, protocol, CI or dependency change is needed. Avoid
claimed end-to-end relay or installed performance from this proof. Record the
exact locked source, opt-in outcome, capture and settlement counters, elapsed
quiet/jitter and native due wait, readback and total runtime. Expect about
90–135 seconds after native setup and approximately 130–180 seconds in a
warm fixture run including fresh keyed stores. These are estimates from
default waits and prior adjacent fixture timings. Use a bounded 240-second
outer test timeout. Run the original opt-in native PostJoin test separately
and obtain independent Claude Code Auto review of the exact code before
enabling any later qualification phase.
