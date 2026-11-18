# Native Peer SelfRemove proof review

Claude Code `claude-opus-5-5` in `--permission-mode auto` approved exact
`68c95cfcb336ac95a79a88ca417f7a44e34b7b6b`, base
`0644345b9112c45cf0e235af4d37548d3d05e023`, in session
`6f847c06-933a-4d60-a0bf-27723b428cf8`. Scope was the opt-in native proof
and its test-only module mount. There were no blocking findings.

The reviewer read the full source diff, adjacent native fixtures, pinned
MDK API contracts, and exact author test and Clippy logs. The reviewer
used local read-only inspection while the author ran the builds. The
reviewer confirmed the byte-exact retained proposal, deny-only fanout
seeding, normalized one-use sender admission, native founder auto-commit,
independent recipient removal following exact commit processing, and cold
SQLCipher readback. No installed path or external connection was exercised.

An earlier independent Auto approval on `a255cfd`, session
`168357a4-ec7d-43f8-a5b5-476665c40b60`, suggested tighter route refusal and
acknowledgment checks. The final code binds the refused event ID to the
recovered proposal, verifies the reason and the absence of queued,
reported and fanout work, and requires one matching positive report with
no failures after resume. Final review also checked mode-0700 directory
creation before identity or data creation. The remaining optional checks
relate to auxiliary fields the traced pre-send path cannot populate and
founder sink content, where post-commit roster and epoch checks qualify
the emitted native commit. The review covers an in-process seal. A
persisted seal, epoch-5 terminal integration and installed capability
negotiation are outside this proof.
