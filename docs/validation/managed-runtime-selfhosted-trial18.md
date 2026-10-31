# Managed runtime proof: local Trial18

The [exact sanitized report](managed-runtime-linux-amd64-trial18.json) records
ok=true, all ten executable assertions, no unmet cases, complete cleanup and
unchanged monitored home metadata. Run on Linux AMD64 using system Bubblewrap,
uv 0.11.16 and managed CPython 3.13.13, with the default 1320-second total budget.
No helper override, CI context, private failure log, privilege or host-policy
change was used. Historical Trial17 and failed native reports remain unchanged.

The final tools suite passed 505 tests, 8 skips, exit 0, under the managed
interpreter. Progress was available with 505 matched prefixes, last naming
`test_mdk_smoke.ValidationTests.test_send_publication_is_count_not_boolean`.
These are bounded hints, not an authenticated trace or proof of hang causality.
All three namespace preflights exited zero. Verified downloads, interrupted
extraction/fresh retry, post-rehash archive substitution refusal, two runtimes,
exact verifier, hostile ambient/cwd restoration and installed help/doctor/native
help/literal argv passed. Offline retention passed after removing uv, downloads,
caches and one version, with enforced network-namespace isolation.

Before this proof the agent passed both affected modules normally and through a
symlink (48 tests, 1 skip each), and the full system-interpreter suite (505 tests,
8 skips, 21.414 seconds). Independent Claude final code review repeated the same
module counts and full suite (21.8 seconds) and approved publication. Actual Linux
namespace cases executed successfully; the new actual-Darwin-only test accounts
for the additional Linux skip. The review's deliberately broken cleanup mutation
required a supervisor termination, separately disclosed in the
[code review](../reviews/managed-runtime-fixture-portability-code.md); it is not
a normal source-test or naturally bounded mutation success.

Quality analysis reports 25 findings, three gating (historical churn and growth
of an existing test class), three minor and 19 new-symbol findings. No suppression
or baseline reset was used; no passing quality-gate claim is made. No production,
harness, workflow, deadline or Linux acceptance requirement was changed.

This is a local working-tree proof against baseline `20c76f1`, not a three-target
native pass, authenticated-helper full proof, configured macOS confinement test,
production uv installer or release. Linux doctor/sandbox still use system Python;
the independent configured macOS SIGABRT remains unresolved. The native rerun is
required to establish that the actual macOS suite now exits successfully.

Final source SHA256 values:

- sandbox tests: `bfd8da1b74c006b92f3061423f93d385346f2fd1c250bf28c0444caaf89ae61f`.
- smoke tests: `c635d8642e2a77b94099a54e255f646b934802a90db724760d95ffb5491081c7`.
- unchanged managed harness: `fbe6d7a73f4965636e4d699a461b3e0f0b6480778a0ffaf364587001252a5e8a`.
- unchanged harness tests: `f4b3b25391e631115a88555091d38b026acc4df29acc404a8700b081ac64c332`.

Exact report SHA256: `c8a81af28116d3929290d595512f09d9f654346881ba7b6484b3e32dc805610a`.
