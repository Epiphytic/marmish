# Authenticated Peer relay qualification issuer review

Claude Code ran with `--permission-mode auto` and Claude Sonnet 5.5.
The scope is the checked issue/readback increment of the [production
Peer route phase](../phases/peer-production-route.md). The author
recorded exact Linux results in the [validation
report](../validation/peer-qualification-issuer.md).

| Session | Reviewed scope | Disposition |
| --- | --- | --- |
| `a532dc09-073f-46c2-a959-3f7293690d2d` | Detailed design, final blob `0199c8ff` | Approved the bounded requalification issuer and restart readback contract. |
| `74bc0764-a86d-49b3-976d-839dec81cad4` | Initial issuer source `dbca503` | Requested caller-level credential failure and purpose-domain tests, historical schema 1 status and a precise key-wipe claim. |
| `632a002b-18af-4116-93d1-a0c9e5bd0fdf` | Intermediate revision `758d704` | Found an invalid test-only manifest clone at compile time. The final test swaps and restores fields explicitly. |
| `28d04f1f-22b3-47fc-b7e6-45473952a8d4` | Exact final source `bdf3c54` | Approved without blockers. |

The final correction uses a real helper invocation in the qualify path
to check denial and timeout ordering before either the terminal prompt
or a network connection. Purpose binding tests use the same root
credential and vary one profile coordinate at a time. Historical
schema 1 records display as unqualified even once expired. The
specification describes zeroizing owned key buffers and the remaining
limit for internal cryptographic state.

The final reviewer inspected the complete issuer diff and ran the
exact native test executable, including the controlling-PTY WSS case,
the credential failure case and all 23 identity tests. The author ran
the normal build, strict installed-checks Clippy and the Command and
Terminal regressions. The documentation records the metadata-only
meaning of `show` and the repair requirement after a partial pair
replacement. The implementation retains the conservative
`write_uncertain` status for record-stage errors after the attestation
has moved.

Claude Code Sonnet 5.5 Auto session `df8c60cb-18a2-4358-b5f0-bac45c9020f6`
approved the composition `4aab1f2`. This additional review was static.
It confirmed that the Rust, dependency and ignore files match the
approved author source byte for byte and that the earlier installed
Offer work was intact. Rebasing the composition onto `cd90f8b` added
only the earlier Offer review and validation documents.
