# Near-cap SQLCipher fixture validation

The [design](../phases/peer-candidate-near-cap-fixture.md) specifies the
fixture input to installed Manual timing qualification. The database is
a real encrypted native Pending stage. SQLite returns the scratch growth
pages to its freelist, so this workload represents allocated encrypted
image hashing, copying and native readback over an unchanged MLS group.

## Diagnostics during implementation

The first candidate test at `cdfcef4` failed after 20.07 seconds in
`stage_pragma`. The bundled SQLCipher dispatches direct
`PRAGMA page_size` to its cipher setting and exposes its text value.
The SQLite pager setting is available through the checked table-valued
query used by the locked MDK. The fixed fixture reports named errors
for the individual settings.

A small probe linked against the same bundled SQLCipher verified that
increasing the actual pager extent then dropping the scratch table
retained 3,968 pages with 3,961 freelist pages and 16,252,928 logical
and allocated bytes on ext4. This probe used disposable synthetic
SQLite content. A separate low-cap probe reported primary
`SQLITE_FULL` (13) and returned to autocommit with the original
logical extent and integrity. Encryption rewrote bytes in the
restored page, which the rollback test now allows only on the private
copy. These probes isolate SQLite behavior from the native and installed
tests.

## Exact offline native selector

The Linux AMD64 candidate test executable stamped with composed source
`9b30b76` passed the isolated selector after 22.26 seconds.

```text
--ignored --exact peer_invitation::prepared_consent::joined_tests::exact_joined_snapshot_grows_to_near_cap --nocapture
```

The six fixture Rust files and phase design match author source
`c45c926` byte for byte. The test joined a genuine two-account native
stage, grew it to 16,314,368 logical bytes and 16,318,464 allocated bytes
on ext4, and measured 3,983 pages of 4,096 bytes with 3,384 on the
freelist. Native readback on a separate checked copy retained the exact
group, canonical epoch, own leaf and receipt. The selector also checked
the private-copy cap refusal and the digest of the untouched original,
along with the wrong-key, exact inode, file layout, allocation boundary
and active receiver VFS refusals. SQLCipher emitted its expected
wrong-key HMAC diagnostic. This run exercised the native stage helper
without relay traffic. Installed cursor selection, byte and quota
observation, List and Show, and latency remain separate controller
evidence. An explicit corrupted-page control was optional in the design
and was not executed.

## First installed run

The composed Linux AMD64 candidate built, passed strict checks and was
installed from the checksum-verified package at `9b30b76`. The installed
near-cap refresh selector reached live Pending on a fresh image and grew
the exact selected stage. Closed original measurement matched the offline
geometry and extent above. The subsequent cold candidate did not publish a
Pending read view within the controller window. It reported 25
`proof_refused` samples, zero publications, and zero selector failures.
The selector exited failed after 84.44 seconds. This run establishes
the installed-stage growth and selection path. Cold charged List and
Show and the warm timing samples still require a passing installed run.

The refusal label covers both proof assembly and the final ticket clock
check. A separate unoptimized SHA-256 diagnostic measured 244.5–255.7 ms
per hash of a synthetic 16,314,368-byte buffer. The native path hashes the
original three times and the copy twice before publication. At that
measured speed, five hashes alone would exceed the one-second lifetime.
The diagnostic did not identify which check refused the installed proofs.

The separate ordinary Manual regression passed on the same installed
debug package pair in 54.11 s. It reached exact Pending 12.972 s
after Offer, restored the same invitation in 7.601 s after a cold
restart, and made the local view unavailable within 270 ms of the
attributed Peer connection loss. Both binary command results survived
the loss and were read by the separately installed stock daemon.

## Optimized installed qualification

The exact `9b30b7620141d1a703f7a1f6e9c4572f8f8d727b` candidate and
stock packages were built with Cargo's release profile, locked offline
dependencies and Rust 1.97.1. The controller was also optimized. Both
packages were installed with the native installer using their SHA-256
manifest digests. The run used Linux AMD64, an AMD Ryzen AI Max+ 395
host, glibc 2.39, ext4 and two independent private loopback WSS relays.
The locked upstream MDK master revision was `e4421e86`. Manifest digests:

