# Selected Linux relay copy deadline rerun

**PASS: four selected cases in 2440.850466 seconds.** This corrective run
repeats the four cases below 1.5× deadline headroom in
[the full 12-case run](copy-runtime-live-r4.md). It is not another full suite.
The unchanged [JSON report](copy-runtime-live-r5-corrective.json) has SHA-256
`8f60bfb89b0e0ec0b296a645d12517f517adda80c8debad7b1b80aa89c61b0fc`.

| Case | Exit | Seconds | Payload bytes/s | Deadline headroom |
| --- | ---: | ---: | ---: | ---: |
| put_zlib | 0 | 436.492758 | 4,823.3 | 7.560× |
| get_zlib | 0 | 541.608404 | 3,887.2 | 6.093× |
| put_existing_destination | 1 | 730.498139 | — | 4.517× |
| get_existing_destination | 1 | 716.894207 | — | 4.603× |

Each process had a **3300-second deadline**, exceeding four times the longest
selected baseline observation, 796.870511 seconds. Headroom uses the separately
recorded elapsed time including local validation; the table's operation times
and goodput use the inherited transfer timer. All four reruns satisfy the
4× margin. The original raw Put/Get results remain functional passes below
4×; they exceeded 1.5× and were not selected for mandatory corrective reruns.
Earlier failures and the original observations remain unchanged.

Both zlib transfers copied **2,105,344 bytes** with matching SHA-256
`8ec5adcd58753dd6f8c02575febd5f8ebf6ebac2e9bdb4372546b40c4c2aa936`.
The harness checked exact contents, mode 0640, nanosecond mtime
1700000000.123456789, literal special-character paths, empty stdout, verified
commit confirmation and negotiated zlib. The random fixture is newly generated,
so its hash differs from the baseline. Both existing-destination cases returned
`transfer_conflict` without changing the destination's inode, mtime or contents.
They are expected refusals, not successful transfers. No preparatory remote
transfer was required for these fixtures.

The explicit relay was `wss://relay.eu.whitenoise.chat`; both endpoints ran
on the same Linux AMD64 host. The immutable native binary SHA-256 was
`ef9f55b88c4b69a267abedf431e73973332f07110309283f50ff93c774bb716d`,
identical to the baseline. The baseline JSON hash was independently checked:
`5f96d205251666672dc0a89d0327a1ad7f4a7cf7b980db92bb35b80e790d9fa3`.
The harness records `source_revision_verified: false`; the earlier immutable
build context separately records base commit
`33753e271595da1651780a5d80fb0d067b6293f4` and dirty Rust source fingerprint
`c31ae8e848234aeabe71066c12ad9fac4525035ddcb514037cac35820e730738`.
This predates the post-receipt uncertainty fix in 23105d8 and does not exercise
that correction. Its version string is 0.2.3, but this is an unreleased source
build; public v0.2.3 assets do not contain native copy.

The report records both owned daemons exiting 0 and private test state removed.
An independent process scan after completion found no process using the owned
immutable product executables. Independent report checks verified the selected,
completed and omitted sets; baseline and binary hashes; positive payloads,
hashes and codec; refusal exits; goodput and headroom arithmetic; and cleanup
records. These checks did not repeat the transfers.

Observed goodput includes setup and commit, not just data transit. No network
wire-byte count or encrypted-network compression ratio was measured. The
eight omitted full-suite cases are explicit in the JSON. This run does not
qualify an installer, another platform, Tor, daemon restart, live cancellation
or interrupted resume, retention GC, more-than-65-MiB relay transfers, or a
historical installed-format upgrade. See the
[qualification amendment](../phases/streaming-copy-qualification.md) and
Beads `marmish-8ka.5` / `marmish-8ka.6` for those limits.
