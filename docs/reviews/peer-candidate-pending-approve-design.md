# Manual Pending approval design review

Claude Code Opus 5.5 in `--permission-mode auto` approved the
[local approval design](../phases/peer-candidate-pending-approve.md)
at `41860d8f7029de97d83861f4b8591a9740d41010`, design blob
`90d501210c76d73399f985d92d027816a50489f8`. The final session was
`2b7ecee7-e939-43fd-810d-6eea76f140a0`. The exact source baseline was
`78b4d39`. The final reviewer verified the separate installed family and
exact v2/v3 checks, the same-incarnation authenticated history gate, both
SQL floors, bounded short exchange, local versus integrity fault
handling, and same-request durable readback.

R1 session `ec641326-dc33-409a-989e-a4ac0db6dad0` requested changes on
`242f0a76585764d56cba3a1ccd06d59713ba7d81`. R2 fixed explicit private
intent selection, the SQL Manual-only transition, bounded one-place
admission and narrow native retry, with conservative lost-reply handling.
R2 session `b214f16f-190b-4b4a-9fd6-04c400d38a76` requested changes on
`4386250a45a6eb28501f80398184a5adbc5e9052`. R3 preserved the exact v2
inventory and verifier and gated first approval and retry on completion of
the current incarnation's authenticated history.

During implementation, keep `installed::verify_rows` in the v3 verifier.
Document that restart may temporarily make a retry Busy or unavailable.
Record client and listener timing skew in the IPC evidence, including a
client timeout that precedes the listener's deadline.

This review covered the design and existing source only. No runtime
validation, build or network test was performed. Native PostJoin and
independent founder confirmation remain separate phase gates.
