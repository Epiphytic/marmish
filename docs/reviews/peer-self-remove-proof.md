# Exact native SelfRemove proof reviews

Claude Code Sonnet in Auto mode approved the final detailed design in
session `4126b3d6-aa34-490b-b955-1c693bc688c6`. The design SHA-256 is
`edd9cb50ac9aa7446f552a93b5aa59dc109511020caaf7344c524f9394f2cbf6`.
The final design checks the native rewind gate and read-only SQLite
snapshot, both reads of the native LeaveRequest and exact Sent record,
the independently loaded device key, the current native branch and the
authenticated route. The earlier review sessions requested those
bindings and rejected an unsafe write-transaction alternative.

Claude Code Sonnet in Auto mode approved the completed code and tests
in session `1716f0ee-ac27-43db-b8c7-8863d1000a03`, with no blockers.
The exact seven-file scope manifest SHA-256 is
`c772730efe931220647a645b37dc119a71106061b1b9c8741e9136e42acf05c4`.
It includes the lock, native manifest, all three proof source files, the
phase design and the validation document. The reviewer verified all
seven hashes and reran the exact locked offline fixture. One test passed
in 16.54 seconds. The author run passed in 17.17 seconds.

The proof keeps each snapshot synchronous and enforces SQLCipher's
query-only guard. The runtime re-reads native leave evidence and
requires the same exact candidate before the runtime retry. Another
account's real SelfRemove exercises a wrong leaf and a synthetic wrong
key exercises the signer-key check. A true same-account sibling vector
remains ungenerated. The stale MLS proposal is checked against a
newer expected pin. A separate native LeaveRequest negative uses an
older stored leave epoch. A real closed encrypted connection exercises
the storage refusal.

Twenty in-process rechecks measured p50 9.683 ms and p95 10.595 ms
in the author run, and p50 11.313 ms and p95 14.158 ms in the reviewer
run. Both ran in an unoptimized Linux Rust test profile. These numbers
measure local proof only.

The production egress permit, mixed-fanout ordering, inter-snapshot
replacement, a crash without a retained fanout, and convergence at a
remote member remain outside this proof. The fixture is opt-in, owns no
external relay and grants no socket publication.
