# Installed v2 recipient Pending bridge validation

The exact author source was `4689d29c79be65cdf4b5b4b3d35fa26e05397c6b`
with upstream MDK `bdf9833e`. The opt-in Linux x86_64 test uses a private loopback WSS relay with its own
TLS trust and NIP-42 authentication. Founder and recipient run in separate
processes and leases with disposable accounts. The founder runs a real v2
Offer, receives the exact signed Consent, creates the MLS group with MDK and
sends the native Welcome in a signed Delivery. The recipient first uses the
existing test fixture to provision the provisional keypackage. Each process
that works after Held registers the managed receiver quota on the exact newly
acquired lease before it opens one stock SQLCipher cursor. It passes only
public test coordinates between processes.

Every scenario begins with a new signed Offer, Consent and Delivery and a
cold Held read. The stopped and wrong-generation Start checks and the second
Start refusal run before the held native permission is used. Route
qualification and recipient role checks precede each relevant turn. Native
success and zero effects were verified using exact custody reads, the actual
native poll counter, native proofs and the relay publication-attempt counter.
The custody writes below describe verified state transitions and the scoped
writer paths. Other cursor bookkeeping and disposable native snapshots can
still write. The latency windows cover local bridge work, exclude relay setup
and network time, and report one run.

| Scenario | Verified effect after Held | Local observations |
| --- | --- | --- |
| Successful managed Welcome | One Start, one native poll and committed join, one Pending in a new process. Another new process independently verified the same native group, epoch, epoch identity and local leaf as the founder, then replayed exact Delivery and received `AlreadyApplied` with zero package reads. | 344 ms preflight and Start, 182 ms native turn, 358 ms cold readback and Pending, 376 ms post-Pending readback and replay |
| Pre-effect process exit | One Start and no native poll. Two fresh processes found no authority to qualify Pending or repeat Start. Both retained the exact charged JoinStarted. | 340 ms preflight and Start. Zero native polls |
| Managed capacity refusal | One Start and one native poll. A physically charged sibling family consumed the headroom so SQLite refused the rollback journal growth. The original ciphertext was unchanged. Two fresh processes found no authority to qualify Pending or repeat Start, retaining the exact JoinStarted. | 332 ms preflight and Start, 127 ms refused native turn, 2,416,640-byte peak, one main open, one journal write refusal |

Both accounts authenticated and each relay transcript contained the three
signed enrollment flights. There were zero additional relay publication
attempts after Held in all scenarios. No receipt or application status
publication took place. The independently measured cap refusal establishes
unchanged native stage bytes, while an arbitrary uncertain operation only
leaves fenced custody pending separate native verification. The process-exit
cuts preserve and reopen already closed files. Managed hot-journal recovery
uses its separately exercised journal-sync and main-write crash fixtures.

| Focused checks | Result |
| --- | --- |
| Exact private v2 WSS bridge | Three process-cut scenarios passed in 57.50 s excluding build |
| Managed journal crash and stock WAL | Passed in 2.52 s |
| Legacy capped native Welcome | All four subprocess cases passed in 54.17 s |
| Receiver inventory default selection | 5 passed, 3 opt-in cases ignored |
| Signed v2 wire and prepared/Held reopen | 3 passed |
| Ordinary binary smoke | Offline locked Cargo check passed in 7.80 s |
| Candidate `installed-checks` binary smoke | Offline locked Cargo check passed in 17.66 s |
| Exact all-target Clippy | Passed with `-D warnings` in 27.09 s |
| Formatting | Scoped Rust formatting and `git diff --check` passed |

## Scope

This proof advances a private test-only receiver from v2 Held to Pending.
Native witness types prevent passing legacy or uncapped evidence into the
installed checkpoint. The legacy native fixture additionally checks wrong
authenticated carrier, file replacement, digest, deadline, stop, real journal
refusal and incomplete native futures. The v2 wire and installed custody tests
verify authenticated transcript and negotiation reconstruction.

The integrated refusal matrix exercises expired Held custody, changed policy,
legacy Start, competing Delivery, a damaged retained frame, foreign lease,
wrong stage inode, stale owner, retired route, and retained epoch mismatch.
An injected lost native result recovers exactly one Pending transition from a
verified closed join. The earlier cold refusal checks measured the absence of
effects. The expanded matrix checks typed refusals for the added cases.
[Integrated refusal and uncertain completion results](peer-installed-capped-pending-refusals.md)

Expiry after Held, legacy refusal at `commit_custody_pending`, wrong native
package and joined roster or administrator integrity need additional direct
coverage. A crash during an uncommitted WSS Welcome is a separate case from
the local managed hot-journal test. The supervised daemon, installed approvals,
application status, receipt and application egress, and other architectures
need separate qualification.