| Package | SHA-256 |
| --- | --- |
| Private receiver candidate | `b8f4739eb9cb61e775853b5ed4151b97044160114e71e331300ead04884168be` |
| Stock | `bfe5c13c07175d7f50bfc0e0cbbc2d3181d5a431ba7ded97f8dfe91f9a3acffc` |

The installed selector passed in 72.13 s. The empty listener returned
Unavailable and recorded three NoPending slots before invitation. The
independent founder completed one Offer, Consent and Delivery. Pending
first appeared after 20.663 s and the identical record appeared 1.700 s
after the cold start on the grown near-cap image. Both binary Command
results remained readable by the separately installed stock daemon. The
command round trips took 3.528 s before and 3.716 s after the Peer route
cut. The same-UID Peer read became unavailable within 84 ms of that cut.

Each cohort used 20 consecutive 500 ms scheduled slots, with 20 verified
publications and 20 bracketed comparisons. There were zero missing,
refused or missed slots in each. Monotonic and continuous clock values
agree at the precision below. With 20 slots in one run, the nearest-rank
p95 is the second-largest sample.

| Cohort | Proof age p50 / p95 / max | Publish gap p50 / p95 / max | Minimum prior-proof slack | Maximum start lag |
| --- | ---: | ---: | ---: | ---: |
| Independent | 283.79 / 306.43 / 309.54 ms | 497.38 / 537.27 / 538.91 ms | 190.67 ms | 2.13 ms |
| 96-way offered IPC pressure | 265.99 / 285.41 / 285.95 ms | 496.93 / 526.70 / 543.97 ms | 195.22 ms | 20.40 ms |

In both cohorts the proof-age p95 was below 500 ms, every publication
took under one second and every paired prior-proof slack was positive. The recorder found
zero clock-expiry gaps in the bracketed intervals. These measurements
cover proof publication. The current projection can also be unavailable
to individual listeners, as the independently timed requests show.

| Reads | Requests | Exact | Unavailable | Busy / timeouts / unexpected | Reply p50 / p95 / max |
| --- | ---: | ---: | ---: | ---: | ---: |
| Independent probes | 20 | 20 | 0 | 0 / 0 / 0 | 29.02 / 71.61 / 116.47 ms |
| Probes with concurrent pressure | 20 | 8 | 12 | 0 / 0 / 0 | 38.71 / 69.71 / 77.23 ms |
| Pressure requests | 12,192 | 8,121 | 4,071 | 0 / 0 / 0 | 31.21 / 58.92 / 78.71 ms |

Each probe series alternated 10 List and 10 Show requests. The pressure
generator sent equal numbers of List and Show. The read listener maps
slot refusal and other unavailable-view conditions to the same
Unavailable response. The same candidate executable separately passed
`ipc::peer_read_listener::tests::fifth_read_is_unavailable_and_fatal_refuses_four_admitted_replies`. This run measures availability under
offered load and does not attribute an individual refusal to a slot.
The elapsed reply percentiles include all results. The collector read
18,777 stderr bytes including 13,251 bytes of timing events. It spent
8.648 ms in 969 capture reads. The candidate's time writing stderr
was not measured separately. The pairing baseline accounts for the
41st event outside the two groups of 20.

| Measured object | Logical bytes | Allocated bytes |
| --- | ---: | ---: |
| Closed original (3,983 × 4,096-byte pages, 3,384 on the freelist) | 16,314,368 | 16,318,464 |
| Checked snapshot copy | 16,314,368 | 16,314,368 |

The stable copy appeared within 9 ms of starting extent sampling after
the two cohorts. Aggregate charge was 84,013,056 bytes including the
49 MiB reservation and the greater of logical or allocated bytes for
each image. This is strictly below the 128 MiB admission limit. The
test asserted that the original's closure, inode and encrypted bytes
matched its pre-reopen witness after the installed process stopped.

The successful installed run kept the source and timing limits of the
debug attempt and used the same page geometry. These results qualify the
measured optimized workload on this Linux AMD64 host. Public relay and
native ARM64/macOS timing require separate measurements.
