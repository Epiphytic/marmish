# Manual receiver managed package proof

The Linux AMD64 candidate reader binds the sealed package proof to the
registered quota, current installed Manual checkpoint incarnation and
encrypted stage image. The queued read opens the separately charged
snapshot instead of reissuing the package. It verifies both wall floors
without advancing them. This cut can read an already committed outbox.
It leaves the daemon, producer, send, ACK, Delivery and Pending paths
at their existing gates.

The queued proof case uses one real enrolled local profile and encrypted
MDK SQLite at the checked upstream dependency in `Cargo.lock`. It
runs seven isolated subprocess steps with the same recorded ciphertext
and filesystem identity. Alongside successful cold native attestation,
it covers wrong account and origin, expiry and regressed wall, a foreign
checkpoint, same-inode content change, same-byte replacement, an exact
retained journal, sticky stop, hard process exit during a charged copy,
and refusal after restart with that copy still present. The reader
produces no signature or native write. The test compares the cursor and
stage bytes before and after the operation and checks snapshot presence
or absence for the corresponding cut.

The unchanged existing native package matrix passed 15 scenario groups
in 209.11 s on `c6c48a9`. All product operations and the old-case source
are identical in the later Cut 1 snapshot apart from fixture method
visibility and an explanatory comment. These were local tests with
disposable identities and no relay contact.

The encrypted stage measured 2,416,640 bytes. The final author snapshot
`6aa749d` passed the queued restart regression in 14.61 s across all
seven subprocess steps. Initial schema preparation took 7.756 s
(189 migrations, outside the five-second native turn), key package
issuance took 296.202 ms, cold charged attestation took 373.971 ms,
and the later original re-digest took 43.630 ms. Both fast package
lifetime and sticky revocation tests passed. The locked candidate
all-target Clippy pass for both native and engine completed in 31.81 s
with warnings denied. The equivalent stock run passed in 30.96 s.
Both include the standalone cursor target. Claude Code reran the
final archived queued restart regression independently in 17.40 s.
These timings describe a debug-profile Linux AMD64 harness. A larger
valid encrypted image and other platforms remain unmeasured. The
fixture-authority negative compile probes and release-shaped stock
binary symbol audit remain open for combined actor qualification.

`recheck_managed_package` checks only the retained stage identity and
ciphertext. Before sending or settling an acknowledgment, the next cut
must also refresh the exact queued SQL witness or verify the exact
attempt in the same transaction. The changed-attempt and ACK stale
witness regressions are required in that cut under `marmish-dwt.4.12.3`.
