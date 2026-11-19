# Managed Delivery leaf code review

Claude Code CLI in Auto mode, Sonnet 5.5, approved exact dormant source
`35db4bb3a8a24b0d2f23ab65158497e54f3fb76e` against `3a224b9` in session
`9203d8d2-0020-4824-a11e-4c8c495df822`. The review was static. Independent
author execution is recorded in the [validation report](../validation/peer-candidate-managed-delivery.md).

The reviewer traced the candidate port borrow, turn seal, original encrypted
image re-digest, existing native window, exact queued Consent witness,
retained Delivery revision, and transaction rollback after the last fence.
It checked that stock handlers still use the existing transaction semantics.
The changed checkpoint comparison also verifies the exact stored artifact and
carrier ID before accepting Held.

The reviewer confirmed a gate on future activation. The current due query can
return an expired row, while the managed readback refuses at its signed cutoff.
An expired head therefore remains charged and can indefinitely starve later
rows. Keep the managed scheduler inactive until bounded retirement or deferral
handles that row without extending native permission or advancing either wall
floor on refusal. Existing fixture retirement remains available on its own path.

The review suggested retaining finer internal causes from the native window.
The leaf keeps the existing closed expiry category for that failed fence.
