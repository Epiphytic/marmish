# Managed runtime proof: local Trial17

The [exact sanitized Trial17 report](managed-runtime-linux-amd64-trial17.json)
records `ok=true`, no unmet executable cases, complete owned cleanup and unchanged
monitored real-home metadata. This Linux AMD64 run uses validated system
Bubblewrap, managed Python 3.13.13 and uv 0.11.16. The default total deadline was
1320 seconds. No private failure log, CI context, helper override, host-policy
change or privilege escalation was used. Historical Trial16 remains unchanged.

Both installed versions passed launcher, doctor, literal-argument and native-help
checks. All three namespace diagnostic stages returned zero. The real interrupted
extraction, archive substitution after rehash, and hostile ambient/cwd cases
passed. Offline retention passed after removing uv, downloads, caches and one
version, with independent network-namespace enforcement.

The final managed full tools suite ran 495 tests with 7 expected skips and exit 0.
Verbose progress was available: 495 valid starts were observed, ending with
`test_mdk_smoke.ValidationTests.test_send_publication_is_count_not_boolean`.
These bounded child-output identifiers are progress hints, not an authenticated
trace or causal hang attribution. No raw output is stored in this evidence.
Before this proof, the same 495-test suite passed under the system interpreter
(20.640 seconds), and all 79 focused harness tests passed normally (11.623 seconds)
and with symlink TMPDIR (11.520 seconds). The later test-only follow-up strengthened
the real RecursionError and supervisor-return contracts without changing harness
source. All code increments received independent Claude approval before this run.

Quality analysis exited zero with no gating findings. It still reports three
minor changes to existing symbols: the optional capture parameter on `run` and
`_run_process`, and two additional cleanup branches in `_run_process` (complexity
14 to 16). New-symbol findings include unittest discovery/callback reachability
and the focused fixture class size; this is a disclosed disposition, not a claim
that the quality tool reported no findings.

These are local Linux AMD64 results, using system Bubblewrap rather than a full
authenticated-helper proof. The separate [authenticated helper smoke](managed-runtime-helper-smoke.json)
remains bounded smoke evidence only. This run establishes neither three-target
native acceptance, runner-group isolation, configured macOS startup/confinement,
a production installer change nor a release. Linux doctor and sandbox code still
require `/usr/bin/python3`; launcher startup under the managed interpreter does
not establish absence of all system-Python dependencies. The configured macOS
SIGABRT remains separate and unwaived.

Source SHA256 for this working-tree run:

- `tools/marmish_managed_runtime_spike.py`: `fbe6d7a73f4965636e4d699a461b3e0f0b6480778a0ffaf364587001252a5e8a`
- `tools/test_marmish_managed_runtime_spike.py`: `f4b3b25391e631115a88555091d38b026acc4df29acc404a8700b081ac64c332`
- `.github/workflows/native-validation.yml`: `479f3bccd0d21d6c7b8663a83ba9cb22e200d885841e53b0af7cead810ae235f`
- `tools/test_marmish_managed_workflow.py`: `3e36589e3b9bd1cc82f818ae1343229e3911732ed438bafbd76f897bd14b8505`
