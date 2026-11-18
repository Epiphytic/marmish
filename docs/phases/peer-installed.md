# Installed Peer enrollment and status

Marmish clients can opt in to receiving Peer invitations and see the servers
that have connected. Server operators can invite their execution clients and
see enrollment and connection history. Peer membership gives the two
principals a private status channel. Command and Terminal authority still
comes from the current server configuration.

The native manifest tracks upstream MDK master, with a reproducible
resolution in Cargo.lock. The [MDK refresh](../reviews/mdk-master-private-wss-refresh.md)
resolved `59b7ed3ff007494b25f8404c144c1d82809b3973`. Its public account opener
uses `rusqlite::Connection::open(path)` through `open_account_encrypted` and
has no named VFS option. `open_hardened_sqlcipher` applies hardening to an
already opened connection and cannot construct a native account from it.
The locked build retains the existing bundled SQLCipher patch.
The [private WSS history proof](../validation/peer-private-wss-activation-history.md)
demonstrates native convergence on a local relay. It does not qualify the
installed owner, deployed transport, or end-to-end acceptance and status.

## Profile, startup and production boundary

Add a closed optional `peer` section to the owner-checked configuration in
`command_config.rs`. Keep intent in the profile configuration and per-offer
admission, decisions, replay fences and membership in SQLCipher. Reuse the
existing profile identity and client/server/both role. A client role can
receive and a server role can invite. Both uses separately fenced
directional owners under one lease. Each Peer membership has its own native
account and store, separate from Command.

1. Take the profile lease, load and validate configuration and resolve the
   identity. If Peer is absent or Off, create no Peer owner, path, database,
   queue, socket, subscription or VFS registration. Existing setup does not
   enable Peer.
2. With Peer enabled for this role, verify runtime platform eligibility and
   current local relay qualification. A default-VFS dispatcher must mount
   before the first SQLite open in that process. The quota increment must
   review that ordering and its failure behavior before installing it. Start
   ordinary services and the independent Peer owner under the reviewed
   ordering. A Peer owner still initializing never delays ordinary daemon
   readiness.
3. Report `disabled`, `starting`, `ready` or `unavailable` through local
   Peer inventory and daemon status. Unavailable codes are `role`,
   `platform_unqualified`, `relay_unqualified`, `relay_expired`,
   `relay_stale`, `routing_unsupported`, `clock_untrusted`,
   `quota_unavailable`, `storage_unavailable` and
   `transport_unavailable`. Retry transient errors with bounded backoff.
   Unknown expiry, invalid code/configuration combinations and corrupt state
   grant no authority. Reject malformed settings in foreground validation.
   An unavailable Peer keeps ordinary Command and Terminal operational.

At 7457246 the shipping invitation module is dormant behind the scoped
`allow(dead_code)` in `lib.rs`. The code compiles paths, raw socket and
store cores without providing a production origin or listener. Selectively
promote reachable code under independently reviewed increments:

| Current seam | Installed boundary |
| --- | --- |
| `peer_invitation.rs`, paths, raw socket owner/wire/group, cursor store and consent checkpoint | Reuse the existing scoped cores. Add a private `QualifiedOrigin` constructor from validated production relay evidence. Keep all relay fixture constructors test-only. Remove the dormant allowance when the owner is reachable. |
| `peer_daemon_worker.rs`, daemon checkpoint/decision, `peer_pending_ipc.rs`, `daemon.rs` | Split out the reusable role, budget, scheduling, projection, UID and exact-decision core. Mount only a production bootstrap with leased identity and system time. Keep `PeerFixture`, fixture connector selection, fault switches and `run_peer_fixture` test-only. Send bounded Peer local IPC only to the same leased owner. |
| Prepared Consent owner/ingress, custody checkpoint/schema, stage binding/joined/native/retirement and terminal retirement | Promote checked owners and opaque native and disk witnesses after their M1–M5 and terminal gates close. Preserve current compare-and-swap and descriptor checks. Keep disposable stage and retirement fixtures test-only. |
| `peer_postjoin_gate.rs`, `peer_postjoin_verify.rs`, receipt verification/publication/private relay proof | Keep capture-only gates and proof harnesses test-only. Extract only the final sealed verification, bounded exact signed publication and transaction inputs actually used by installed enrollment after full M5 approval. |
| Native leave proof and quota VFS/dispatcher | Keep proof drivers test-only. Add production apertures only for the qualified quota and the reviewed, terminal-only, exact leave path. |
| Heartbeat wire, planner, sampler, send fences and receiver projection | Reuse the validated cores under the accepted owner after its send and receive transactions qualify. Keep fixture `AcceptedStatus` constructors and host-sampling proofs test-only. |

