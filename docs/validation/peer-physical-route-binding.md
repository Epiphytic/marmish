# Checked Peer physical route validation

The [reviewed](../reviews/peer-physical-route-binding.md) source is
`d8c68eaa5894c175945dcbe92e7e6fbf7f004bf3` on Linux AMD64 with Rust 1.97.1
and upstream MDK `bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`. The profile
used disposable file-provider credentials and real SQLCipher
initialization. A bounded qualification probe ran against a loopback-only
WSS relay, verified by the native TLS path with the fixture's private CA.

The cold fixture closed the qualifier and reacquired the Ready profile.
It verified that a bind still works after the credential directory is
moved away. A recheck succeeded after identical bytes were written to the
same record inode. The profile files had the same bytes at the end, and
the relay still showed four connections, one accepted control write and
one accepted Group write, all from qualification.

Recheck rejected changed bytes and same-byte replacement inodes,
including a replaced directory and owner lock. It also rejected a
wrong-mode file, a symlink, an already-cancelled route and a route whose
qualification expired during revalidation. Restoring the original path
or clock could not repair a failed route. Cold bind rejected missing or
forged evidence, schema 1/2 records, a wrong SQLCipher root with retained
account and device, and foreign account or device material. A separate
deterministic test covered positive and negative clock divergence, raw
and monotonic regression, stopped wall time, pre-epoch time and overflow.

The first fixture run failed before the probe because preparing keys alone
leaves an initializing profile. The fixture now opens native storage and
uses its binding witness to mark the profile Ready before testing.

| Check | Result |
| --- | --- |
| Focused relay trust, qualification and route | 17 passed in 12.46 s plus the opt-in test below |
| Isolated cold WSS and file faults | 1 passed in 10.37 s |
| Exact opt-in native controlling-PTY issuer | 1 passed in 0.60 s |
| Selected-provider qualification key read | 1 passed in 0.20 s |
| Command config and Terminal backend | 26 and 5 passed respectively |
| Ordinary `marmish` and `marmish-worker` build | Passed |
| Strict all-target Clippy with `installed-checks` | Passed |
| Rustfmt on the six changed Rust paths and Git whitespace check | Passed |

Exact Cargo selectors are `peer_relay_trust::`,
`peer_relay_trust::physical_route::tests::physical_route_cold_real_wss_binding_and_faults`,
`peer_relay_trust::tests::peer_qualification_native_cli_requires_exact_controlling_tty_confirmation`,
`identity::tests::qualification_reads_only_sqlcipher_from_selected_keychain`,
`command_config::tests::` and `terminal_backend::tests::`. All tests used
the native lib target with `--locked --offline --features installed-checks`.
The controlling-PTY selector uses `--exact --ignored` and needs
`MARMISH_TEST_PEER_CLI` set to the matching ordinary native CLI.

Ripwire reported dead-code findings for the deliberately unmounted
scope, name-based duplication around small request and clock helpers,
churn in the existing readback functions and long fault-test routines.
The code review checked the shared file opens and verified that the
credential and TLS contracts remained intact.

This scope stays unmounted. It adds no live Peer connector, daemon
behavior or installed enrollment. macOS, Linux ARM64, packaged artifacts
and deployments were outside this run. Independent files are observed
in sequence. The check can only reject drift it observes, and the
[design](../phases/peer-physical-route-binding.md) retains the same-UID and
cross-restart rollback limits.
