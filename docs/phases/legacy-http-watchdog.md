# Legacy HTTP worker cleanup

Bead `marmish-d93.4`; base `28db4a1b96ba3e7e6b7137bd582a1a8ff34a758e`.
This phase changes only optional legacy rendezvous tooling and its tests.
Native runtime, qualification gates and workflow source remain unchanged.

Actual confidential run `42646816080/1` at source `7ac1a2c` had exactly one
error: the real Watchdog success test raised `PermissionError`, errno 1,
while sending SIGKILL to its unreaped worker group. The suite ran 1,003 tests
in 52.101 seconds with 45 skips. It was not a request timeout. The denied
signal skipped reaping and pipe cleanup. The precise kernel reason was not
independently observed; EPERM itself proves neither exit nor absence.

## Approved design before implementation

Claude Opus 5.5, session `abbf7db9-0da4-4837-ab7d-29980b625a30`,
approved design SHA-256
`922090764ae4c2e65dabf7502709d8162e3fdeb8f7286be7e045d5b8b40fe20f`.
Review evidence SHA-256
`85040b5c92a91c2972015dd8d485b94a830fc676226824ef6c7b3e3ff03e6957`
records **APPROVE**, no blockers. Implementation starts after this record.

Extract private worker cleanup, preserving the unreaped Popen reservation.
On Darwin-only SIGKILL EPERM, lazily reuse installed_state._confirmed_exit:
two matching WNOWAIT observations must prove the same unreaped direct child
exited. Compare that status with bounded wait. All cleanup uncertainty maps
to `artifact_worker_cleanup_failed`; EPERM is never accepted as absence.
Always attempt the existing two-second wait, and close selector/pipes even
when signaling or proof fails. After reap send only signal 0; accept only
ESRCH. Darwin gets a bounded two-second absence-observation window; persistent
EPERM or a surviving group fails. Linux retains immediate strict absence.

Request deadline, selector cadence, output cap, fixed worker, sanitized env
and stdin-only credentials remain unchanged. Keep all three original
Watchdog tests and their bounds/assertions. Capture a real own-loopback
simulated-Darwin EPERM regression before correction; it must fail on the
actual old cleanup, then pass after the change. Add focused fake-child proof,
errno, status, timeout and absence tests without real signals. Run only the
explicit rendezvous module and existing state DarwinZombieCleanup class
under pinned managed Python, no full discovery/Cargo/public network/namespaces.
Obtain independent completed-code review. Linux simulations cannot establish
native Darwin correctness: fresh hosted macOS validation remains required.

## Completed increment

Implementation and independent completed-code review are complete; see
[review and validation evidence](../reviews/legacy-http-watchdog.md).
The own-loopback simulated-Darwin regression demonstrated RED then GREEN;
29 focused tests passed with zero skips, independently repeated by Claude.
Native hosted macOS validation remains pending. This increment does not
qualify any native release gate or establish the underlying EPERM cause.
