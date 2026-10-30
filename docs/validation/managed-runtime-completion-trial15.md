# Managed runtime integration: Trial15

The [real local report](managed-runtime-linux-amd64-trial15.json) records Linux
AMD64 system-Bubblewrap success: all executable completion cases, independent
offline network denial, complete cleanup and unchanged monitored home metadata.
The managed full suite ran 478 tests with 12 reported skips and exit 0.

Five of those skips are the new workflow tests' optional PyYAML dependency,
which the clean managed interpreter does not have. That gap is retained in this
historical result and is being removed before final integrated validation.
The original seven reported skips are unchanged; no namespace or cancellation
case was skipped. All 62 focused harness tests passed normally and with symlinked
TMPDIR, including real SIGTERM, KeyboardInterrupt and deadline cleanup tests.

This run uses system Bubblewrap, not the authenticated-helper path. Native CI,
runner-group policy, production installation and the configured macOS SIGABRT
remain unvalidated by this local run.

Source SHA256:

- `tools/marmish_managed_runtime_spike.py`: `12be7e7ca71cb5d22f3c5cdd048314476e869f06338cdd7342bdbaaa10c477ad`
- `tools/test_marmish_managed_runtime_spike.py`: `b849b62f612ff293cdd9fe7c923728eb508f05b3a9f5f7b4963cc49537324eb5`
