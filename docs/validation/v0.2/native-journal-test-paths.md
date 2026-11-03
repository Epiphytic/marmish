# Native journal fixture path correction

Tracking: `marmish-fuu`. Base source: `20606e567338e02c168198e0d807ae8b32cef917`.
The failed hosted candidate and all 33 observed failures are recorded in
[candidate evidence](candidate-41628708556.md). This validation qualifies the
local fixture repair, not the candidate or a macOS release.

The four failing allocation sites now share a cfg(test) helper that resolves
only the existing temporary allocation parent and creates a private 0700 leaf.
Existing canonical journal/store fixtures reuse that helper. Production code,
SQLite NOFOLLOW, file ownership, link counts and encryption are unchanged.

The deterministic owned-parent-alias regression verifies canonical allocation,
owner UID, mode 0700, encrypted open/reopen, final database-symlink refusal and
TempDir removal. The existing ancestor/immediate-parent negative independently
verifies raw ancestor alias refusal (`Storage("journal")`) and canonical keyed
reopen. Final-leaf refusal remains `Storage("journal_path")`.

Local Linux checks ran as UID 1001, with no root skip:

| Command | Result |
| --- | --- |
| `cargo test --offline --locked -p marmish-native --lib journal:: -- --nocapture` | 73 passed, 38.67 s |
| `cargo test --offline --locked -p marmish-native --lib command_service::tests:: -- --nocapture` | 15 passed, 0.62 s |
| `cargo test --offline --locked -p marmish-native --features installed-checks installed_ -- --nocapture` | 68 passed, 5.05 s |
| `cargo clippy --offline --locked -p marmish-native --all-targets --features installed-checks -- -D warnings` | passed |
| `cargo fmt --all --check` and `git diff --check` | passed |

All 33 formerly failing tests are included in the first two runs. The new
regression adds one test to the default library suite. No full native suite was
repeated locally, and no live network, provider or keychain test was performed.
The next immutable-source hosted run must qualify Darwin and all other gates.

The exhaustive native opener audit found direct Journal test openers only in
journal, command rooms, command store and command service; reopen sites retain
their shared fixture paths. MDK and command-check tests use ProfileLease, which
canonicalizes its existing base before database paths. Independent installed
fixture audit found no additional direct SQLite/Journal opens. Preflight uses
an owned HOME fixture and passed the failed candidate; a hypothetical symlinked
HOME ancestor is outside this repair and is not claimed supported by that test.

`ripwire crates/marmish-native --quality-delta` reported similarity between short
journal setup functions and a nearby temporary-directory builder, plus the new
Rust test as unreachable in its static graph. Those findings were inspected;
the shared helper reduces repeated allocation logic and all new code is
cfg(test). No broad refactor or static-analysis suppression was introduced.

Detailed logs and raw Claude responses remain outside the repository. Review
records preserve the exact independent findings and scope; no private runtime
state or secrets are included here.
