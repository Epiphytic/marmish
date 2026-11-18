# Peer M2 public runtime preflight review

Claude Code independently reviewed the standalone preflight design and the
integrated test source in Auto permission mode using Sonnet 5.5. Both reviews
used base `3a17333` and locked upstream MDK
`9a29d1e46f11bc8e421046d5a1e20b52b59795df`.

| Review | Claude session | Exact scope | Verdict |
| --- | --- | --- | --- |
| Design | `e7007a23-787d-42b6-b437-73a933ed9476` | Standalone preflight in the pending join custody design | Approved |
| Code | `717e3379-ae1b-4e9e-b0b2-98c11e88ba92` | `69a1edca0499b6f14ccffd53fe320d7d30bca756`, tree `bfc115eb94b3a2159eb3280f60450664e22ff967` | Approved |

The design review asked to reset adapter and publisher counters after
preparation, require DELETE journaling for the recipient's setup and all
subsequent opens, prove zero migrations directly, check image identity and
sidecars, and label the fixture carrier. The source uses the public storage
migration summary on the three measured opens and adds no production hooks.

The code reviewer inspected both test files and reran the exact new test from
the frozen Cargo build. It passed in 25.55 seconds. The reviewer verified the
single native ingest, exact effect and receipt, transport and publisher guards,
copy bounds and cleanup, and stable source across two independent readbacks.
The reviewer found no blocking issue. The writable copy can change on readback.
The test checks the authoritative source digest before and after each readback.
The validation documents the counter scope and the observed copy byte change.

The local [validation results](../validation/peer-m2-native-preflight.md)
cover the complete opt-in target and strict scoped Clippy. The reviewed design
response SHA-256 is
`c3a05fcefff9a39ed8fbda45722069c0867c30019b9f7cb2295df50a2507709c`.
The reviewed code response SHA-256 is
`221526812168fee853b3ae1111a5d84d27e2f1425e34f58e8550b65d1f4ff542`.

## Composed review

Claude Code Sonnet 5.5 Auto session
`88533f5e-d5e7-480b-ab1f-f762ead6d70f` approved the composed staged tree
`e94a7eca62791d02bf94893e2b7f6b4accc051e3` on `4bb566e`. The reviewer verified
exact staged content and the separation between the opt-in public runtime
test executable and the unit-test-only quota dispatcher. The reviewed
composition used no production route or new runtime dependencies.
The reviewer inspected source. The owner ran the composed validation.
