# Installed streaming daemon socket correction

Beads `marmish-8ka.5.5`; baseline
`78e39ee0301a58bbe2ed0da1187e58ef74f01ff0`. Design review is required before
implementation. This correction changes the build-only installed qualifier,
not the product socket contract, public installer pins or workflow budgets.

Candidate [42346198320](https://github.com/Epiphytic/marmish/actions/runs/42346198320)
passed all three complete native/package jobs and rendering. Its first installed
[stream matrix](https://github.com/Epiphytic/marmish/actions/runs/42350381517)
installed and prepared identities on all platforms, then failed at the
180-second readiness deadline before publishing any server-ready record. No
remote command is validated. Driver control flow places command execution after
local and all-server readiness, which were not reached. All server results
retain incomplete cleanup/stop proofs;
those flags cannot be treated as proof that a process survived or was stopped.

`installed_matrix::Daemon::start` passes and retains a caller-supplied `--socket`.
The product daemon binds `ProfileLease::socket_path()` instead; the CLI socket
override selects an existing IPC endpoint for clients. Streaming's generated
`ms-*` addresses are therefore unrelated to the listening daemon. Its children
also receive a private TMPDIR different from the parent driver, so resolving the
parent's default socket would remain wrong. A real locally installed candidate
reproduction observed `activating` at the child's standard socket, no socket at
the requested decoy, and a live owned child. SIGTERM returned exit zero with
engine/MDK/runtime stop reports true. This proves the address mismatch, not full
relay activation or cross-platform operation.

Correct and reuse the existing `installed_autoexit::socket` resolver, which
derives the profile socket from the private workspace manifest's checked 32-character
lowercase hexadecimal profile ID, effective UID and explicit child
`Environment.temporary`, with the product socket length bound. Expose it only
within installed-check modules, or relocate that same implementation to the
shared matrix module and keep its existing autoexit users. Streaming resolves
both prepared identities with this helper and passes those paths to the existing
owned Daemon. Remove streaming's unrelated random socket directory and its
cleanup. Do not change other finite qualification cohorts, daemon behavior,
relay readiness definition, PID/UID peer authentication or stop proof criteria.
Do not mutate process-global TMPDIR or trust remote artifact paths for authority.

Claude design round 1 identified an additional latent bug in that resolver:
`records::hexadecimal(id, 16)` checks sixteen characters, whereas the product's
`random_hex(16)` creates thirty-two characters. Correct its check to exactly 32
lowercase hexadecimal characters; reject 16-character IDs. Its existing
autoexit callers have no qualifying execution evidence for real profile IDs;
do not infer that they passed. Validate the local identity name and private
child temporary directory. Enforce the stricter product construction bound,
`runtime_path_bytes + 10 < 104`, before spawning either daemon, as well as the
socket's `<104` bound; use a distinct local path-length error. Resolve both
paths once, before spawn, and refuse equal sockets. Existing product runtime
directories remain owned by product/profile cleanup; the qualifier must not
unlink them separately. Stop-proof semantics remain unchanged.

Regression requirements: explicitly different parent and child temporary roots;
distinct prepared identities; exact UID/profile-ID socket construction; invalid
or traversal profile IDs refused; 16-character IDs refused; both adjacent
edges of the stricter runtime-path length bound. A mandatory bounded subprocess
regression must obtain a real product-generated profile ID under a TMPDIR
different from the parent, compare the resolver with the real product socket
path, and answer an actual PID/UID-authenticated Status request there. Use the
real daemon or product identity socket resolver, with owned child cleanup and
private state removal. Repeat the real installed smoke after the source fix,
preserving owned child cleanup and private state cleanup. Keep the failed workflow and
native candidate results in sanitized evidence. Run focused installed-feature
tests, strict native installed-checks Clippy, formatting and diff checks, then
actual Claude auto integrated code review. Publish reviewed source to both
remotes and qualify a fresh immutable candidate followed by all eight serial
installed cohorts. A Linux reproduction or review cannot qualify macOS or the
full client/server matrix.

The other finite matrix/benchmark/policy/recovery/lifecycle callers of shared
Daemon still pass decoy addresses. Track those separately; this bounded
streaming correction does not qualify or repair those cohorts.