Production constructors cannot take injected identity, certificate chain,
clock, or fault hooks. TLS trust is validated production configuration.
No Cargo feature, including `installed-checks`, can expose Peer fixtures
to a release build. Exercise the non-test entry points. Add a compile-fail
check for fixture authority on the release build and scan release/link
artifacts for fixture entry points and fault switches. Do not promote
any global VFS registration as a prerequisite of ordinary daemon startup.
An approved path-scoped quota must protect actual database and journal
extents while stock paths delegate ordinarily. Use a named VFS if supported
through the upstream public opener. Consider the
[dispatcher design](peer-quota-dispatch.md) only after exact reviewed proof
on each enabled platform. Preserve its separate 16 MiB closed database
check and 128 MiB combined native admission. A hot, over-cap or unverified
stage remains quarantined and charged until authorized reconciliation.

## Private relay qualification

Use one exact canonical direct `wss` origin, with a normalized host and
default port, and no user info, query, fragment or application path.
Confine each signed Peer route and usable endpoints to this origin. Never
take a route from a candidate identifier or remote invitation. Onion,
Tor-required, mixed, or ambiguous routing is unavailable in this scope.
There is no default, SRV discovery or fallback for Peer.

Configure either ordinary hostname and chain validation plus an exact
`spki-sha256` pin, or an explicitly supplied private root pinned by
`ca-sha256` with hostname/chain validation. Provide the owner-only
`marmish peer relay qualify` foreground probe with that exact endpoint and
trust policy. Require an explicit operator-approved relay and bounded
test window. Use fresh approved disposable keys, a third account for
negative checks, harmless application records, and fixed time, record
count and byte budgets. Verify WSS/NIP-42, recipient and group read/write
isolation, EOSE, history after reconnect, required retention and ingress
caps, refusal of wrong-route and over-limit records, and authenticated
writer scope. The [live Peer](peer-live.md) relay contract defines the
bounds. A short reconnect probe cannot establish long retention. Record
separate relay-side enforcement and duration evidence for guarantees the
local probe cannot observe in its window. Unsupported probes leave the
destination unqualified.

On success write an atomic owner-checked qualification record for canonical
origin, exact trust policy/digest, probe suite and bounds version, resolved
MDK revision, OS/architecture, trusted time and expiry no more than 30 days
later. Reject stale, cross-origin, wrong-platform and clock-rollback records.
A changed trust pin, suite, MDK or relay security contract requires
requalification. The daemon checks qualification at startup and before
authoritative network work. It validates TLS normally on every connect.
It does not probe with the user's identity. `marmish peer relay show` reads
the local record and shows the origin, abbreviated trust fingerprint, suite
version and time, and `unqualified`, `qualified`, `expired` or `stale`.
Relay-local qualification belongs to each installed profile. Platform enable
eligibility comes from the artifact's reviewed support scope and requires
production-compatible native/quota evidence for that OS and architecture.
Unverified platforms refuse enablement. The external operator and endpoint
remain outstanding and are unnecessary for local deterministic work.

## Admission and decisions

**Received invitations join before the user's decision.** After exact signed
Offer verification and before KeyPackage or stage work, require eligible
role and receive mode, check the canonical founder's local block state,
enforce the per-founder and profile limits and durable rate charge, reserve
the reviewed stage budget and recheck expiry/replay. Use the existing fixed
known/unknown reservation classes, one retained generation per founder,
eight Offer rows per class and 600-second new reservation floor per class.
The existing signed/cursor fences and resource checks still apply on
recovery. Reject policy refusals or invalid flights without a reply or
KeyPackage consumption. Budget/backoff deferrals do not consume a package.
Before consumption recheck all current bindings and the applicable signed
flight deadline against the authenticated Delivery, package and qualified
route. A valid exact Delivery follows the Consent and package bounds. Verify durable joined custody in the
isolated account before exposing Pending. Show the founder's full npub and
`joined, no authority`. The enable help discloses that joins and departures
can reveal availability and Peer enablement to the founder even on decline.

This supersedes the pre-Join user approval in [staged inspection](peer-staged-inspection.md)
and the [invitations plan](peer-invitations.md). Keep the
[join custody](peer-pending-join-custody.md) proof and pending fence.