## Measured accounting on the composed source

Exact source `7d725d01d59e8be5fe940fd2186bb2d83375a068` counts the SQLite
rows affected by the installed Start and Qualify statements after successful
transaction commit. The native counter measures actual Welcome future polls
and successful joins whose managed turn closed. Other cursor bookkeeping and
snapshot writes remain outside these counters. A successful cold MLS readback
independently corroborates the observed Welcome success. For capacity refusal,
unchanged encrypted original bytes corroborate no committed join.

Each measured window begins after quota mount, physical route setup and the
exact custody read. It covers the named local operation, uses the suspend-aware
continuous clock, and asserts an elapsed duration under five seconds and below
the remaining exact signed custody bound. An independent wall sample after each
window also asserts the signed bound has not expired. Relay fixture setup and
Delivery replay retrieval do not contribute to the named local latency.

After the positive path reopened Pending, the test saved its exact retained
custody evidence, independently verified the MLS coordinates, and fetched the
exact signed Delivery again from the relay. It reopened the SQLCipher owner
and reread Pending. The before and after reads matched exact saved evidence,
including the durable row, stage and custody identity. The replay returned
`AlreadyApplied`, performed zero package reads and caused zero observed
Welcome polls, closed native joins and committed Start or Qualify rows.
The exact persisted evidence comparison establishes that the retained
Pending row was unchanged. The scoped SQL counter only covers Start and
Qualify, not every writer.

The managed journal branch in native readback calls
`RecoveredOwner::rollback_journal`. Commit `e942577` split this inner turn
from the fixture wrapper and changed shared charge arithmetic after the
original rollback qualification `6a8846ec2a6f3b363ec9b7cdf8f90faf15fbe188`,
guard matrix `515557a61235f81debdb3deb6d52bdb60a6cafee` and evidence record
`ccf2beb100d01613a7b1fd2052dff79e58a98169`. On exact composed source
`7d725d01d59e8be5fe940fd2186bb2d83375a068`, both the author and the reviewer
reran `managed_recovery_hot_journal_and_stock_wal` from the same binary,
validating actual journal sync and main write, a separate interrupted
rollback and verified keyed recovery through the current wrapper and inner
turn. The private WSS process cuts reach fresh joined readback after a
completed Welcome. A crash during an uncommitted Welcome still requires fresh
joined evidence before any Pending transition.
[Earlier managed rollback validation](peer-receiver-charge-recovery.md)

| Measured exact private WSS scenario | Local observations |
| --- | --- |
| Successful managed Welcome | Start 367 ms with one committed row. Welcome 356 ms with one poll and one closed successful native join. Cold readback and Pending 470 ms with one committed row. Fresh Pending readback 372 ms and exact Delivery replay left all effect counts at zero. |
| Pre-effect process exit | Start 390 ms with one committed row. The cut and subsequent cold refusal performed zero Welcome polls, closed native joins or committed custody rows. Cold refusal 250 ms. |
| Managed capacity refusal | Start 381 ms with one committed row. Native refusal 118 ms with one poll, one rejected journal write and zero closed native joins or committed custody rows. The 2,416,640-byte peak and retained encrypted source were observed. Cold refusal 247 ms with zero effects. |

All three fresh process-cut scenarios passed in 122.85 s excluding build.
Their post-Held relay publication-attempt deltas were zero. The sibling
managed rollback test passed on the same exact binary in 5.72 s for the author
and 7.03 s for the reviewer. The same binary's legacy capped Welcome proof
passed all four subprocess cases in 96.44 s. These reruns shared the host
with other validation. The normal
and `installed-checks` binary checks passed in 9.32 and 15.11 s respectively.
Strict all-target Clippy with `installed-checks` passed in 39.59 s.

## Reproduce

```sh
cargo test --offline --locked -p marmish-native --lib -j2 private_v2_pending_survives_cold_cuts -- --ignored --nocapture
cargo test --offline --locked -p marmish-native --lib -j2 private_v2_pending_recovers_uncertain_join -- --ignored --nocapture
cargo test --offline --locked -p marmish-native --lib -j2 peer_installed_custody_reopens_signed_prepared_consent_and_held_delivery
cargo test --offline --locked -p marmish-native --lib -j2 peer_capped_welcome_native_proof -- --ignored --nocapture
cargo test --offline --locked -p marmish-native --lib -j2 managed_recovery_hot_journal_and_stock_wal -- --ignored --nocapture
cargo check --offline --locked -p marmish-native --bins -j2
cargo check --offline --locked -p marmish-native --features installed-checks --bins -j2
cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings
cargo clippy --offline --locked -p marmish-native --features installed-checks --all-targets -j2 -- -D warnings
```
