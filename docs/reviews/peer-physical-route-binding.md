# Checked Peer physical route review

Claude Code used Claude Sonnet 5.5 in Auto mode for each review. The
[design](../phases/peer-physical-route-binding.md) uses approved blob
`13b7d44ed1f2d6587f9aa80e393e30459d089ae2`.

| Session | Exact scope | Disposition |
| --- | --- | --- |
| `fd212314-9dc0-4d91-abf7-8a769bba3689` | Approved design revision | Approved the unmounted physical binding and independent eligibility boundary |
| `050fe334-8057-49ef-8fcf-06456054f3cf` | Implementation `96acaa0` from `9a7177b`, all six Rust paths | Approved with no blockers |
| `fcc94a29-9bc3-4947-8c6d-3d4336762027` | Exact source `d8c68eaa5894c175945dcbe92e7e6fbf7f004bf3`, including the final cold fixture | Approved with no blockers |

The first code review checked the initial descriptor-to-byte binding,
key derivation compatibility, exact lease and file checks, record and
certificate windows, and sticky cancellation. It found no path from the
new scope into the connector, invitation owner or daemon. The final
review also checked that the fixture marks its profile Ready only after
opening real SQLCipher databases, that qualification uses the bounded
loopback listener, and that cleanup stays within the exact owned profile.

The fixture now checks missing record, managed attestation and CA files
and demonstrates that identical bytes written into the same inode still
pass. It avoids copying the deliberately altered disposable SQLCipher
root. Remaining review refinements include precise errors for more
negative cases, separate CA/attestation time-edge cases and panic-path
runtime cleanup for the fixture. The reviews relied on source inspection
while the author ran the [native checks](../validation/peer-physical-route-binding.md).
