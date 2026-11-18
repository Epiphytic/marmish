# Private candidate preflight registration review

Claude Code CLI `--permission-mode auto`, Claude Sonnet 5.5
(`claude-sonnet-5-5`), approved the [detailed design](../phases/peer-candidate-preflight-registration.md)
in session `8a3be424-96ac-4278-8bc9-ecb8e22733ea`. The reviewed commit is
`b619d1fef8ca2942c4d6b883e5dd9d9d37f6fb76`, design blob
`bbfc0359d6398c7d33f440ef67f9c86935cd2d95`, against `08ca5d7`.
The reviewer inspected committed files and performed no builds or tests.

The initial session `4c50ab5e-a34f-4c97-b4f5-3a47fbba4a82` requested
changes at `e55cc18`. The revision authenticates the supplied SQLCipher
material on every recheck, reserves the complete topology before the
irreversible default VFS registration, specifies the shared directory/file
witness and quota borrow, and confines directory creation to the register
call. The final review cleared all four blockers. It also approved exact
0700 refusal after restrictive umask and fatal refusal after a failed
SQLite initialization.

The code review should check the following nonblocking requests from the
design reviewer.

- Pass lease and material to the registered recheck. Treat a poisoned
  registration mutex and failed SQLite initialization as indeterminate.
- Immediately after registration, require the new default's `pNext` to
  equal the captured stock default. This follows the insertion order in
  the bundled libsqlite3-sys 0.37.0 SQLCipher source.
- During the later actor mount, create only the cursor in an already
  checked empty `peer/` directory. Refuse a missing cursor alongside
  existing reserved members.
- Keep exact directory mode on every check, including inherited mode
  deviations. Add negative cases for absent or tampered issuer tags and
  pre-schema-3 qualification records.
- Restrict Ready field/method gates and the receiver registration code to
  Linux AMD64 test or private candidate compilation as specified.

The local design phase opens no Peer store or socket. The later daemon
composition owns listener, actor and end-to-end qualification.

## Code review

Claude Code CLI in Auto mode approved exact code
`14df37d8fc7c570a9d09f0572b2e6b1324475d67` in session
`b45ac9cf-da87-42ff-a4d1-af42dc785d20` using Claude Sonnet 5.5. The
reviewer ran the archived, matching test executable and the registration
harness passed in 29.56 seconds. The author also ran the remaining
[local checks](../validation/peer-candidate-preflight-registration.md).

The first code review at `5e3d071` requested changes in session
`077da851-53d5-4dd8-ac53-ec0295c6e643`. The initial expired-lease test
asserted a re-registration refusal and could pass without exercising the
lease check. The corrected test releases the original lease and requests a
direct dispatcher recheck using the replacement lease. It separately checks
that forwarding survives and a second registration remains forbidden. The
revision also adds config-byte drift coverage and makes the isolated child
return before registration when no fixture environment is present.

The final reviewer found no blockers. The optional suggestion to inspect
the retired weak reference's strong count was left out. The test releases
each holder it acquired and asserts the dispatcher’s exact lease-conflict
result. The registered forwarding descriptor remains valid through that
refusal.

## Integrated review

Claude Code in Auto mode, Claude Sonnet 5.5, approved the integrated
registration source in session `1c7b13dc-ce1a-4df6-bdb7-34d268055f58`.
All nine registration files match their reviewed author versions. The reviewer
checked the composition with Delivery, Consent and the Pending memory view
and asked for exact-source regression runs. The author and integration runs
are recorded in the [validation report](../validation/peer-candidate-preflight-registration.md).

The design requires the future daemon to use the sealed registration entry
point when creating `peer/`. Separate tests for root substitution, a
different-profile lease and restrictive umask remain open. The test-only
VFS name used while constructing the descriptor is replaced before SQLite
registers it. Candidate archive scans must account for that source literal.
