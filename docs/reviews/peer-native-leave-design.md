# Native leave design review

Claude Code Auto with `claude-sonnet-5-5` approved the [design](../phases/peer-native-leave.md)
in session `8baf23e8-b0dc-4521-abfc-ebfaf5fb88db`. The reviewed SHA-256 is
`a5d745bdd3e4f8a2b4bb1f581b97dde120635451850924c5a946bb5fff8e80f6`.
Approval requires the private WSS activation schema to land first and the
terminal guards and recovery selection to be checked again against it.

The review resolved deadline ties, bounded recovery of a non-hot DELETE
journal, and progress for unrelated invitations while a terminal row is
parked. Independent plain SQLite tests observed rollback journal activity
and preserved the inode and schema version during the same-value header
write. The protected SQLCipher stage still needs its own recovery proof.

An interrupted third recovery attempt must park on the next owner turn
without reserving a fourth. Acceptance includes the 1 s and 2 s retry floors
and failed explicit recovery on either side of the deadline, with the parked
slot still charged and its attempt count unchanged.
