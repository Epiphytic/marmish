# Native interactive terminals and peer operations

Beads: `marmish-dwt`. [High-level design review](../reviews/interactive-peers-roadmap.md).
This roadmap targets native Rust and upstream MDK.
The existing command, stream, rsync and copy modes retain their behavior.
CLI examples below describe proposed interfaces. Each runtime phase needs its
own detailed design and Claude review before implementation.

| Increment | Depends on | New room purpose |
| --- | --- | --- |
| Lossless PTY attach and writer fencing | Native transport and ephemeral MDK route proof | Terminal |
| Reattach, observe and takeover | Lossless attach | Terminal |
| Screen diff evaluation | Lossless baseline and reattach | Terminal |
| Identifier onboarding and server invitations | Existing native setup and transport | Peer |
| Heartbeats and permission views | Accepted Peer rooms | Peer |

## 1. Interactive terminal and session ownership

Add explicit `marmish terminal <npub-or-alias>[@relay]`. Require a local
terminal and an explicit, revocable, default-off per-key grant for interactive
shell access stored in server configuration. Ordinary
command allowlists, including broad executable patterns, must never
implicitly grant a shell. An interactive shell runs with the OS permissions
of the daemon's nonroot execution user; it cannot enforce an executable allowlist
for commands typed into it. Keep the existing account model. Give Marmish a
separate tmux socket and session namespace in an owner-checked private
`0700` directory with symlink-safe creation and validation. Start tmux and
its shell with an explicit minimal environment and reviewed configuration.
Keep tmux optional for existing
noninteractive deployments. Report a missing tmux dependency before session
admission and use a trusted server-side executable path.

Use a versioned two-member Terminal room purpose. Bind it to the expected
accounts, MLS authority and a server-generated session ID. Use the existing
authenticated transport rather than parsing terminal bytes as commands.
Default to one persistent session per server npub, client npub and execution
user. Explicit `--new` creates a fresh tmux session and MLS group. Attach to
the existing session on reconnect; when it is occupied, offer observe, take
over or start a new session. A single writer owns input and resize. Takeover
increments a server-issued generation, so delayed input from the former
writer cannot reach tmux. Observers receive output only and cannot resize,
reply to terminal queries or signal the process. Keep the tmux session
running on disconnect. Clients send keepalives every five minutes by default.
After eleven minutes without authenticated activity, expire the attachment
lease and release the writer slot. Another live observer never renews the
writer's lease. Measure expiry on the server's monotonic clock.

Start with frontends attached through the same identity-owning client daemon.
Authenticate each attachment separately and check its generation where bytes
are written to the PTY, including in queued work. Bind privileges again on
reattach and revoke active attachments when the grant is removed. A second
device with the same Nostr secret does not share the first device's MLS state.
Cross-device attachments require a separately reviewed endpoint and MLS design.
The existing rejection of a third leaf, even for an admitted key, stays in force.
Revocation fences input immediately, detaches all associated attachments and
terminates the affected owned tmux sessions. Document the limits for
processes that deliberately detach from those sessions.

Start with lossless, sequenced PTY bytes and bounded input/output batching,
with prompt flushes for input echo and a maximum latency under continuous
traffic. Send resize and teardown promptly. Use a dedicated tmux attachment
PTY. Restore the local terminal on detach, error and handled signals, and
refresh the current screen on reattach without replaying unknown raw bytes.
Keep PTY input and output out of Marmish's durable application queues, journals
and logs. Before coding, trace the pinned MDK path and prove a usable ephemeral
payload route that preserves authenticated binding, replay protection and the
persistence needed for MLS state. If the existing path persists terminal payload,
resolve the gap in a reviewed design before using it. Relays can retain encrypted
wire events; bound the expiry as transport permits and distinguish that from
local storage. Tmux has its own configured scrollback. Bound frames,
dimensions, queues, detached sessions and replay state. Gate remote terminal escape sequences
with side effects such as clipboard writes. Keep terminal query replies at
the controlling endpoint. Test disconnects, duplicate/reordered frames,
raw-mode restoration, resize, Ctrl-C, full-screen applications, shell
authorization and ordinary-stream regression.

If the pinned upstream MDK API cannot support the required route, consider an
upstream contribution or a separately reviewed transport design that preserves
the Marmot trust boundary. Keep the project on upstream MDK.

