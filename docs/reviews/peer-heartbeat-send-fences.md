# Peer heartbeat send-fence review

Claude Code 2.1.301 with `claude-sonnet-5-5` conducted each
review in `--permission-mode auto` on a frozen repository copy.

## Design

| Round | Session | Design SHA-256 | Verdict |
| --- | --- | --- | --- |
| R1 | `ddf0f4ea-d479-4015-a98b-53d6950be909` | `8daf6414a794cfe494fa70e17f527081ded30dd5dcfc1968a25b21eccfe229fb` | REVISE |
| R2 | `3ec9c296-5a69-4620-a3b1-3e722a127895` | `962982b4846abbb6b890a1a5910fb7929479cafeb072de89b745b99c99d96b99` | REVISE |
| R3 | `b12adaf0-ca3d-463c-be42-f46b70024379` | `fea06b332932f7156172482956b22117a54473efacc2ec3b6df264374ebd68bd` | APPROVED |

R1 used base `35e51c68a575059caae3b2edc7c9f13c703a9ca0`. R2 and R3
used `b6bbb684d35aba0f5f431431781e40559a6064e5`.

R1 required exact advertised permission identity, module-enforced
hardening and schema adoption, directional uniqueness and terminal
authority, explicit errors and opaque reservations, and accurate CI
scope. R2 confirmed most changes. Its local SQLite probe found that
`sqlite3_stmt_readonly` also accepts transaction control and writable
PRAGMAs. R3 removed caller SQL and callbacks. It adopted a fixed
same-transaction authority query, full main and temporary schema checks,
a real commit hook and per-statement DDL fault, and one named CI subset.

R3 approved only standalone storage. It required an exact fresh scope
restore or sync per store instance, a main-qualified fixture authority
outside the reserved namespace, named core commit and DDL fault tests,
no retained state, and exact scoped format and Clippy checks. The fixture
documents why the locked MDK dependency already enables the required
rusqlite hooks. Subsequent design text imports the public key from its
pinned `nostr::key` namespace and qualifies the core Cargo filter so it
cannot accidentally select mounted codec cases. The SHA-256 with those
mechanical clarifications is
`5a101c627699c14d665fb2a4c9353b7cf9ffdad02c5e9d0e06b7306ea047c61b`.

The [activation design review](peer-heartbeat-activation.md) covers
the separate production gates and future Reserved work identity.

## Code

Claude Code auto session `4e09528a-0185-4cf2-927c-c4e04c227bc0`
returned **APPROVED** with no blockers on base
`b6bbb684d35aba0f5f431431781e40559a6064e5`.

| Scoped file | SHA-256 |
| --- | --- |
| `crates/marmish-native/src/peer_heartbeat_send_fences.rs` | `c323f5a5f82c0db0a357cf483f19dac93f1cf2b1db4cc4380c2e5fb83372a82a` |
| `crates/marmish-native/src/peer_heartbeat_send_fences_tests.rs` | `adcad2bbcbcb8cefdc22b437cdba0849cd595c9e68f39ccdc5557b8558566408` |
| `crates/marmish-native/tests/peer_heartbeat_send_fences.rs` | `de92dd672b6e926dd868950c14b104a54f11d31417325776910cf00810dabf27` |

Claude verified the frozen design and source hashes, checked hardening,
authority, exact main and temporary schema, clock and counter fences,
directional uniqueness, retirement, commit uncertainty, rollback and
restoration. It independently ran the supplied compiled executable
and passed 24 tests with two optional benchmarks ignored, passed the
five-case CI selection, and ran the storage benchmark. Its independent
sample median was about 49 ms per durable reservation and 0.7 ms for
close, reopen and restore. Claude verified formatting and an empty
test temporary root. The author separately built the exact source and
ran `cargo clippy --locked -p marmish-native --test peer_heartbeat_send_fences -- -D warnings`
and scoped rustfmt successfully. See the [local validation results](../validation/peer-heartbeat-send-fences.md).

Claude's optional notes cover extra fault permutations, redacting scope
values before any production logging, and a production abandonment
path if the authoritative row disappears during reconciliation. The
standalone review leaves that case closed until reattach. The reviewed
integration stays test-mounted. Actual accepted status authority,
native publication, planner timestamp consumption, and receiver
checkpointing still need their separate proofs and reviews.
