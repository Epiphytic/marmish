# Peer checkpoint integration review

Claude Code Auto approved the exact integrated source in session
`3244b6f6-2101-458f-ae74-524e2e846cee`. The scope was Git tree
`42179908f772123f08bc38e36307074d546acda1` on published base `1a692e2`.
The reviewer verified the tree before and after reviewing it. This scope
preserves the Terminal mounts and adds the three OpenMLS dependencies
only to development targets. The invitation module has no daemon
activation or production constructor for a qualified origin.

The earlier reviews for [invitation integration](peer-invitation-integration.md),
[pending activation](peer-pending-activation.md),
[typed SelfRemove](peer-self-remove-proof.md) and
[provisional reservations](peer-provisional-reservation.md) document their
individual design and scope fingerprints. The exact integrated replay
uses upstream mainline MDK at `0ad3c61` from the evaluated lockfile.

The reviewer passed the 31 focused storage cases, the standalone cursor
target, the native signer case, the reservation benchmark and nine staged
native proofs. The pending leave test includes the typed SelfRemove
positive and negative checks. The reviewer identified the separate
native request-to-fanout crash window already recorded in the SelfRemove
phase as a follow-on before production use.

Author checks on the same tree passed 39 focused invitation tests, 31
standalone cursor/checkpoint tests, one ignored typed native leave test,
three private invitation WSS tests and ten raw WSS tests. The WSS cases use
disposable keys and a bounded loopback relay. All-target Clippy passed with
`-D warnings -A clippy::large_enum_variant`. Running without the exception
produced the same two warnings in `terminal_selection.rs` on both the
published base and the reviewed tree.

| Local metric | Samples | p50 | p95 |
| --- | ---: | ---: | ---: |
| SQLCipher reservation commit | 20 | 77.575 ms | 106.228 ms |
| SQLCipher reservation recovery | 20 | 77.399 ms | 98.158 ms |
| Authenticated native SelfRemove recheck | 20 | 8.981 ms | 9.714 ms |
| Private WSS authenticated readiness | 12 | 2.379 ms | 2.870 ms |
| Private WSS publication receipt | 12 | 0.834 ms | 0.935 ms |
| Private WSS authenticated delivery | 12 | 1.605 ms | 1.790 ms |

These runs used the default unoptimized Cargo test profile with debug
information off. A single private invitation run observed 57.472 ms
for intake and 7.514 s for native preparation through publication
receipt. These effects have no application authority.

## Composition with Terminal

Claude Code Sonnet in Auto mode approved the same source composed with the
Terminal writer checkpoint in session `6e083eca-abf4-4a23-9473-dfa24a728632`.
The composed index was `3f21609c819749c8897c3cd763378e2f3e0c8f8e`.
The reviewer checked both shared native module mounts, the development-only
dependencies, and the absence of daemon, CLI and Terminal activation.
The native build and warnings-denied all-target Clippy passed. The reviewer
ran 44 Peer library tests, 152 Terminal library tests and 31 cursor-store
integration tests with no failures. A separate unfiltered run was stopped
after more than thirteen minutes during existing native trust tests and
provides no full-suite result. The later Terminal enrollment diagnostics and
listener pacing increments touch disjoint paths.
