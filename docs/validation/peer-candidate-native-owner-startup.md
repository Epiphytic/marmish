# Candidate native owner startup validation

Exact source `c033631` on Linux AMD64. Both immutable native test binaries ran
`candidate_native_owner_startup_qualification` against fresh disposable
SQLCipher databases and a real registered default dispatcher.

| Build | Primary subprocesses | Additional cold check | Result | Wall time |
| --- | ---: | ---: | --- | ---: |
| Stock with test registration | 29 | 1 stock reopen | Passed | 193.75 s |
| Linux AMD64 candidate | 29 | 1 stock reopen | Passed | 172.92 s |

The cases cover healthy explicit close, implicit drop, smoke-open and
smoke-close displacement, account reopen, Journal smoke close and reopen,
lifecycle read and write, runtime construction, actual SQL errors, and panic
at each listed partial ownership stage. The lifecycle-write case generates
the same Ready profile first and replaces only its disposable MDK account
database with a fresh creation at the candidate turn. This distinguishes
first-write admission from the preexisting-lifecycle read path. The paired
Journal remains present to verify bytes and sidecars. Lease and descriptor
oracles distinguish deliberate retention after fatal from the trusted close.
The healthy case passes a separate process's stock account-binding and
Journal reopen checks.

The connector is inert in this constructor proof. No active service,
invitation actor, or Terminal is started. The daemon caller must run guarded
native preflight with the same registration Admission and profile lease.

Composition `038562c` keeps the reviewed constructor and custody source
byte-identical. Strict engine and native Clippy passed in both stock and
candidate builds. The exact candidate qualification passed in 114.83
seconds. The stock archive passed 60 IPC tests with two opt-in cases
skipped and all five checked Command startup cases. The independent
composition reviewer repeated the native qualification and Journal suites
on both exact archives. See the
[review record](../reviews/peer-candidate-native-owner-startup.md).
