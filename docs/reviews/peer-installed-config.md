# Installed Peer configuration review

Claude Code Sonnet 5.5 in Auto requested a closed Serde representation in
R1 (`ddbf1164-f690-4068-9d89-c3742b55ff67`). The revised design uses
internally tagged empty struct variants so the decoder can reject unknown
and duplicate members. Claude Auto approved R2 in session
`1a71bfc5-b420-4b35-aa40-3eecf9126483` at
`38ecce97147e9c08fd08842b03c28e79c4f5b430`.
The [approved design](../phases/peer-installed-config.md) has SHA-256
`d8fd36f63797190b4da09ed2a88785d4d8768ac29ce92acb2146f8044d02002b`.

The parser keeps Peer intent private on the normalized config. The existing
setup proposal preserves it through ordinary updates and returns the specific
role-conflict category before writing. An initial test run exposed Serde's
sequence form for derived types. An explicit object visitor preserves the
original map entries for duplicate detection and rejects array forms at both
Peer object boundaries.

## Implementation review

Claude Code Sonnet 5.5 in Auto approved R1 in session
`83f3c04a-88e1-45ec-aab2-8fc518a63512`. R2 documents the inherited transport
checks, covers a newline embedded in the host and removes unrelated setup
formatting. Claude Auto approved the exact R2 source in session
`22d41481-a868-4cf0-9ebb-f23447deaae9` at
`260bfdb4bf263f46143de85d3eef777b2fb2fd2b`. The implementation diff
from the approved design has SHA-256
`9e4d34818c2c5dbad565ba319017f46f9a5b45b9e4560eff825e6f7d65f85b15`.
The reviewer independently verified both focused suites, strict native
all-target Clippy, parser formatting and the diff. The normalized `_peer`
field has no reader and can take its final name when a consumer is reviewed.

## Validation

Linux x86_64, Rust 1.97.1. All commands used the lockfile and cached
upstream MDK at `59b7ed3f`.

| Check | Result |
| --- | --- |
| `cargo test --locked --offline -p marmish-native --lib -j2 command_config::tests` | 26 passed, including six new Peer parser tests |
| `cargo test --locked --offline -p marmish-native --lib -j2 native_setup::tests` | 12 passed, including setup preservation. One existing opt-in controlling-terminal test ignored |
| `command_config::tests::peer_config_` and exact `native_setup::tests::ordinary_setup_preserves_peer_intent_and_rejects_direction_conflicts` on the built test binary | 6 passed and 1 passed |
| `cargo clippy --locked --offline -p marmish-native --all-targets -j2 -- -D warnings` | Passed |
| Parser `rustfmt --check`, setup format comparison against baseline, and `git diff --check` | Passed. Setup retains only its preexisting formatting delta. |

These checks cover the local parser and setup increment. Installed Peer
activation, an external relay, and macOS remain outside this validation.
