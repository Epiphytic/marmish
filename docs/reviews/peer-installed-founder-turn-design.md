# First installed founder turn design review

Claude Code `claude-opus-5-5` independently inspected [the founder turn
design](../phases/peer-installed-founder-turn.md) in `--permission-mode auto`.
The source base was `9df12c8` with MDK locked to
`bdf9833e340909f29c628f4fc2f9a3d6873f1cf5`. The reviewer inspected local
development source, locked MDK and the exact sibling design. No code changes,
Cargo runs, builds or relay tests are part of this design evidence.

| Review | Exact design commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `9958eed2b5963842e82a6717d60b1818bf77799f` | `f7fa07e2-4132-41f4-b4cb-1738991dab74` | Request changes |
| R2 | `153eca91de149d4bddab2ce8814d9a6732b98667` | `6f6bc31d-4d71-492c-bd4f-4b0711448fd4` | Approve |
| R3 | `44e5374c0e0ab3cf09065fae6da6722f807cac23` | `e25dc101-2f4b-42bf-99b5-715ced9e1875` | Request changes |
| R4 | `1613a212ddc5514499decae2b374384232f14a0c` | `89e30c77-674f-432b-a4a4-0a977226137e` | Approve |

The approved phase document SHA-256 is
`4def28baf05827ad3ea4009e08cfaba0a4c00e4fa284608a6d9431c9a10f4021`.
The R4 raw result SHA-256 is
`43e89078a2f1f5b59ca92ab7c050ba7d6c0974b95c3a175a148771df3b2fc0cc`.

R1 identified an owed native Welcome that MDK could publish independently of
the wrapped v2 Delivery. R2 requires exact native withdrawal and rechecks
the tracked Welcome ID and empty outstanding set on cold readback. It adds
independent profile-bound HKDF domains, refuses stock opens of the managed
founder filename family before ledger access, defines the one-slot quota and
real grant checks, and names the already reviewed route and capped-dispatcher
work as implementation dependencies. R3 pinned both dependencies, defined
all four checked send dispositions, clarified the governing v2 frame cap and
fixed the scope of the registrar and normal binary checks.

R4 closes R3's remaining egress gap. Create and completed cold inspection
require independent refusal and attempt counting at both the native transport
adapter and the KeyPackage publisher. Both counters must stay at zero.
The implementation review must check every method exposed through the
publisher relay facade, including subscribe and unsubscribe, and verify that
no other adapter can open the completed image in this proof. R4 raised those
points as nonblocking wording suggestions already covered by the zero-call
and gated inspection requirements.

The proof stays test-only and Linux AMD64 loopback-scoped. Start only after
the authenticated socket and capped Welcome source are composed. Cold restart
is historical readback. Live retry, v2 recipient receipt authority and
operator-facing activation require separate reviewed evidence.
