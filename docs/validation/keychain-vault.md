# K1 native key vault validation

This increment supplies the vault library and bounded `marmish-keychain` helper.
It does not yet change frontend storage defaults, import Nostr identities into
confined MDK, migrate profiles, or pass SQLCipher keys to detached workers. Those
are K2 work; the complete user-facing feature is not delivered by K1 alone.

Local Linux actual-provider check:

```
cargo test --locked -p marmish-vault --test native_vault -- --ignored --test-threads=1
```

The final expanded check passed one parent acceptance test and its isolated
child in 2.52 seconds (independent repeat: 2.44–2.47 seconds). It checks distinct named Nostr/SQLCipher entries, exact
readback, helper round trips, non-overwrite, missing/duplicate entries, metadata
limits at 128/129 records, and locked reads/creation. It
uses a private D-Bus session and owned temporary GNOME Keyring state, never the
ambient user keychain. The checked-in test is the executable assertion source.
Rust workspace tests passed 139 primary tests plus four self-reexecution child
checks; helper coverage includes strict parsing and an actual 15-second stalled
stdin watchdog. Workspace all-target clippy and formatting passed. Python
regression checks passed 172 tests with four Darwin-only skips. macOS ARM64
all-target crosscheck/clippy passed; this is compilation evidence only.
Claude independently approved the Linux implementation with follow-up conditions;
see [the initial code review](../reviews/keychain-k1-code-initial.json).
All source follow-ups are resolved; [the follow-up review](../reviews/keychain-k1-code-followup.json)
approves Linux and statically approves macOS. Native macOS execution remains
a separate gate. The remaining validation wording correction is incorporated here.

Native macOS execution remains unvalidated locally. The native workflow runs
its disposable-keychain acceptance step before the transport prerequisite spike,
so provider failures can be distinguished from unrelated transport failures.
A successful Linux build or cfg-gated macOS source is not native macOS evidence.

Provider limitations: Linux references are not globally unique transactions;
Marmish rejects ambiguity and never overwrites existing items. A provider relock
race may cause the upstream wrapper to run a prompt; helper lifetime is bounded.
macOS legacy Keychain ACLs can reject a replaced/rebuilt helper with a different
code identity. Labels are display metadata; exact immutable references select
secrets. Temporary/provider allocations are not a complete erasure guarantee.

Static checks: `ripwire --pr-context` completed. `--quality-delta` exited 2:
normalized short enum-to-string and slice-to-fixed-array helpers match unrelated
stream/job helpers, including a duplicated reuse warning. They operate on
separate domain types and errors; coupling the vault to those modules would be
inappropriate. New constructor wrappers, test lifecycle verbosity and
trait/serde/test-discovery dead-code reports are recorded without suppressions
or a new baseline. No existing production symbol was made more complex.

The post-create ambiguity/readback rollback path has no native fault-injection
coverage; exact-item rollback is implemented and reviewed. The macOS two-query
lookup retains the documented same-UID concurrent-writer limitation. Cargo.lock
adds 94 packages including the vault, removes none, and preserves every existing
package version; existing dependency strings only gain version disambiguation.
