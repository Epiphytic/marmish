# Installed Delivery ingress over private WSS

Exact tested source `f183141f047f51c16f4a34a225926475a15f3f18`.
The tests ran on Linux AMD64 against a disposable authenticated WSS
relay and native SQLCipher, using isolated test identities and stage storage.
The candidate CLI composed into that commit does not yet route to this
library fixture.

| Check | Result |
| --- | --- |
| Exact locked build with installed checks | 43.77 s |
| Full non-ignored native library suite | 1,073 passed, 142 ignored, 1,073.59 s |
| Opt-in Delivery suite | 4 passed in 111.83 s, including three WSS scenarios and the child dispatch stub |
| Independent Held replay repeat on the same binary | Passed in 80.57 s |
| Cursor selection on the same binary | 76 passed, 4 opt-in cases ignored, 65.43 s |
| Consent opt-in regressions on the same binary | 2 passed in 188.52 s, including six scenario subprocesses and the child stub |
| Strict installed-checks Clippy, all native targets | Passed in 37.33 s |

Delivery before the matching Consent receipt survived a cold read as the
same 19,649 signed wire bytes. A true 60-second ACK timeout drove exact-byte
resend. Mixed authenticated history and live observations advanced the cursor
only for their matching receipts. Wrong and replaced native KeyPackage
observations, oversized frames, conflicting wrappers and unrelated receipts
did not admit Delivery. The early authenticated competitor was fenced when
the queued first Delivery became Held. Exact Held replay made no native
KeyPackage reads.

An independent read-only custody wake left Consent settled and recovered
the single saved Delivery. An expired observation was refused without
advancing custody. A separate saved-row expiry check compared authenticated
expiry with both adjacent seconds and retired only at the exact verified
boundary.

| Private fixture observation | Main case | Read-only wake case |
| --- | ---: | ---: |
| Mixed page durably acknowledged | 503 ms | 499 ms |
| Cold retry check | 18 ms | 19 ms |
| Consent ACK to Held | 384 ms | 377 ms |
| Cold Held native package recheck | 323 ms | 324 ms |

The saved-row retirement case took 381 ms for its measured transition.
The single samples exclude process startup and the intentional lost ACK.
They characterize this local workload.

The first integrated test selected a randomized first page to find the
already-Held record. The fixed assertion observes the exact signed stamp
as its own page and verifies the precise record and native-read count.
Each reported WSS run used an archived exact binary so concurrent Cargo
builds could not replace the executable needed by a cold child process.
