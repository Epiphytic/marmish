# macOS provider supervisor correction: offline evidence

Base source: `7c49c3785eab010bf017c1bb7f03511524312b95`. Task: `marmish-4lf`.
The isolated correction does not change product credentials, ACLs, workflows or
release pins. Its [approved design](../../phases/v0.2.1-macos-provider-supervisor.md)
records the exact failure and ownership constraints.

Candidate [41698628787, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41698628787)
failed the macOS offline supervisor suite, before the real provider qualification
step. That qualification was **skipped**, not failed or passed; no macOS provider
report exists. Both Linux provider jobs and all three install quickstarts passed.
The full private macOS job log has SHA256
`b3b9269ebd3dde35797991c4a9ad2b76cf999b0a217e63e12257d6eb9dfc2dfb`.

Two lifecycle fixture errors were reproduced on Linux through a symlinked parent
directory. Canonicalizing the fixture root fixes that comparison. The EPERM
cleanup explanation is supported by Apple XNU source; the runner's exact race
scheduling has not been reproduced natively. The correction accepts only a fresh
ESRCH after reaping the exact owned child, and rejects permission denial or an
unknown/remaining group.

Author checks on 2026-11-03, Linux:

- `python3 -m unittest discover -s tools -p test_marmish_native_keychain_check.py -v`: 23 passed (0.158s).
- `python3 -m unittest discover -s tools -p test_marmish_native_https_check.py -v`: 24 passed (0.218s).
- `python3 -m py_compile tools/marmish_native_keychain_check.py tools/test_marmish_native_keychain_check.py`: passed.
- `git diff --check`: passed.

The repeated real short-output test exercises process-exit timing, while mocked
negative cases prove denial, timeout and unknown outcomes remain red. It is not
proof of Darwin EPERM behavior. Native macOS offline tests and actual disposable
Keychain qualification remain pending.

`ripwire tools --quality-delta` exits 2: two existing unittest classes grew from
new negative coverage, and its `stop` name collision attributes intentional
finite rejection of unknown OSError to `marmish_app.py`. New unittest methods
also appear unreachable to static analysis despite actual discovery/execution;
small repeated mock scaffolding remains. No suppressions or baseline changes
were added. An actual duplicate overflow test was consolidated into the existing
case with eight bounded repetitions.
