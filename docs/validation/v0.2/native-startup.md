# Native relay startup amendment

Source parent `01117e45ca94a262dfac1e5392bcb448835925c8`; scope and fixed
contracts are in [the approved amendment](../../phases/v0.2-phase1-startup.md).
The [actual Claude completed-code review](../../reviews/v0.2-native-startup-code-r1.md)
approves the integrated source, independently passing 32 focused tests and
strict native/vault all-target Clippy. No blocking findings remain.

Root's integrated offline run passed 108 native tests, 8 vault tests and 3
helper tests. The separately owned keychain gate was ignored in this run.
The final added early-wait regression was completed during that run; all 8
startup tests then passed against the final revision (101 other tests filtered).
Final strict native/vault all-target Clippy with warnings denied, workspace
formatting, `git diff --check` and actionlint passed. Cargo.lock is unchanged.

The scoped static quality delta exited 0 with no gating regressions. It reports
new trait/test symbols as unreachable, similarity between explicit finite match
tables and scripted test setups, and a two-line minor growth in connector code.
The trait and tests are compiled and exercised. These findings were inspected;
no baseline or suppression was introduced.

Startup now requires an observed Connected state before admitting publication,
rather than interpreting an SDK no-op success as readiness. Pending and queued
restarts remain within three original bounded turns. Diagnostic categories are
finite; unknown errors never expose their contents. Hosted clients enable the
existing opt-in debug output on stderr, preserving public JSON shape.

The reviewer notes that configured endpoints are observed sequentially, without
a final simultaneous connectivity snapshot; the network can change afterward.
Debug status polling is bounded but can be noisy. The scripted seam validates
readiness admission; publication call ordering is source-reviewed rather than
instrumented. These are nonblocking limitations of this scoped amendment.

This does **not** establish the cause of the previous hosted failures. A fresh
full four-job exact-source direct/Arti matrix is required after publication.
Phase 1 remains open, and no remote-command, installer or release acceptance is
claimed by these offline results.