Manual is the default when receiving is explicitly enabled. Automatic
requires either an allow-list of exact founder principals or the distinct
`--any-founder` opt-in with a warning and a bounded durable rate. Use
the existing durable 600-second floor at new reservation in each of the
two classes. This permits at most 12 new Automatic reservations per
profile in any half-open hour, even across restart. The same exact
generation does not spend a new slot. Manual rows retain their captured mode across configuration
changes and require a Manual decision. In allow-list mode, an unlisted
founder may enter Manual Pending through the same admission limits and needs
an explicit Manual decision. Automatic uses the same checked transition
after join, current mode and local policy. Denylist, expiry,
duplicates, competing live generations and failed proofs still fence work.
Receiving has no reverse Command-grant requirement.

Pending approval advances through exact Activating and the independently
verified [PostJoin/receipt history](peer-postjoin-private-wss.md). Only
the sealed authority at each side can open status. Recipient decline or
expiry fences first, then uses the independently qualified
[native leave](peer-native-leave.md) only for the eligible terminal scope.
If removal cannot be proven, retire locally when safe and show
`removal not verified; local state retired` after retirement. Preserve a
charged record when verified retirement cannot yet finish.

The server operator initiates invites only for current execution clients.
Identifier resolution reuses `marmish setup --grant-identifier` and the
[existing onboarding](nip05-onboarding.md) resolver and atomic writer.
Show lookup source domain and complete canonical lowercase npub. Require
exact confirmation, rejecting abbreviations. An explicit startup preflight
can resolve a supplied identifier before launching the daemon, saving
authorization only after confirmation. Without a TTY require the exact
expected npub. The daemon itself uses saved principals and never resolves
identifiers. Publish a public contact identifier only on explicit client
request. A changed mapping requires deliberate rotation and fresh
confirmation, preserving unrelated grants.

## Local operator controls

Reuse versioned local UID-bound IPC, exact selectors and bounded paging
from [client inventory](local-client-inventory.md). Read-only requests have
no network effect. Absent authority or a stale selection yields typed
readback, without changing a newer row. No generic confirmation override.

| Command | Effect |
| --- | --- |
| `marmish peer enable [--manual]`, `--auto --founder <npub>` (repeatable) or `--auto --any-founder` | Atomically chooses receive intent for client/both. Reject bare `--auto`, a combination of both Auto policies and unsupported roles/platforms. Use `marmish peer enable --originate` to enable founder invitations on server/both. On both, combine it with one explicit receiving form to enable both directions. |
| `marmish peer disable` | Atomically turns Peer off under the owner stop fence. Prints that encrypted state and membership remain. |
| `marmish peer relay show\|qualify` | Display local qualification or attempt the explicit probe. |
| `marmish servers list\|show` | List canonical founders and separate enrollment, received heartbeat and reachability observations. Show can select Pending Offer and display full selection. |
| `marmish peer accept\|decline <offer-id> --npub <npub> --revision <revision> --generation <generation>` | Exact current Pending decision. Prompt with complete npub interactively. Headless automation supplies matching `--confirm-npub` plus the full selection. Replay reads proven same-generation disposition. |
| `marmish peer block\|unblock <npub>` | Block future recipient work and fence current authority in that direction. Unblock never reopens a revoked/declined generation. |
| `marmish peer forget`, `marmishd clients forget` | Forget bounded terminal display metadata with exact selector after validated cleanup. Preserve durable cooldown, replay fence and block. |
| `marmishd clients invite (<npub> \| --identifier <name@domain>) [--confirm-npub <npub>]` | Invite an existing execution client. Headless requires exact confirmed npub. |
| `marmishd clients list\|show`, `marmishd clients revoke <npub>`, `marmishd clients rotate <old-npub> --identifier <name@domain> [--confirm-npub <new-npub>]` | List current permissions and directional history. Revoke the exact old grant and fence its Peer scope. Rotate after verification by revoking the old grant, confirming and granting the replacement with explicitly selected policy, then inviting the replacement. Existing Command grant changes go through the established config writer. |
| `marmishd status config` | Configure status opt-in, interval, field selection and per-key visibility locally. Bare `marmishd status` keeps its existing behavior. |

A founder-side revoked execution grant immediately fences new commands under
ordinary controls and invalidates the affected Peer authorization revision.
Serialize the effective config change with the same owner fence used by
status publication. If a config change takes effect on restart, stop/fence
the existing owner before confirming it. Stop new status, enrollment and
recovery publication for that direction
before scheduling any removal. Revoke and rotate output states that the
action fences Peer authority and the old leaf remains. Recipient block independently fences local
Peer authority and does not change founder-side Command grants. Do not
reopen either accepted generation on unblock or grant restoration. A new
verified offer is required. Founder removal and retirement of accepted
recipient memberships require separate bounded terminal designs with an
exact native witness and typed Unconfirmed on unresolved removal. The
Pending decline/expiry leave phase does not authorize those effects.
Data delivered before revocation remains available to the recipient.

