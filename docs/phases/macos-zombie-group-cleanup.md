# Darwin reserved process-group cleanup

Native diagnostic [run 41394907662](https://github.com/Epiphytic/marmish/actions/runs/41394907662)
reports `group_signal_failed`, `permission_denied`, `exit_confirmed` for its
owned `/usr/bin/false` preflight. This precedes the confinement probe.
Apple's [XNU killpg1 implementation](https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/kern/kern_sig.c)
excludes zombies from the group iteration and returns EPERM when no eligible
member is found. This explains the observation, but EPERM alone never proves
absence of live descendants.

Refine only installed harness `_finish_child` and owned diagnostic `run_private`.
Keep the exact leader identity reserved and unreaped throughout group signaling.
On Darwin only, an EPERM from group signaling may be deferred when two WNOWAIT
observations confirm the same exited leader. Both observations must match on
si_pid, si_code and si_status, si_pid must equal child.pid, and child.returncode
must still be None. For this confirmed zombie, parent waitid ownership and its
unreaped PID replace process_identity/getpgid lookups, which Darwin may reject for
zombies. The original child was created with start_new_session=True: a session
leader cannot change its process group. Live-leader identity and getpgid checks
remain unchanged; an unconfirmed leader never receives this exception.
Every other signaling error remains fatal. Never defer an observation
error or EPERM while the leader is live. Diagnostic confirmation must retain its
existing expected exit-status equality check.

After the final group signal and successful leader wait, require a bounded
two-second group-absence proof: only ESRCH from killpg(pgid, 0) establishes absence;
success or EPERM means the group is still present/unknown and cannot pass. Do not
send another nonzero signal after reaping: the numeric PGID may have been reused.
Persistent groups retain private state and fail cleanup. A deferred permission
failure is accepted only after this complete proof. Add this same absence check
to the diagnostic; currently it stops after waiting for the leader.

This changes no confinement profile, command policy, transport authentication,
daemon worker cleanup or production application lifecycle. It fixes a harness
platform mismatch while requiring evidence that surviving descendants are absent.

Tests inject Darwin zombie-only EPERM followed by ESRCH (pass); live-leader EPERM,
unconfirmed/reaped/mismatched leader, wrong errno/platform, surviving group,
post-reap EPERM and leader wait failure (all fail). Assert no nonzero signals
after reaping and exact expected status preservation. Existing Linux reserved
leader and diagnostic privacy tests must pass. Claude reviews this design before
implementation and the integrated code afterward. Dispatch the existing hosted
macOS diagnostic and installed prerequisite workflows after reviewed publication;
record actual results without treating static/unit passes as native acceptance.

The initial Claude design review rejected an unspecified Darwin zombie identity
lookup; the parent-owned repeated WNOWAIT contract above resolves that finding.
The diagnostic adds finite `group_survived` cleanup failure for an absence proof
that expires (permission_denied for a final EPERM, otherwise none while present).
When can_signal is false, no post-reap group probe runs: retain observation_failed.
Do not change the existing expected exit-status comparison or conceal its failure.
Tests must model process_identity/getpgid rejecting a zombie, plus the unconfirmed
case failing closed. Do not catch EACCES under the EPERM-only exception.
