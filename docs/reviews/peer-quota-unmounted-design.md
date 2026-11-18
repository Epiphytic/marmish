# Unmounted Peer receiver quota design review

Claude Code Opus 5.5 in Auto mode approved the [design](../phases/peer-quota-unmounted.md)
at `828aa3b8d4632c31b17fd385bcc2f3c41cfd2123` in session
`90073ece-b1de-4418-aa12-cf7f5c224ca4`. The reviewed design SHA-256 is
`30e18af92c8cacc5cd964b8c494f86ca8507fa9ee7f56e7ebc77870ac413659f`.
The reviewer inspected exact local source and made no edits, builds or runtime
calls. The verdict had no remaining blockers for the unmounted boundary.

Round one, session `d19e9e5d-d6cb-4691-b406-e88c8fdc9b7d`, found the original
shared preflight registrar seam, lease transfer, quota inventory, observations
and SQLite caller audit insufficient for later runtime qualification. The final
design isolates the compile-only cut and makes those runtime dependencies
explicit. It preserves the separate review and Linux plus APFS qualification
required before mounting in production.

For implementation, use a lease-bound config read or exact under-lease reread
and compare the normalized binding. Direct Peer origins already use
`peer_origin` with `RouteMode::Off`, which rejects `.onion` even when the ordinary
profile uses Auto. Cover that parsing/eligibility boundary and keep compile-only
lint allowances narrow. No runtime or platform qualification was performed
for this design review.