## Heartbeat and inventory

Status is a separate opt-in. Follow the independently reviewed
[heartbeat activation](peer-heartbeat-activation.md) defaults unchanged.
On eligibility send an initial sample and then every 3,600 seconds by
default, with 300–86,400 second bounds, jitter and replay-safe freshness.
Defaults once enabled are effective username, uptime, 1/5/15-minute load
and per-client permission summary. Disk needs explicitly configured
paths/labels. IP needs selected interfaces/families and a complete
address-safe direct route proof. Tor, onion, mixed and ambiguous routes
suppress IP. Unknown data remains distinct from zero.

The server checks its current execution grant, exact accepted
authorization, native epoch, leaf, route and generation on every send.
The founder waits for the durable acceptance receipt. The recipient waits
for verified active native authority. Keep sequencing, send fences and
receive projection durable under the same accepted owner. Encrypted
permission summaries may include the exact validated allow/deny patterns
for that client only, preserving absent versus empty allow sets,
intersected allow layers, additive denies and wildcards. Per-key sharing
can narrow fields and disable permission summaries. Show Terminal
eligibility separately. Permission summaries are advisory. Public
profiles, diagnostic logs, CI and benchmarks contain no private metrics,
addresses, contact data or permission patterns. Bound and escape all
peer-controlled display strings.

Keep server-side current execution grants, last qualified Command
connection, Peer enrollment and last Peer contact separate. Include
eligible clients with no history. The client lists enrollment and the last
verified heartbeat with explicit freshness. Accepted state without a
fresh heartbeat never implies reachability.

## Recovery and qualification

On stop/disable fence scheduling and local mutations before releasing the
profile lease. Disable does not issue leave. Re-enable rechecks current
authority, exact route, generations, clocks and file/native evidence.
Resume only exact authorized publications that have a reviewed durable
retry proof. Leave interrupted data charged until its authorized cleanup
finishes. For uninstall, document explicit local purge from a stopped,
verified profile after safe cleanup. Purge does not imply remote removal.
A Command-only rollback uses a compatible saved config with Peer disabled
and exclusive profile ownership. Never restore one side of the native and
metadata binding or resurrect a revoked principal.

1. Refresh MDK and compose reviewed custody, M5, quota and terminal
   prerequisites. Require exact production-compatible native/quota
   evidence per platform before enabling. Installed owner and egress start
   only after independently approving their promotion design and relevant
   prerequisites. A disabled configuration parser, identifier preflight,
   redacted local inventory and display tests can proceed independently.
2. Run deterministic core checks for no Peer files/sockets/VFS when
   disabled, role and unavailable categories, stale decisions, bounded
   admission, profile isolation, policy/revoke and ordinary behavior.
   Use opt-in local live cohorts for real SQLCipher rollback, hot journal
   and exact stage ownership, private WSS and native enrollment, custody
   faults and interrupted startup.
3. Qualify the agreed deployed private relay. On installed artifacts verify
   setup/installer startup, manual and automatic join, decline/expiry,
   authority-free Pending, receipt-gated status, asymmetric revocation,
   Command/Terminal coexistence and published stats and permissions.
   Kill across Join, Pending decision, uncertain publication, receipt and
   status commit. Recover with no in-memory witnesses. If endpoint or
   platform evidence is missing, limit the artifact and report the gap.
4. For live qualification use the already proposed protected exact-SHA
   [cross-platform route](interactive-peers-cross-platform-qualification.md):
   `epiphytic-ci` Linux AMD64 `bel-ci`, and Linux ARM64/macOS ARM64
   `laptop-ci` sharing one slot. Require administrator confirmation of
   trust/admission and live capacity, installed toolchain, native arch,
   disk and trusted tmux preflight. Serialize jobs. Use the plan's same-SHA
   controlled local evidence while a runner path remains unconfirmed.
   No untrusted PR workflows, same-repo unreviewed refs or additional
   GitHub-hosted jobs. Keep default core CI on trusted master and use the
   protected route for reviewed candidate PR evidence. Installer/live
   trials and benchmarks remain opt-in. No new runner provisioning.
5. Report at least 20 independent, bounded trials per repeatable operation.
   Include latency to Pending/acceptance, heartbeat publication to local
   projection, encrypted bytes and publication counts, and Command
   latency under Peer activity versus an idle Peer. Report sample count
   and p50/p95 separately for local and deployed environments. Remove
   private relay/profile data and retain exact source/build provenance.
