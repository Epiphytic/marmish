# Candidate receiver Manual KeyPackage validation

Source `40cb464e5358ebcaa23af7932ad65461ee090e59` was tested against
base `0498e267`. This Linux AMD64 proof exercises registered candidate code and actual upstream
MDK at the locked `bdf9833e` revision. It uses separate disposable processes, a
private profile and file-backed test identity, the real registered default
quota dispatcher, an encrypted installed cursor and a genuinely signed v2
Manual Offer. The fixture qualifies the local native aperture. Authenticated
relay exchange, the installed actor and macOS/ARM64 remain outside this run.

## Exact checks

- Stock `cargo check --locked -p marmish-native --all-targets` passed without
  candidate configuration. The candidate test link and
  `cargo clippy --locked -p marmish-native --all-targets --target
  x86_64-unknown-linux-gnu -- -D warnings` passed with
  `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS='--cfg marmish_peer_receiver_candidate'`.
  Both used offline Cargo, two build jobs and disabled Rust debuginfo.
  The stock check took 21.33 s, the candidate link 41.96 s and strict
  Clippy 27.00 s. Formatting and patch whitespace checks passed.
- The native test `candidate_package_cold_attestation_under_registered_quota`
  runs the original issue and readback behind the exact registered
  lease, inventory and quota. It compares the signed object and durable
  artifact, checks the closed original ciphertext, refuses a changed artifact,
  reopens the same profile for Prepared replay, and checks removal of the
  disposable snapshot and absence of sidecars. All 15 sequences completed
  in 136.07 s.
- The same subprocess matrix covers Bound, a file created before its durable
  witness, empty NativeStarted, readiness stop before allocation and before
  the first schema open, refusal after prestage, a discarded prestage
  witness, changed extent, foreign logical owner, an expired schema turn,
  and readiness loss after signing. A real native DELETE-journal write
  interruption leaves a charged image. The restart uses managed rollback,
  refuses the unsigned result, and cannot generate again. A cancelled
  copy is removed and allows a later cold attestation. A process exit
  inside the copy retains ambiguous residue and prevents cold progress.
  The replacement and oversize cases leave the original blocked.
- Six additional receiver inventory and charged-copy/recovery checks
  covered reinterruption, wrong key, corrupt journal, extent/inode or
  sidecar changes and unchanged ordinary WAL forwarding in 4.30 s.
  The registered-profile forwarding and irreversible-dispatch subprocess
  test passed in 29.82 s. Two fast
  package tests covered checked lifetime arithmetic and exclusive,
  sticky readiness, stop, route cancellation and wall restrictions.
- The v2 signed Offer flight lifetime is one day. The native KeyPackage
  configured lifetime is 30 days, which places the synthetic one-second
  threshold boundary outside the valid signed Offer range. The unit test
  exercises that arithmetic boundary directly.

Copy verification uses a separate quota allocation and may mutate the
disposable encrypted copy during native hydration. The original must remain
byte-identical during successful readback. Neither the schema-only witness
nor a cold unsigned NativeStarted state can yield signing authority.

## Local timing

These are phase timings from the debug candidate with full SQLCipher DELETE
durability. They describe the local native operation and exclude network and
daemon startup.

| Operation | Elapsed |
| --- | ---: |
| Newly allocated encrypted schema, 189 migrations | 6,049.679 ms |
| Historical migrations within first open | 6,021.558 ms |
| Cold keyed original reopen after schema prestage | 26.692 ms |
| Entire bounded original issue | 290.581 ms |
| Separate cold signed readback | 334.056 ms |
| Cold Prepared replay | 338.269 ms |

Across the matrix, successful fresh schema opens took 5.42–7.40 s. The
schema was ready inside its 15-second gate. Issue and separate cold
attestation totalled 624.637 ms, within the 4.5-second local qualification
target. Reopening to replay Prepared repeated the signed-only check without
signing or migrating. No installed feature is enabled by the proof.

## Remaining qualification

This proof exercised the closed-stage cap and separate charged-copy
accounting. It did not fill the aggregate allocation near 128 MiB or
inject an incorrect key directly into the new signed-readback path.
The managed recovery regression separately tested an incorrect key.
Likewise, the registration regression tests changed local configuration,
and the package regression tests a distinct logical owner.
The current stock check compiled all targets without candidate
configuration. A linked stock binary symbol audit is a separate gate.

On a very fast filesystem the expiry case's two-second wall deadline
may outlast the first schema open. The exact run exercised the expired
turn. The registered route must remain current when the actor uses this
aperture, and the actor must reacquire the owner following route change.