Then evaluate the VT screen diff and redraw throttling in
[Claude Chill](https://github.com/davidbeesley/claude-chill) at source
`66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9` (MIT). Reuse a narrow component
where its contract fits and retain the upstream license and copyright notice
for copied code. The last-redraw-wins helper is suitable only for updates
independently reconstructable from the client's confirmed screen state.
Never apply it to raw terminal bytes or dependent deltas. Negotiate any
optimized mode, bound snapshot/history sizes, and resync on gaps, resize and
attach. Keep a lossless mode and prove cursor, alternate screen, paste,
Unicode, colors and terminal modes remain correct. Measure key-to-echo and
first-render latency, redraw event and byte counts, and scrolling throughput
against the same raw PTY workloads. Prefer simpler batching if diffs buy
little or break terminal semantics.

## 2. Identifier onboarding and server invitations

Support explicit profile publication from a client identity. Start with
[NIP-05](https://github.com/nostr-protocol/nips/blob/master/05.md) for
email-shaped lookup backed by a domain's HTTPS mapping and matching signed
kind-0 metadata. A real email address may also be published as an opt-in
contact field, with a clear preview that relays and readers can copy it.
Names and self-attested contact fields are display hints. They cannot serve
as a global email directory. Preserve existing profile fields when updating
metadata and surface conflicts for an identity shared with another app.

Allow a server to resolve operator-configured identifiers during foreground
setup or startup, or during explicit refresh. A successful lookup yields only
a candidate. DNS and HTTPS lookups stay off the admission path. Show the
complete candidate npub, source and verification
status. Require the operator to compare it with the intended peer through
an independent channel and record explicit confirmation before installation.
A generic confirmation bypass such as `--yes` cannot add the key. Persist the
canonical key in the existing authorization configuration. Subsequent
daemon starts use the pinned npub. If a refresh detects a changed mapping,
show the mismatch and require explicit replacement. A headless start cannot
add an unresolved key or replace an authorized key. Keep existing grants
intact on lookup failure. Reject malformed or conflicting answers. Resolve
only operator-provided domains, bound HTTPS and relay lookup, ignore
redirects and unconfigured relay hints, and preserve Tor routing policy.
If HTTPS discovery cannot obey the required Tor route, fail the lookup.
Explicit npub onboarding must remain usable without directory access.

Add a client policy for inbound server invitations, with manual approval
as the default and explicit opt-in to automatic acceptance. Use a dedicated
two-member Peer room rather than reinterpret the existing Command room's
creator as a server. Verify Welcome publisher, founder, exact membership,
roles and room purpose before recording the server. Keep a small separate
budget for unknown-inviter work and specify before coding whether MDK can
retain pending Welcomes without joining or needs quarantined staging. Budget
that staging before processing untrusted MLS data. Manual approval and
opt-in automatic acceptance both grant only the peer channel. Never add
discovered peers to command trust or execution allowlists. The Peer room
cannot be promoted to Command or Terminal. Bound pending and accepted rooms,
rate-limit unknown inviters, deduplicate
replays and support blocking and forgetting a server. A server can invite
only an explicitly authorized client with a usable KeyPackage. Unavailable
clients stay pending. Provide a local server list showing canonical npub,
optional alias and invitation state. Keep known, pending and blocked
distinct from current reachability.

## 3. Private heartbeat, permission views and client history

Use the accepted two-member Peer room for status. Send an initial update
when the peer channel becomes ready, then a configurable periodic heartbeat
defaulting to one hour, with bounded retries and jitter. Bind status to
the authenticated server and current group authority. Track the last fresh
heartbeat separately from local receipt and last successful command or
terminal admission. Report fresh, stale or unknown based on the advertised
interval within local limits. Handle clock skew, replay, duplicates,
out-of-order delivery and restart. A missed heartbeat is evidence of
staleness, since a sleeping client or unavailable relay can miss updates.

Send status only to currently authorized peers. Provide server controls for
field visibility and heartbeat disablement. The status schema includes
effective username, host uptime, load averages, disk usage for configured
filesystems and selected local interface addresses. Address sharing defaults
off. Refuse address sharing when Tor is forced or the peer route uses onion
relays, including per-key overrides. The other host details stay inside encrypted
peer rooms. Avoid external address probes, full mount listings and broad
interface broadcasts. Samples include timestamps and bounded retention in
the existing encrypted journal. A peer revoked locally stops receiving new
updates. Already delivered data remains under the recipient's control.
Relay observers may correlate the timing of periodic encrypted traffic.

Proposed client commands are `marmish servers list` and
`marmish servers show <npub-or-alias>`. Show last seen, sample age, host
status and the server's effective permission description for the requesting
key. Derive it from the same additive default/per-key policies used at
admission, showing intersected allow layers and the union of denies, with
wildcard semantics and a separate interactive-shell grant.
Label dynamic PATH/policy effects and stale snapshots. Enforce permissions
on the server for every admission, independent of the description.
Proposed server commands are `marmishd clients list` and
`marmishd clients show <npub>`. Include configured grants, effective
policy, pending invitation state and last authenticated connection or
successful admission. An allowlisted client that has never connected
appears as such. Use one local read API with same-user access while the
daemon owns the profile. Preserve the existing single-writer state model.
Version new journal state, prefer additive migrations from supported profiles
and document what must be backed up for rollback. Older binaries must reject
an unsupported newer schema cleanly.

## 4. Qualification and distribution

Use disposable identities and a bounded relay fixture, with installed Rust
clients and servers exercising real Marmot messages. Verify mixed-version
behavior, old command and file operations, restart, grants and revocation,
metadata mismatch and headless startup, invite abuse and status confidentiality.
Include old-peer refusal of the new room purposes without blocking the old
peer's existing operations. Set numeric latency and resource bounds during
detailed design. Retain the existing installed matrix and its regression gates.
Test Linux AMD64, Linux ARM64 and macOS ARM64 in native roles on the
self-hosted pool. Record the actual platform pairs exercised and keep any
unavailable pairs pending. Retain numeric terminal measurements and limits.
Review and publish each completed increment under the repository's phased
workflow. This roadmap does not authorize a release or version bump.
