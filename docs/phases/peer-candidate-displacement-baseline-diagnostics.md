# Diagnose the installed two-cut dispatcher baseline

An exact Linux AMD64 run against the same receipt-installed candidate
and stock artifacts passed Ready in 13,101 ms, then failed the Status
case in the generic controller-header reader with an early EOF. A
single confirmation run passed both cases in 12,597 ms and 12,610 ms.
The header read is shared by the S and P frames, so the original
failure does not identify a cut stage or a child exit. It is distinct
from the earlier run that returned a refusal X. Keep qualification of
the paired passive case paused pending the baseline disposition.

## Bounded test-only changes

- Keep the two private child scenarios, current successful R/A/C,
  S/P/G frames and evidence, and all existing deadlines. Place the
  controller read failure next to its known case and expected frame.
  Report the fixed header, body, bound, tag, or deadline failure. On
  this failure path only, observe the owned child's status. On EOF
  or reset, allow the kernel to finish marking the child waitable by
  polling within the existing two-second `STEP`. For other failures
  do one nonblocking observation. Use Linux `waitid` with
  `WEXITED | WNOHANG | WNOWAIT` on this exact owned child. The
  launcher put it in its own process group and session before exec.
  A zero signal-info PID means the child is still running. If it is
  waitable after an exit, kill or dump, require both its process group
  and session IDs to equal its still-unreaped PID. Signal the private
  group before reaping the leader with `Child::try_wait`. A group or
  session mismatch reports failure and cleanup can signal only the
  leader. An interrupted or failed observation keeps the guard.
  Classify running, exit 1, fixture exit 86, test failure 101, signal,
  other exit, or observation failure. Keep the cleanup guard intact
  on observation error or timeout. After an actual reap, clear the
  guard. Never signal a numeric group after reaping its leader.
  Descendants that leave the private group are outside this diagnostic.
  The bounded post-failure poll does not extend an acceptance
  deadline. At S failure record once whether the stale Status socket
  is pending, closed, reset, delivered a byte, or had another I/O
  error. Never emit the byte.
- Reuse the bounded private log read on the failure path. Surface only
  known shutdown phase/category pairs and fixed child fixture refusal
  categories. Include a bounded missing, unreadable or truncated
  log state where relevant. Preserve X and its bound and map an
  unknown X body to a fixed unknown category. Give relevant bare
  child refusals in the post-Commit Status and fatal completion hooks a
  distinct fixed category. Emit the category as an existing structured
  diagnostic with fixed stage and category before attempting X.
  Neither status observation nor log read may panic before reporting.
  Print no arbitrary bytes, paths, credentials, or raw child status
  output. Logs are disposable, as before.
- To establish the first active response after Commit, explicitly open
  the same-UID Status IPC connection and use the existing framed send
  and a test-only scoped read of the existing framed reply. Keep
  the original deadline. Retry only when connect failed before sending
  with ConnectionRefused or NotFound, or when the decoded response
  is exactly a successful `activating` status. A send or response read
  failure may have reached the listener and fails immediately.
  A successful decoded `transport_degraded` is a distinct setup
  refusal. The exact second request stays the cut. In the child, once
  one active response was selected, a successful decoded degraded
  response before the intended cut emits the fixed
  `status_degraded_before_cut` refusal without displacing SQLite.
  Reuse the same bounded status classifier in the parent and child.
  Do not change production Status, dispatch, or shutdown behavior.

Test the classifier and the Status retry/refusal table, including
connect-not-ready, decoded activating and degraded, and send/read
failure without another request. Test malformed, lost and unknown-X
controller frames. Verify exit 1, 86, 101 and signal observations
with short-lived children. Test pre-reap group cleanup with a real
private descendant holding a pipe or socket open. Check closure
through the existing reader handle. Verify the fixed log fallback
when an X frame cannot be sent. Confirm the exact candidate
and stock builds with the existing strict scope.
Run the old two-cut ignored selector with the revised fixture and
matching stamped package three times serially, with new private profiles
and relays each run. Preserve per-case time and fixed categories. An
unexplained new failure blocks paired qualification. If the generic EOF
does not recur, retain its unknown cause and record that the new
diagnostics are prospective. Three clean runs validate the instrumented
fixture on those runs with no inferred flake-rate bound. The paired
phase can resume only with that explicit risk and disposition. A later
unexplained recurrence blocks it. Changing production behavior, widening a
deadline, running additional blind repetitions, or adding the paired
scenario belongs outside this diagnostic change.
