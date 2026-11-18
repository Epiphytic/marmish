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
