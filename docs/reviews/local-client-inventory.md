# Local client inventory reviews

Claude Code in auto permission mode approved the design and final integrated
implementation in session `e7ea815f-b406-431d-8f52-f8f18d02c86d`. It reviewed
effective policy reads, same-owner IPC, authenticated receipt and engine
admission tracking, the encrypted migration, and the local and live checks.

The first integrated review required correcting the schema-4 fixture and
documenting the rollback boundary. The fixture now removes the inventory table
before presenting the old format. The user documentation requires a backup of
the stopped profile before using a newer owner. The final review confirmed
both fixes and ran focused Rust checks and the Node release-tool suite.

The migration also required the release checker to recognize format 8. Claude
Code separately reviewed and approved the source marker design in session
`296765e7-27e0-47c9-a40b-223fcc18fcd5`. The marker comes from the authenticated
candidate's Git source. Existing format tuples still apply without the marker,
and published 0.3.0 retains journal 7. The integrated review covered the
source binding and manifest tests.

The removal and re-addition checks run against the inventory store rather than
three separate daemon launches. The live test covers a normal owner restart and
a policy change. The separate-UID socket case, macOS and Linux ARM64 remain
unverified. See [validation and benchmarks](../validation/local-client-inventory.md).
