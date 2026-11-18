# Managed Consent and Delivery design review

Claude Code CLI in Auto mode, Sonnet 5.5, approved the managed package
and read-only challenge at `ab6ec2ac034410303a1a93ebf495070bd2ceb7cc`
(design blob `aec6d6e3600985dd2f026426625e626d32e031df`). Session
`8f6229a7-33b0-4788-b9e7-a3f4ccc3e5c2`. The reviewer checked
exact source `adb8c6f` through bounded local reads with no changes or
builds. Offer/Consent and Delivery remain separate compile and review
cuts.

The first review requested changes at `8b52ff7`, session
`754c6b6f-c2b3-4680-867b-d1946cb7806c`. The revision assigned item-level
compile gates and narrowed ownership. It moved both turns into
the inventory owner, separated the queued challenge from BoundStage,
and documented exact current and historical retry checks.

The read-only aperture verifies the consent singleton floor via
`Context::verify` and `rows::check_wall` (`peer_consent_version.wall`),
then the selected origin floor (`peer_cursor_origin.wall`) in the same
transaction. It leaves both as found. This aperture reads an exact
post-commit outbox survivor. Legacy `recover_queued_consent` is unchanged
and continues to own its existing floor behavior. The first cut carries
the package witness and read challenge. Signature changes to old
publication and Delivery proof consumers belong to their later
cuts, with fixture-only joins left intact.

## Cut 1 code review

Claude Code CLI Auto, Sonnet 5.5, approved the initial implementation
`dd28950deba0d410644df632cfaa6d5c5d70ab16` in session
`4ca78f7d-25e9-4102-9c30-6d7fa25d7e20`. The author added real
refusal coverage and tightened its assertions. The reviewer approved
`32d87d0` in session `e47954a6-b962-43c8-a61e-64643120409c`
and independently ran the queued restart regression in 19.48 s.
The nine strict-lint interface errors arose only after the installed
reader shared the checkpoint module. The final amendment preserved
the original method declarations and widened only the three test
witness type names to crate scope. The actual witness module mounts
remain test-only in `peer_invitation.rs`. Constructors and fields stay
private.

Claude Code CLI Auto, Sonnet 5.5, approved the exact final source
`6aa749ddcb6440e4bb9087a16c1c3a9c41b46e7d` in session
`6dc20157-3a54-4813-9938-3a8dddba8da7`. The review checked the
amendments against `32d87d0` and the original method signatures
against `adb8c6f`. It ran the archived exact SQLCipher restart
fixture in 17.40 s, including foreign owner, modified image,
identical bytes on the wrong inode, retained journal and sticky stop.
The author independently ran strict stock and candidate Clippy
across both packages and all targets. The separate attempt-change
and ACK stale-witness regressions remain required in the next cut.
