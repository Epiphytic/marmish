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

Direct bridge coverage is still open for wrong signed ancestry, foreign owner
or quota, expiration in each state, lost route or role, unequal v2 negotiation,
wrong native package, roster or epoch, and a separately injected uncertain
native effect. Cold pre-effect and capacity readback prove that qualification
was withheld. Their generic refusal assertion leaves the precise readback
error unmeasured. The replay proves `AlreadyApplied` and zero package reads.
There are no SQLite statement counters. The supervised daemon, installed
approvals, application status, application or receipt egress and other
architectures need separate qualification.

## Reproduce

```sh
cargo test --offline --locked -p marmish-native --lib -j2 private_v2_pending_survives_cold_cuts -- --ignored --nocapture
cargo test --offline --locked -p marmish-native --lib -j2 peer_capped_welcome_native_proof -- --ignored --nocapture
cargo test --offline --locked -p marmish-native --lib -j2 managed_recovery_hot_journal_and_stock_wal -- --ignored --nocapture
cargo check --offline --locked -p marmish-native --bins -j2
cargo check --offline --locked -p marmish-native --features installed-checks --bins -j2
cargo clippy --offline --locked -p marmish-native --all-targets -j2 -- -D warnings
```
