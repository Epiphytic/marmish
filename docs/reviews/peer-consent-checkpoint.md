# Peer Offer and Consent checkpoint reviews

Claude Code ran each independent review in auto permission mode with
Sonnet 5.5. The reviewer could inspect the exact archived files and run
offline tests. The checkpoint uses disposable data and test-only entry
points.

## Detailed design

The first review required changes in session
`2917b367-2c94-4b69-b642-4b93e18d7ff2`. It found gaps in outer timestamp
binding, test module ownership, the persistent clock relative to cursor
rebase, typed capacity dispositions, the scope of automatic consent,
and outbox expiry. The revised [phase design](../phases/peer-consent-checkpoint.md)
defines all six boundaries.

Claude approved the revised exact design before implementation in session
`c0dc9316-4113-490f-8c78-018bb43f4a1b`. The archived base was `15defcc`.
Claude verified the design SHA-256
`309e9bae4fa8bc85fffc45ba640b83b4244ed057b8262cb17263ef1c0d3d92ad`.
The design-only review ran without tests.

## Code review round one

Claude reviewed all new source and fixture files, the cursor mount and the
approved design in session `1ae6784d-8fc8-41c0-9550-9e4b2c1753d5`. It verified
the archived hashes and passed all seven focused tests. It required explicit
SQL rollback and reopen coverage for a local approval and outbox discard.
The revised tests cover both. They also verify malformed input, recovery of
automatic approval under Manual policy, and rejection of the same signed
revision on another configured origin after discard. The Offer's verified
singleton route rules out that cross-origin replay before deduplication.

The signed native KeyPackage excludes publication endpoints. This
checkpoint checks the local stage custodian's origin and selector separately
from the cryptographically verified package. Production native custody
still needs to supply independently authenticated metadata.

## Code review round two

Claude approved the integrated checkpoint in session
`2a45d9b7-cbe8-4ddf-b693-c824002f7e7b` on archived base
`35e51c68a575059caae3b2edc7c9f13c703a9ca0`. It verified the SHA-256
manifest for the approved design, the cursor test mount, and the four
new checkpoint source files. The manifest digest was
`7f9046ed7fff0194238893c1bc52b6aee2e84bbb0ed65e93697d04ec80cc5423`.

Claude passed all seven tests with
`cargo test --locked --offline -p marmish-native --test peer_cursor_store peer_consent_checkpoint_core`
and confirmed the earlier rollback coverage finding was resolved. It made
no edits and used no network. Independent checks on the same source
included strict Clippy, the complete cursor target, and the opt-in timing
benchmark. [Validation results](../validation/peer-consent-checkpoint.md)

The reviewer noted that the total 8,192-fence bound follows from the
per-origin class limits. The focused test checks the 256-entry class
boundary directly. The offline schema initializer relies on versioned
ownership of its private tables. A future production package owner will
need to provide the origin and selector provenance for its material.
