# Exact PostJoin publication proof design review

Claude Code `claude-sonnet-5-5` reviewed the design in Auto permission mode
against upstream MDK `9a29d1e46f11bc8e421046d5a1e20b52b59795df` and the
reviewed Marmish custody interfaces. The independent session was
`298a974d-4eff-4a60-ad8b-a12b1df21e61`.

The first pass requested a typed retryable pre-send refusal, separate
post-maintenance custody verification, a single-lineage scope for exact
recovery and a narrower controlled founder-history oracle. The second pass
approved SHA-256
`39947c2cf8aa827936a91f2ae9c29eb8ce6fc6bba6c6f91c8826c990821541ed`.
The published plan also incorporates the review's three nonblocking edits
covering the stable MLS epoch, post-attempt checks and the extra sweep bound.
The review inspected source and executed no tests. Installed activation
requires a separate private WSS proof and independent source review.
