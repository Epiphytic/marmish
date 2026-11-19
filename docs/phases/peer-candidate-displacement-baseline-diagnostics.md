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
  this failure path only, check the owned child's status once without
  waiting. Classify running, exit 1, fixture exit 86, test failure 101,
  signal, or other. If `try_wait` reaps the child, remove it from the
  existing cleanup guard so it cannot later signal a reused PID.
- Reuse the bounded private log read on the failure path. Surface only
  known shutdown phase/category pairs and fixed child fixture refusal
  categories. Preserve X and its bound. Give relevant bare child
  refusals in the post-Commit Status and fatal completion hooks a
  distinct fixed category. Emit the category to the private child log
  as well as attempting X, so failure to send X does not erase evidence.
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

Test the classifier, malformed or lost controller frames, and status
diagnostic classification locally in the candidate libtest. Confirm
both exact candidates and stock build with the existing strict scope.
Run the old two-cut ignored selector with the revised fixture and
matching stamped package three times serially, with new private profiles
and relays each run. Preserve per-case time and fixed categories. An
unexplained new failure blocks paired qualification. If the generic EOF
does not recur, retain its unknown cause and state that the new
diagnostics are prospective. Changing production behavior, widening a
deadline, running additional blind repetitions, or adding the paired
scenario belongs outside this diagnostic change.
