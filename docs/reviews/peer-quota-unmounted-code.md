# Unmounted Peer receiver quota code review

Claude Code Opus 5.5 in Auto mode approved exact source
`0cad2b7f42cd73178ae678c9aa41b46f6fcc8dd9` in session
`76527d75-818b-4d6b-8521-30e5852a1c07`, with no blocking findings. The review
compared the six changed files against `77ddc9f4919cc8c5aa57724babde9c691e363605`
and the [approved design](peer-quota-unmounted-design.md). The reviewer inspected
the sealed lease, reservation and turn, generation and inode checks, accounting,
panic failure, forwarding and the test-only registrar boundary. The reviewer
independently ran the author's frozen executable, with eight quota and six
configuration tests passing. The executable provenance and author checks are
recorded in [validation](../validation/peer-quota-unmounted.md).

The reviewer suggested including the forwarding descriptor's test-specific name
in the artifact audit. Both normal binaries passed that expanded check. The
descriptor stays compiler-checked alongside the shared callbacks. The dead-code
allowance stays on the isolated unmounted module, which has no normal mount
caller. The established parser tests cover onion rejection and AutoFounders.
The new lease test covers representative Manual and AutoAny candidates.

The next owner review must supply an internal reservation mint tied to durable
charges. The fixture still supplies a scalar budget. A successful admission
records generation and witness before entry. The installed phase must account
for the possibility that the subsequent entry check refuses. Neither observation
qualifies the physical cap in a mounted production process.

Claude Code Sonnet 5.5 Auto session `61bf2e42-f200-473d-a7f3-04925e86dcd7`
approved the composed source `f7b1bc6` after a static review. The six Rust
files match the author source byte for byte. The reviewer confirmed the
existing installed Offer and qualification issuer have no caller that can
register the quota. The review required exact tests and a binary marker
check after the MDK lock refresh. The validation report records both.
The upstream compare covers app recovery code outside Marmish's dependency
graph and reports no storage migration between the two locked revisions.
