# Managed runtime proof: local Trial19

The [exact sanitized report](managed-runtime-linux-amd64-trial19.json) records
`ok=true`, all ten executable assertions, no unmet cases, complete cleanup and
unchanged monitored home metadata. The execution source was published commit
`e5a636d1026978b5c539c898795e554d32118eac`. Source association comes from the
coordinating execution record: this local report has no source-commit field or
CI context, and none has been added. This is a local Linux AMD64 proof, not a
self-hosted GitHub Actions result despite this evidence family's filename.

The actual full tools suite passed 513 tests, 8 skips, exit 0 under managed
CPython 3.13.13 using uv 0.11.16 and system Bubblewrap on Ubuntu 24.04, with a
1320-second total budget. All three actual namespace preflights exited zero.
All ten assertions passed: verified downloads; rejection of bad digest, missing
archive and existing slot; interrupted extraction with clean retry; actual
post-rehash substitution refusal; two absolute interpreters and exact archive
verifier; invalid-cwd configuration isolation and allowlisted child environment;
installed help/doctor/native help/literal argv; hostile ambient/cwd isolation
and restoration; the full suite; and offline retention after removing uv,
download caches and one version. Offline retention used enforced network-
namespace isolation. Native help probes exited 0 without a signal in both slots.

Sanitized progress was available, with 513 matching verbose prefixes, latest
`test_mdk_smoke.ValidationTests.test_send_publication_is_count_not_boolean`.
These are bounded hints, not authenticated execution traces; matching prefixes
are not independently completed tests. The suite summary and exit code establish
the test result. No helper override or CI-context provenance is claimed. No raw
logs are included or published, and no raw logs were fetched for this evidence.

The three fixture source hashes match the
[phase checkpoint](../phases/managed-runtime-native-portability.md). The two
unchanged harness hashes below were also checked against published e5a636d:

- `tools/test_marmish_lifetime.py`: `0792ebc31a107c221a358086a6fe5b04abb1afca20975d3cbd67a15338d148e3`.
- `tools/test_marmish_native_validation.py`: `8145fef064a5c29715d5f205164234516c0862b649b1c323d8e5087e30b2af8d`.
- `tools/test_marmish_transport_sandbox.py`: `fe44db01f8dfc41becc94752e643053ebbf34441befb350dd0400d30c4383fbd`.
- Unchanged managed harness: `fbe6d7a73f4965636e4d699a461b3e0f0b6480778a0ffaf364587001252a5e8a`.
- Unchanged harness tests: `f4b3b25391e631115a88555091d38b026acc4df29acc404a8700b081ac64c332`.

The report's installer-Python SHA256 is
`1e1745b7dda9f380efbd9d6fb3852c195e3650ad190c302f51f3c9908f6f044c`.
Archive URLs, sizes, pinned SHA256 values and runtime audit remain unchanged in
the exact report. Linux doctor/sandbox still require system Python. This proof
is not a production installer release, authenticated-helper full proof, or
three-platform native pass. The separate configured macOS confinement SIGABRT
remains unresolved; see the [new hosted results](managed-runtime-native-41150264034.md)
for measured macOS success and both hosted Linux failures.

Exact report SHA256: `c4698701f03d5f5dea9f168453ccf02756bbbeac05402d4c2a7ad6f9331a8e13`.

Independent Claude Code evidence review, actual `claude-opus-5-5` in auto mode,
APPROVED publication of the six final evidence/status documents. It verified
this report byte-for-byte, source hashes against e5a636d, all proof facts and
privacy; it also verified the three native objects and their selected metadata
consistency. It made no network/CI, tests, native, administrative or git changes.
Source association comes from the execution record, not invented report fields.
