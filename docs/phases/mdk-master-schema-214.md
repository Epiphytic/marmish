# Advance upstream MDK master to SQLite migration 214

The native workspace follows the upstream MDK master branch. Advance the
reproducible lock from `122ad45ece73c1d6f687a4e92a541b9b2febde34` to
`b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1`. The upstream change adds the
link-chat-progress tables and their account storage ownership in migration 214.
Keep the Cargo manifests on master and review any change outside the ten
MDK lock entries before accepting it.
[Upstream migration and storage change](https://github.com/marmot-protocol/mdk/commit/b3cd2cb1c3a585372c5061b8f35d31a93ce9cdc1)

Advance Marmish's read-only native start preflight from known account
schema 213 to 214. Keep checks for future schema, malformed databases and
profile-specific SQLCipher keys. Use the existing real native preflight case
that creates, closes and reopens an MDK account with WAL files. Verify the
fixture reaches 214 and that injecting the next migration makes startup
refuse while preserving the account inventory and bytes. Run the other
preflight tests, strict native all-target Clippy, the exact core selectors,
and a private WSS native Command and Terminal writer/reattach smoke from
the same build. Check the build stamp matches the latest lock and preserve
the limits of the Linux AMD64 evidence. No machine migration or application
feature activation is part of the change.

Claude Code Sonnet 5.5 in Auto mode approved the design at `3df86d5`
in session `6d006cd9-7d42-45c9-87e9-e259125eb193`. It confirmed
the exact upstream storage transition and the retained read-only refusal
checks. The real test asserts the owner reached the known boundary before
inserting the next migration and checking refusal.
