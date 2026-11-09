# Legacy HTTP worker cleanup review

Bead `marmish-d93.4`; implementation base
`28db4a1b96ba3e7e6b7137bd582a1a8ff34a758e`.
See [the phase design](../phases/legacy-http-watchdog.md).

Claude Opus 5.5, session `abbf7db9-0da4-4837-ab7d-29980b625a30`,
reviewed the design and completed code in auto mode. Both verdicts were
**APPROVE**, without blockers. Design SHA-256:
`922090764ae4c2e65dabf7502709d8162e3fdeb8f7286be7e045d5b8b40fe20f`;
design review response SHA-256:
`85040b5c92a91c2972015dd8d485b94a830fc676226824ef6c7b3e3ff03e6957`.
Completed-code review response SHA-256:
`d2c7dc86c6867614545986e990be6b3854897b7a8cec5aac3739e74e0c59c4bd`.

The reviewed implementation hashes are:

| File | SHA-256 |
| --- | --- |
| `tools/marmish_installed_rendezvous.py` | `aebb8f2800d627a9d9235618b86c2521990ffdf494dfa8852fc841830ec9d8de` |
| `tools/test_marmish_installed_rendezvous.py` | `95806f886c18de5d33bc00b692a9d7d1f81406d4b845fb491a825b087b84b5dc` |

The new own-loopback regression first produced exactly one expected
`PermissionError` against unchanged cleanup, then passed after correction.
It simulates Darwin EPERM only after the test's own real worker has exited.
The focused rendezvous module and existing `DarwinZombieCleanup` class passed
29 tests with zero errors, failures or skips under managed CPython 3.13.13.
The reviewer independently repeated that exact inventory: 29 passed in
3.058 seconds, with zero skips. The reviewer inspected the RED mechanism but
did not rerun RED. No full discovery, public transport, Cargo, namespaces or
native runtime tests were run for this increment. `git diff --check` passed.

The quality check reported test-fixture repetition and class verbosity;
additional signals concerned dynamic test discovery and deliberate stable
cleanup error mapping. The reviewer accepted these findings without
suppression: extracting a test framework would add unnecessary scope, and
cleanup uncertainty still fails. Serial mock execution is required because
the tests patch shared OS and clock functions.

The existing three Watchdog tests retain their assertions and bounds.
The installed-state proof helper is unchanged. Child-exit proof does not
prove process-group absence: post-reap checks accept only ESRCH, and an
unobservable or surviving group fails. The underlying macOS reason for
EPERM remains unknown. Linux simulation and local review do not establish
native Darwin success; fresh hosted macOS validation remains pending.
