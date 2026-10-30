# Managed runtime completion: integration Trial14

The [sanitized report](managed-runtime-linux-amd64-trial14.json) preserves this
failed integration run rather than replacing the passing historical Trial13.
All three real completion cases passed after review hardening: interrupted uv
extraction and fresh retry, post-rehash substitution rejection, and hostile
ambient/cwd isolation. All three namespace diagnostic stages passed. Owned
cleanup completed and monitored home directories remained unchanged.

The managed full tools suite failed: 461 tests, 7 reported skips, exit 1. Two
existing workflow assertions still expected no manual dispatch and exactly two
checkout steps; the concurrent approved self-hosted proof job adds a guarded
manual entry and a third checkout. These are integration failures requiring
updated trust-boundary assertions and a new full run. They are not waived, and
this run did not reach offline retention validation.

Focused completion tests passed 51 cases normally and with symlinked TMPDIR.
No production installer changes or native-platform acceptance are claimed.
