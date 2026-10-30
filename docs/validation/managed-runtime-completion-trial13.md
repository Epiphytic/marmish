# Managed runtime completion: local Trial13

Source baseline: `8e19cdf`; reviewed working-tree source hashes below bind this
actual local run to its implementation. This is Linux AMD64 evidence, not native
three-platform acceptance or a production installer release.

The [sanitized real report](managed-runtime-linux-amd64-trial13.json) records
`ok=true`, no unmet executable cases, complete owned cleanup and unchanged
monitored real-home metadata. Actual pinned uv observed extraction interrupted
by SIGKILL with its group gone, no exposed runtime and a successful fresh-slot
retry. Actual uv rejected the same-payload gzip archive substituted after the
final rehash, with privately verified exact expected/computed digests. The
hostile-parent environment and unrelated cwd case installed the second runtime
and preserved literal launcher arguments; environment and cwd were restored.

The managed full tools suite ran 459 tests, with 7 reported skips and exit 0.
All three diagnostic namespace stages exited 0. Independent Linux network
namespace/socket denial and retained launcher checks passed after uv/download/
cache removal and deletion of the other version. No private failure log was
created. Focused tests: 49 pass normally and under an owned symlinked TMPDIR.
Whitespace checks pass. Independent integrated Claude review follows separately.

Quality analysis has no remaining complexity regression. Nonzero findings cover
the reviewed observer/cwd/callback parameter seams, unittest class length,
discovery/callback dead-code false positives and a short polling-loop similarity
to an unrelated macOS test. These are recorded as inspected limits, not a clean
quality-tool pass; no unrelated cross-module helper was introduced for a score.

Native CI and the macOS confinement SIGABRT remain incomplete. Historical failed
reports and the earlier Trial12 report are preserved unchanged.

Source SHA256:

- `tools/marmish_managed_runtime_spike.py`: `efee8955efa38ff69931b30e51242c792781db534163be21e015cb855f660a39`
- `tools/test_marmish_managed_runtime_spike.py`: `f711bc3b2f8e20373fa798147f182f115503b72575ebe07b4f58087fb6d3e323`
