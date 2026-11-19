# Installed later Offer fixture review

Claude Code Sonnet 5.5 ran in Auto mode. The design review covered the
existing installed Manual approval paths and the exact private fixture.
The author source review was static. Candidate compilation and private
loopback execution belong to the integration run.

- Design R1 requested changes on `f57688d`. Session
  `db6f9e1d-ebf2-48a8-9a21-5f4aa2ec9c5d` identified that the existing V2
  offline selector could not verify the ApprovalV3 cursor and asked for
  concrete signed-clock and relay-response observations.
- Design R2 approved `f77103e`. Session
  `eaa80666-208c-47d7-9418-f80d6aaf34e7` verified the exact ApprovalV3
  read-only proof, the matched authenticated history EVENT and EOSE, and
  preservation of the original Pending without approval.
- Source R1 approved `a7d7b09`. Session
  `dadf8cc0-3985-4db6-b7a6-977e856ebfe6` checked the relay correlator,
  exact durable readback, and test-only gates. It suggested guarding
  second-resolution signed timestamps and accounting for listener teardown
  when asserting the refusal. The author added bounded waiting on the
  founder fixture's existing signing clock in `817dd1e`. The integration
  fixture accepts either typed unavailability or EOF uncertainty after the
  observed worker exit, then checks the same durable Pending.
- Source R2 approved `817dd1e` and the integration refusal contract. Session
  `761a34d1-7b0a-4b45-83a7-ff729c940edb` confirmed the original fixture
  clock and bounded polling. Either refusal is valid only while the
  checked post-stop snapshot still proves unchanged Pending.
- The integrated source through `ba92201` received approval in session
  `a2ff967e-6fad-421f-916d-f9ff339407a0`. The review checked the authenticated
  history witness, the original Pending and keeper readback, ordinary
  Command continuity and test-only isolation. It suggested separately
  counting Consent invite, fence and outbox rows for broader coverage.
  The fixture checks the exact approval state, Offer marker, deferred
  row count, retained MLS file and relay publication count.

The ignored candidate selector is
`installed_registered_manual_later_offer_preserves_pending`. Its scope is
normal authenticated private WSS history refusal, exact previously retained
Pending custody and ordinary Command continuity. The current product rejects
the later Offer and retains the original Pending.
Focused Rust formatting and `git diff --check` passed at `817dd1e`.
The [installed run](../validation/peer-candidate-local-approval-mount.md#a-later-signed-invitation-during-authenticated-history)
passed with the integrated fixture. The reviewer identified authenticated
founder convergence as the next design under the current custody policy.
