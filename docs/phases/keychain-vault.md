# K1: native named key vault

Design for Beads `marmish-d84.1`. This phase supplies a host-side Rust vault and
small helper; frontend/engine/MDK integration is K2 and is not yet implemented.
The overall extension is in [the build plan](../build-plan.md).

## Contract and boundaries

Add workspace crate `marmish-vault`, with binary `marmish-keychain`. Use Linux
Secret Service (`secret-service = 5.2.0`, default-features=false, feature
`rt-async-io-crypto-rust`, blocking encrypted DH session) and macOS
legacy Keychain (`security-framework = 3.7.0`). Pin dependencies in Cargo.lock.
Use cfg-gated target dependencies. No platform shell commands, secrets in argv/environment, automatic unlock,
plaintext mock fallback or general keychain access in the confined daemon.

References consist of version 1, purpose (`nostr` or `sqlcipher`), and a random
256-bit lowercase hexadecimal identifier. Names are non-unique bounded UTF-8 display labels
(1..128 bytes, no Unicode control characters, bidi overrides/isolates, or Unicode line/paragraph separators; no normalization); they are not lookup authority. Multiple named keys
can coexist. K2 owns name-to-reference configuration and profile binding; a name
alone must never accidentally share SQLCipher masters between state roots.

The library exposes probe, create, load and list-public-metadata operations.
Create accepts an exact reference, label and exactly 32 secret bytes. It never
updates existing items, even when their data matches. Search uses the full
application/version/purpose/reference tuple; zero matches is missing, more than
one is ambiguous. Read validates length and zeroizes its owned secret buffer.
Metadata listing must not fetch secret values. K2 catalogs enforce name uniqueness, not the vault. Error output is fixed categories:
unavailable, uninitialized, locked_or_denied, missing, exists, ambiguous,
malformed, backend_failure. Do not include native error strings or secret values.
A present locked/denied/uninitialized backend is distinct from absent service.

Linux uses service-wide exact attribute searches including locked items, never
unlock calls or collection creation. Create targets only the persistent default
alias; missing alias is uninitialized, never use session/first-collection fallback.
Check that collection is unlocked before create. The wrapper can execute a
provider prompt if the collection relocks between that check and creation. Accept
this documented race rather than reimplement encrypted Secret Service sessions:
prompt errors map to locked_or_denied and the helper watchdog bounds the call.
No explicit unlock is requested by Marmish; tests verify the already-locked case
returns without creating an item. Cleanup Item::delete can likewise invoke a
provider prompt, bounded by the watchdog, and cleanup failure remains failure. Never negotiate plain
sessions. Existing matches in any collection count toward exists/ambiguous.
Use create_item(replace=false). This prevents replacement but does
not enforce uniqueness: search before and after create, reject duplicates. K2
serializes Marmish creation under profile/catalog locks; unpredictable references
and post-create validation cannot defend against malicious same-UID writers.
Read back and compare each newly created item before reporting success. On
post-create ambiguity or readback mismatch, attempt deletion only of the returned
new item's exact path, never other matches; return failure even if cleanup fails.
Crash can leave an orphan, which is not silently selected by label. Native search
may allocate its complete result before the application count limit is applied.

Only provably absent session bus/address/socket or absent service after failed
activation is unavailable. AccessDenied and lock/prompt failures map to
locked_or_denied; timeout, disconnection, failed DH and all uncertain failures
map to backend_failure. Missing default alias is uninitialized. K2 preserves
HOME, XDG_RUNTIME_DIR and DBUS_SESSION_BUS_ADDRESS for the host helper only;
none enter the daemon or command environment. No caller deadline causes fallback.

