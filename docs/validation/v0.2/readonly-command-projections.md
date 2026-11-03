# Read-only command projections

The local same-UID command IPC now distinguishes the canonical Submit request
SHA256 in a client handle from the cached MLS-group-bound invocation digest.
The client invocation listing returns the latter as `group_submit_sha256`.
Server cleanup status reports whether an engine actually exists, and evidence
includes an explicit nullable refusal code. These are read-only projections of
existing state; they add no remote command or cleanup authority.

This permits installed validation to bind client and server observations without
mistaking two different digests or inventing an engine for a no-grant client.
The encrypted reopen/payload-GC regression confirms that both digest identities
remain distinct and durable.

The four product files exactly match the hashes approved by actual Claude Code
auto [corrective review r2](../../reviews/v0.2-installed-contract-code-r2.md),
session `663e5e84-bfeb-421b-b42c-afe2e19c5a76`. The broader installed driver fixes
in that review remain a separate integration increment.

Observed Linux AMD64 checks on the surrounding development tree:

- Native feature-enabled unit tests: 373 passed.
- Engine unit tests: 100 passed; engine and vault integration checks also passed.
- Default native strict Clippy across all targets: passed.

These checks cover source integration, not a final immutable release candidate.
The all-platform installed command, default-auto keychain, performance and both
HTTPS installer gates remain required. This increment does not switch the public
installer away from legacy v0.1.
