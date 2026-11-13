# Self-hosted CI capability preflight

2026-11-13; Beads `marmish-5z3`. **Incomplete: Phase A acceptance has not passed.**
This is capability inventory, not native product, installer, provider or transport
qualification. No GitHub-hosted VM was used.

Source: `f63888e770fbd65c3537e5d185dcd9316109bb4a`.
[Run 43384760030, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/43384760030)
was manually dispatched at 07:18:03 UTC using the serialized native matrix.

| Platform | Actual execution | Result |
| --- | --- | --- |
| Linux AMD64 | 07:18:07–07:18:25 UTC, 18 seconds | Capability probe and owned-container cleanup passed. |
| Linux ARM64 | Eligible 07:18:25; assigned 07:20:46 UTC | Stalled in runner startup; checkout and probe never started. Cancelled. |
| macOS ARM64 | Never eligible behind ARM64 | Cancelled without execution; no capability evidence. |

AMD64 ran nonroot on the self-hosted pool. It reported Python 3.14.4, Git 2.53.0,
curl 8.18.0, C compiler 15.2.0, make 4.4.1, Perl 5.40.1 and Docker 29.9.0.
Archive tools, readelf, D-Bus and setpriv were available. Host Node and GNOME
Keyring were missing. Host glibc was 2.43; the pinned rootless Ubuntu24 container
passed the native/nonroot smoke with glibc 2.39. Cleanup proved that exact probe
container absent; downloaded image layers were retained explicitly.

ARM64 obtained a runner within five minutes of eligibility, but made no progress
past `Set up runner`. The operator reported the stall and requested normal
cancellation at approximately 07:25:04 UTC. This cancelled the pending macOS
job, but ARM64 startup remained in progress. A subsequent force-cancel completed
the run at 07:30:05 UTC, with ARM64 marked cancelled at 07:30:04. Its step metadata
still showed runner setup in progress and checkout/probe pending. The job-log
endpoint returned HTTP 404 both before and after cancellation. The cause is
unknown; cancellation in GitHub does not prove underlying runner lease/process
cleanup. No subsequent build was dispatched against that uncertain slot.

The visible pool snapshot before dispatch found no in-progress runs across 55
accessible organization repositories; 28 queued runs exposed no unfinished
eligible jobs. This was a limited snapshot, not a capacity reservation or a
statement about inaccessible repositories. Configuration provides two execution
slots: Bel AMD64 and a shared laptop macOS/ARM64 slot. Listener count does not
increase that capacity.

Local checks before publication passed: nine probe/policy tests (including 24
policy mutations), fourteen existing release-automation tests, shell syntax,
actionlint and diff checks. Actual Claude design and integrated code approvals
are recorded in the linked [phase plan](../phases/self-hosted-ci.md) and its
review records. These checks do not replace the missing native runs.

The CI migration remains unfinished. macOS default-auto provider qualification
still needs a reviewed solution for privileged disposable-account provisioning;
the current workers have no host sudo. Full nine-pair live streaming also needs
independent macOS and Linux ARM64 capacity. Existing hosted workflow definitions
have not yet been migrated. The release publisher remains disabled.

Source was pushed to GitHub and Radicle (two seeds synchronized). Iris external
readback timed out after 12 seconds with no bytes, so Iris availability was not
verified.