macOS uses add-only SecItemAdd, exact service/account searches scoped to the
selected keychain and bounded results. Retain `SecKeychain::disable_user_interaction()` guard for each Vault lifetime.
Probe default keychain existence and lock status without displaying prompts;
use a minimal isolated `SecKeychainGetStatus` binding with unlock-state bit.
Map OSStatus -25307 to uninitialized; -25308/-25293/-128 to locked_or_denied;
-25300 to missing; -25299 to exists; other statuses to backend_failure.
Service is `marmish.v1.<purpose>`, account is immutable ID, label is display name.
List returns attributes only (no data), match limit129. Legacy ACLs bind the
creating executable: stable installed helper/code-signing identity is required
for reliable access across rebuilds. Ad-hoc signing does not promise continuity;
native tests must exercise access across helper restart/replacement and document
any locked_or_denied outcome. Do not weaken ACLs to all applications. Production
uses the default keychain; tests can explicitly pass a disposable keychain handle
through the library, without altering login/search-list settings. Duplicate
native status maps to exists. No update fallback.

The helper reads one bounded JSON request (maximum 8192 bytes, strict unknown
field rejection) from stdin and emits one JSON response (maximum 131072 bytes) then exits.
Actions are probe/create/load/list; secret values use canonical base64 only in
private stdio, never command arguments, logs or diagnostic errors. Library native
calls can block, so callers must enforce a helper-process deadline, kill and reap
on timeout, and limit response size. Document that direct library calls do not
supply a deadline. Install a fixed panic diagnostic handler and set RLIMIT_CORE=0 before parsing
secrets. A helper watchdog exits after 15 seconds, including stalled stdin;
callers impose an outer deadline too. Reject command arguments, duplicate/unknown
fields, trailing JSON and noncanonical base64; avoid generic JSON values for
secret-bearing requests. Zeroize owned encoded request/response buffers where possible;
this does not promise erasure of provider/internal serialization allocations.
Public list output is capped at 128 records; excessive results fail closed.

K1 does not change existing profile formats, daemon builds or worker launch.
K2 must review startup provisioning, sticky backend selection, explicit file
migration, anonymous-pipe handoff, MDK source attestation and detached workers
before any of those integrations are implemented. K2 must journal the random
reference before create and commit its verified binding before database creation;
recovery accepts only its exact journaled pending reference in pristine storage,
never unrelated orphan items. Workers already holding keys survive vault lock;
frontend reopen/reconciliation while locked must fail without rerunning commands.

## Ownership and validation

One agent owns crate library/provider modules; a second owns helper binary and
integration tests against the agreed public API. Root owns workspace membership,
lockfile integration, documentation, review evidence and commits. Agree exported
API before parallel edits; avoid concurrent lockfile mutations.

Unit tests cover strict reference/label/length parsing, bounded helper framing,
unknown fields, sanitized errors, multiple names, missing/duplicate items,
non-overwrite behavior and locked-versus-absent classification. Fake adapters
supplement rather than replace actual native tests.

Linux acceptance creates an owned temporary HOME/XDG tree and isolated
`dbus-run-session`, starts an owned gnome-keyring-daemon and unlocks only that
throwaway collection via private stdin. Exercise two names and both purposes,
round-trip byte equality, list metadata, duplicate refusal and missing reads.
Cleanup only owned service processes/state; never inspect ambient user secrets.
A missing bus/service probe must return unavailable. Actual native macOS tests
create a disposable keychain without changing global defaults, test add/load,
non-overwrite, names and locked access, then delete only owned state. If no native
runner is available, preserve this as an explicit unvalidated gate, not a pass.

Run workspace tests, formatting and clippy, inspect quality delta and staged
secret/ignore hygiene. Obtain independent Claude design approval before coding
and Claude Code auto-mode integrated review after checks. Record precise native
results and limitations, then commit/push the completed reviewed increment.

## Design review disposition

Initial and follow-up Claude reviews are under docs/reviews/keychain-k1-plan-*.json.
The final narrow finding is resolved by explicitly accepting the wrapper relock
prompt race with preflight lock check and process watchdog (option a). No custom
DH implementation or implicit unlock is added. Native macOS validation remains
an honest external gate.
