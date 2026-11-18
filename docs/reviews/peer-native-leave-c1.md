# Peer native leave C1 code review

Claude Code Sonnet 5.5 in Auto mode approved the exact C1 implementation
at `98a55dfda41863893a2451c41c1a5ae6aaf940d9` in session
`a686a7ff-41ff-47a0-ace1-c7d251241ce2`. The reviewed base was
`a9780fd8c875524a0f489222aef29705a3a36cad`.
The reviewer independently ran the frozen Linux AMD64 binary with
`epoch_five --test-threads=1` and confirmed all nine cases passed.
The author ran the broader custody check and strict Clippy.

The first review, session `7710a229-1a0a-4a43-b852-2cf9f8c66926`,
required direct coverage of the independent SQLite recovery trigger.
The revision opens the UPDATE aperture in a test transaction, proves
each rejection using the trigger's exact diagnostic, and permits
all three qualifying reservations and the qualifying park.
It also checks floor rejection at the checkpoint, exact SQLite integer
bounds, wrong account or origin on epoch-5 reopen and the existing
founder enrollment and acceptance proof on epoch 5.

The reviewer found no blocking issues in the revision. Optional test
strengthening includes both directions of a mismatched timestamp, an
attempt skip from one to three and mutation testing of clauses whose
alternatives are unreachable through the schema.

Claude Code Sonnet 5.5 in Auto mode approved the composed source
`5d8a8a5` in session `3f83bfa8-d633-4163-bc85-ccaaf3b7437d`.
It confirmed the nine C1 Rust files match the reviewed implementation and
checked the recovery trigger transitions and test-only activation under
current MDK. Its independent regression run passed 17 tests with three
opt-in cases skipped. The publication rebase onto the TLS validation
documentation left the Rust files and Cargo.lock byte-identical.
