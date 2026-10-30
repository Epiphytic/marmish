# Managed harness test portability correction

Hosted [run 41141100601](https://github.com/Epiphytic/marmish/actions/runs/41141100601)
tested source `fc8756aa0bcf60e54149abf38431e1712c3775d5`. Installer archive/PTY
jobs passed on all three targets. macOS managed packaging stopped in its offline
contract suite: 62 tests, 1 platform skip, 2 errors. No macOS runtime proof ran
and no managed report was produced; the missing-report upload also failed.

The two errors were test fixtures: simulated Linux namespace-ladder code reached
the real macOS helper validator, and simulated Linux offline code read the real
`/proc/self/ns/net`. The correction supplies explicit validated-helper and
outside-network-namespace fixtures, bounded policy reads, and guards against
accidental host validation/namespace access. Selection-refusal and strict helper
environment assertions remain. No harness/application behavior changes, new
skips or relaxed integration assertions are introduced.

All 62 focused tests pass normally and with symlinked TMPDIR after the fix.
Real Linux namespace and cancellation tests are unchanged. This is Linux-host
validation of portable fixtures; a fresh native macOS run is still required.
The preceding macOS 900-second full-suite timeout remains unresolved.

The same originating run's Linux AMD64 report reached the full suite: 478 tests,
7 skips, exit 1, with the same two required namespace failures. All three new
executable completion cases passed (`unmet_acceptance=[]`), cleanup completed
and monitored home metadata was unchanged. It does not waive those failures.
Raw CI diagnostics were inspected privately and are not committed.
