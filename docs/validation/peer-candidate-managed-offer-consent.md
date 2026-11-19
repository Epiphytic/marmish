# Managed Peer Offer and Consent validation

The Linux AMD64 candidate leaves bind located, signed v2 Offers to the
registered receiver's quota-managed MDK package and durable Consent outbox.
The checked acknowledgment can settle the current exact attempt once. The
daemon actor, same-port send authorization and Delivery wake remain separate
integration steps.

The opt-in proof uses the locked upstream MDK `bdf9833e`, SQLCipher and
disposable identities. It exercises two sets of three isolated inventory
subprocesses. One set stops before queueing and injects a last-guard rollback.
The other stops after the durable queue write and reopens without signing a
second frame. Both save or recover the encrypted outbox over an unchanged
2,416,640-byte stage. Before settlement, a deferred reservation leaves both
SQL clock floors and the exact attempt unchanged.

Three additional subprocesses use a private NIP-42 WSS relay and real,
correlated acknowledgments. The first settles and refuses another attempt.
The second refuses a committed acknowledgment replay and proves a
last-guard rollback leaves the outbox and clock floors unchanged. The third
advances the checked durable attempt and refuses the old acknowledgment.
The replay and stale cases exercise the registered SQLCipher store and one
actual publication per case.

On author source `5c4da23` the two scenario groups ran in 97.31 s total.
The measured durable queue and post-commit stop steps took 261.886 ms and
160.682 ms. Charged reservation took 399.557 ms and 473.337 ms in the two
recovery cases. The private socket acknowledgment samples were 46.749 ms,
45.676 ms and 48.291 ms. The measured positive settlement took 92.478 ms.
These are paired local debug-profile samples, including a private server
callback. They do not estimate installed throughput or public relay latency.

## Refusal coverage

The candidate binary built from `8ad15b8` ran five private WSS cases and
both three-step SQLCipher inventory scenarios in 138.58 s. The WSS cases
all used one genuinely acknowledged frame per scenario. The two new cases
fenced a different checkpoint owner, a changed same-inode package image,
a stopped native window and the ACK deadline. For equality at the ACK
deadline, each test view derives from a checked real receipt and changes
only the wall coordinate or test receipt deadline. Refusals preserve both
SQL clock floors and the exact cursor file. The touched image is restored
before cleanup.

The wider `prepared_consent` filter passed 36 tests with 14 opt-in tests
skipped in 122.20 s. It includes the repaired legacy queue rollback and the
same SQL transaction's refusal after the fixture persists an authenticated
signed v2 Delivery as Held. The storage fixture uses a synthetic Welcome
and runs no native join.
Stock and candidate strict all-target Clippy passed on `8ad15b8` in 28.57 s
and 27.67 s, respectively. The candidate test link took 44.11 s. Later
changes to the Rust sources affect only test code.

This run observed 48.382–53.428 ms between local publication and checked
acknowledgment and 107.184 ms for positive settlement. Queue and post-commit
stop steps took 255.718 ms and 173.224 ms. Charged reservation took 405.331
and 406.205 ms. The 2,416,640-byte stage size was unchanged. Some timed
local runs overlapped, so the samples include host contention as well as
SQLCipher, native account state and loopback relay work.

The final offline deadline fixture at `fc98a87` passed in 6.43 s. It
follows the existing installed-v2 adoption path for signed Offers. It
places the wall exactly at the Offer and Consent expiry in separate
canonical cases. Current issuance requires the package to outlive the
Offer by 600 seconds, so the artifact guard is exercised by injecting a
shortened expiry at the test-only monotonic stage transition into Prepared.
That case retains an authenticated signed Consent and its actual KeyPackage.
All three refusals leave the outbox and both persisted floors unchanged.
The complete `peer_cursor_store` harness on that source passed 77 tests
with four benchmarks skipped in 71.13 s.

For local reproduction, build the candidate `marmish-native` test library
with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`
and `cargo test --locked -p marmish-native --lib --target x86_64-unknown-linux-gnu --no-run`.
Run its executable with `RUST_MIN_STACK=33554432` and the filters
`managed_consent_ --ignored --nocapture --test-threads=1` and
`prepared_consent --test-threads=1`. Run the offline deadline fixture with
`cargo test --locked -p marmish-native --test peer_cursor_store managed_consent_transaction_refuses_each_exclusive_deadline`.
Use `cargo clippy --locked -p marmish-engine -p marmish-native --all-targets -- -D warnings`
for stock and add the candidate target flag and Rust flags for the candidate.
The measured binaries used two build jobs, dev and test debug info off,
and no incremental compilation.

The durable reservation floor and checked ACK wall have separate anchors
in this leaf. An ACK observation below either SQL floor is refused without
a state change. The later same-port turn must compose these clocks before
it can drive installed publication.

The stale-ACK case first uses the genuine checked receipt and its actual
sample to show clock regression after a second attempt advances both
floors. It then reuses the same genuine receipt and first attempt, changing
only the test observation wall to equal both floors and remain strictly
before expiry. The durable transaction must return exact staleness before
the commit callback. It verifies both floors and cursor bytes again.
The charged original image and the single actual relay publication also
remain unchanged.

The final candidate source `c8237e379cfcc614b122ab6588c3aec32daac2f5`
linked in 43.71 s on Linux AMD64. The archived test executable has SHA-256
`5de2405e8d7ab2f71d227165dbba59d965c21432f7bec5904efe1a8769482bfa`.
The focused stale-ACK case passed in 14.80 s, including MDK setup and an
actual private WSS acknowledgment measured at 51.432 ms.
All five WSS cases passed in the final aggregate run. Local ACK time was
52.547–54.039 ms across those five cases and positive settlement took
80.299 ms. Its queue/recovery group hit the existing 15-second native
prestage fence in a cold child that spent 14.88 s in SQLite migrations
under concurrent host load. The exact same queue scenarios passed
in 24.72 s on the preceding source whose only later change was the stale
WSS assertion. The deadline and production code were unchanged.
Rerunning the queue group independently from the final exact binary
passed both restart scenarios in 48.72 s. The two cold migration samples
were 14.26 s and 13.77 s. Queue and post-commit stop took 310.880 ms and
189.370 ms. The charged reservations took 502.445 ms and 467.030 ms.
Claude's independent final rerun also passed the queue group.

## Exact ordinary native startup composition

Linux AMD64 composed source `e080e804dfbf84713f70533d6a5162303f7456c2`
passed stock and candidate strict Clippy in 32.64 and 33.46 seconds
respectively. The candidate link took 48.44 seconds. The candidate
archive passed the managed Consent opt-in filter in 82.11 seconds,
including five private WSS cases and both restart scenarios.
Local acknowledgment samples were 47.271–49.398 ms. Queue and post-commit
stop took 244.587 and 150.418 ms, with no change to the 2,416,640-byte
stage image. This test drives prepared Consent against the real store
and relay. It does not exercise same-port actor publication or Delivery.
