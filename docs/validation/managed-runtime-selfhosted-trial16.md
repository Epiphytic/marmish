# Managed runtime proof: local Trial16

The [sanitized Trial16 report](managed-runtime-linux-amd64-trial16.json) records
`ok=true`, no unmet executable cases, complete owned cleanup and unchanged
monitored real-home metadata. This Linux AMD64 run uses the validated system
Bubblewrap selection. It passed all three real completion cases, both installed
runtime/launcher checks, the namespace diagnostic ladder and independently
enforced offline retention after removal of uv, downloads, caches and one version.
The managed Python full tools suite ran 478 tests, 7 skips, exit 0; the new
stdlib workflow contracts ran without the five optional-dependency skips seen
in historical Trial15. The 62 focused harness tests passed normally and with
symlinked TMPDIR; real SIGTERM, KeyboardInterrupt and global-deadline tests
verified owned child reaping, scratch cleanup and sanitized interrupted reports.
The configured full-proof deadline was 1320 seconds. No private failure log was
created. Historical Trials13, 14 and 15 remain unchanged.

The separate [authenticated helper smoke](managed-runtime-helper-smoke.json)
passed the existing Ubuntu Noble package bootstrap/probe, required-helper
selection and digest-bound provenance, supervisor environment isolation, all
namespace stages and independent offline enforcement using the validated helper.
Ownership-checked cleanup removed its helper prefix and state. This was a
bounded helper smoke, not a complete helper-mode managed-runtime proof. Its
`sysctl` PATH presence check was false under `/usr/bin:/bin`; the absolute
`/usr/sbin/sysctl` namespace preparation succeeded. The existing bootstrap probe
receives its required helper pair; harness application, diagnostic and offline
environments omit that pair, while the full-suite supervisor includes it.

These are local Linux AMD64 results. They establish neither three-target native
acceptance nor runner-group isolation, configured macOS startup/confinement,
a production installer change or a release. Independent integrated review and
native CI evidence are tracked separately. Quality analysis has no complexity
regression; deliberate callback/cwd parameters, unittest length, a standard
fixture setup and a short polling-loop similarity remain inspected nonzero
findings rather than a claimed clean quality-tool result.

Source SHA256 for this working-tree run:

- `tools/marmish_managed_runtime_spike.py`: `12be7e7ca71cb5d22f3c5cdd048314476e869f06338cdd7342bdbaaa10c477ad`
- `tools/test_marmish_managed_runtime_spike.py`: `b849b62f612ff293cdd9fe7c923728eb508f05b3a9f5f7b4963cc49537324eb5`
- `.github/workflows/native-validation.yml`: `479f3bccd0d21d6c7b8663a83ba9cb22e200d885841e53b0af7cead810ae235f`
- `tools/test_marmish_managed_workflow.py`: `3e36589e3b9bd1cc82f818ae1343229e3911732ed438bafbd76f897bd14b8505`
