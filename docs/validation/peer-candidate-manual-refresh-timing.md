# Candidate Manual refresh timing validation

In the private Linux AMD64 qualification executable, put `--debug` before
`--manual-recipient`. Start the child with its stderr drained continuously.
A `pending_refresh` record uses schema 1 and relative nanoseconds. The
`schedule_ns` field measures the latest due slot from the actor's monotonic
schedule anchor. The `start_lag_ns` field measures how late that slot entered
the refresh. The `missed_ticks` field counts earlier due slots skipped on the
same rotor turn. The previous implementation advanced the schedule during
initial incomplete history; profiling still starts after history completion.

The Producer returns its existing dual-clock freshness sample only after
accepting the exact ticket and installing the projection. `proof_age` is
that sample minus the corresponding ticket origin. `publish_gap` is the
difference between this and the preceding successful admission.
`prior_slack` is the preceding ticket's one-second horizon minus the
elapsed time to this admission, with negative values for overdue successors.
Both pairs contain `monotonic_ns` and `continuous_ns`. A refusal leaves
both ages and pair data absent as JSON null, and preserves the preceding
success for the next pair. A missing or regressing comparison leaves its pair
null. No clock reading carries a time of day.

The fixture can detect an incomplete interval by comparing neighboring
`schedule_ns` values with the 500 ms period and the latter record's
`missed_ticks`. Collect at least the requested 20 consecutive attempts and
preserve every refusal. Pair gaps cannot describe the whole IPC availability
interval because owner work can invalidate a proof before its clock horizon.
Use the independently timed client reads to measure availability.

The event reuses opt-in debug stderr. The bounded record is written after
the proof decision, release of the view mutex, and the final actor check.
One event is written at most per eligible scheduled attempt with no
catch-up burst or additional logging task. The controller should account
for serialization and stderr output in its measurements and treat missing
output as incomplete evidence.

## Author checks

Linux AMD64 exact source `66fa3c0f29ed8068a966c63e9b744723c2953c8e`
passed changed-file rustfmt and Git whitespace checks. The runtime-identical
source `3f6e2ca` passed stock all-targets Clippy with warnings denied in
29.68 s. Source-matched candidate `--lib --no-run` linked in 43.72 s under
the private candidate cfg with locked offline dependencies. Test runner
SHA-256:

```text
5d85c9e165def62b8daadfa87cd7739fd13c86a690777a924f1e9f7843b1475e
```

| Test filter with one test thread | Passed |
| --- | ---: |
| `manual::tests` | 2 |
| `pending_refresh_is_one_fixed_safe_json_record` | 1 |
| `registered_qualification::tests` | 3 |
| `peer_read` | 26 |

These covered coalesced scheduling, actual accepted clock values, strict
freshness, signed prior slack, fixed schema, argument placement and view
regression. The mounted candidate, output overhead and near-cap qualification
remain the installed controller's measurements.
