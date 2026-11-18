# Directional Peer heartbeat planner and projection fixture

This increment composes the reviewed [native sampler](peer-heartbeat-sampler.md)
and [private status codec](peer-status-codec.md) behind the
[accepted Peer room model](peer-invitations.md) and
[interactive peers roadmap](interactive-peers-roadmap.md). The planner,
projector and tests are mounted only in a new stand-alone integration target,
`peer_heartbeat_planner`. All existing sampler, codec, daemon, configuration,
profile ownership, durable state, MLS and relay adapter, CLI and required CI
sources remain unchanged. The reviewed sampler design establishes opt-in
status sharing.

## Current accepted authority

The fixture models one directional room per planner or projector. A binding
contains the 32-byte codec room identifier, distinct canonical server and
client keys, and nonzero local room generation. The server owner is pinned
to that binding and its local server key. The client projector is pinned
to its binding and local client key. The local generation may differ on the
two systems and comes exclusively from the authority owner.

The caller supplies an independently verified current binding, local account
and direction, current route verdict and, on the server, the current
execution-client grant. Missing authority, wrong side, wrong room, wrong
account, revoked grant or different generation refuses work before the
planner submits native work, constructs a record or releases a record.
Distinguish unavailable routes, verified confidential routes that suppress
addresses, and verified confidential routes that permit explicitly selected
addresses. Mixed direct/onion routes, Tor or incomplete direct proof may
never provide address-safe authority. Confidential routes can still permit
status with addresses withheld. An unverified route suppresses all status.

An optionally enabled setting contains exactly the per-client fields
allowed by global selection and current per-client restrictions. Its
validating constructor checks 300–86,400 seconds with 3,600 as the default
and validates the selected sources using the sampler's existing request
constructors. Settings absent means status off, including permission
issuance in this fixture. At most eight disk paths and eight interfaces
are allowed. The planner passes addresses only when expressly selected
and address-safe authority is present. It samples one audience per tick.
Global fanout, route discovery, fallback and plaintext publishing are
outside this increment.

The server view contains a nonzero monotonic revision and an immutable,
shared settings object. It carries both the engine-derived policy plan and
the capabilities from the same atomic owner decision. The authority owner
must advance the revision whenever room proof, grant, route, selection,
effective policy or capability facts change. A new object or different
verified route for an equal revision refuses the view defensively. Require
the exact previously installed object when acknowledging the same revision.
A lower revision is rejected without replacing the known view. Missing or
invalid current authority pauses the view and cancels private work. An
explicitly revoked grant or disabled setting needs a newer revision to
re-enable. The
planner increments a checked local epoch whenever it changes, pauses or
resumes the eligible view, including pause and revalidation at the same
revision. Old tickets, prepared records and permission issues are tied to
the previous epoch and cannot revive. Epoch exhaustion closes the planner.

## Scheduling and sample custody

The planner owns a private `SamplerPort` with start and poll operations. The
port's real implementation delegates directly to the reviewed `Sampler` and
its single process-wide worker permit. A deterministic fixture port can
script completion, busy, unavailability and expiry without native calls or
sleeps. A clock function yields one reading containing suspend-aware monotonic
time and wall time in Unix seconds. Native qualification uses the existing
`continuous_time()` and `SystemTime`; deterministic tests inject both clocks.

Only the planner can submit work and accept the port's result. No caller can
supply a `HostStatus` to a ticket. The planner associates each accepted start
with one internal epoch-bound work item. A port result after invalidation is
drained and discarded. A busy or draining port leaves due work due, refusing
extra submission and waiting for a separate caller tick. One owner tick may
poll the existing worker once or attempt at most one start. No timer loop,
blocking wait or catch-up work runs in the planner.

A new eligible view marks one attempt due as soon as the 30-second minimum
spacing from the previously accepted start permits. That same floor limits
configuration or route flapping. A successful release establishes the next
periodic deadline one interval plus jitter from the release turn. A late
tick therefore produces one attempt and schedules forward from its release.
Calculate jitter as the first unsigned 64-bit big-endian word of SHA-256 over
`b"marmish/peer/heartbeat-jitter/v1\0"` followed by the room bytes, modulo
`min(interval / 20, 300) + 1`. The exact room's jitter is stable. Failures to
collect or release retry no sooner than 30 seconds, leave the old periodic
deadline unchanged and permit at most one attempted sample per retry tick.
There is no retry accumulation or catch-up. Unavailable start is also backed
off by 30 seconds. Busy does not consume a retry or the spacing budget.
Store and compare absolute monotonic times with checked arithmetic. On
overflow fail closed. A higher eligible revision makes work due again and
still respects the spacing floor.

The work expires two seconds after the initial decision. The planner passes
the remaining budget, derived by re-reading the clock immediately before
submitting, to the sampler. The planner rechecks its own deadline at
completion and again when releasing the record. Clock errors, zero wall
time, or regressions relative to either observed high-water mark refuse the
turn and invalidate a live ticket or prepared record. Keep both high-water
marks. A later valid clock at or above them recovers the planner according
to the retry floor. Scheduling uses monotonic time. The accepted completion
turn supplies the nonzero declared application creation time. Fresh samples
never move that signed time backward within an owner run.

At most one ticket or prepared record occupies the slot. A ready record
remains private inside the planner with its non-cloneable single-use owner,
room, revision, epoch, and deadline pin. The caller consumes the pin through
`release`, which rechecks the current authority, clocks and pin before
returning typed fixture bytes. Dropping an unconsumed pin never issues a
record. The slot expires autonomously on later caller ticks. Clearing or
replacing a gate cancels the slot and starts draining any prior sampler work.
For a durable integration, the owner must reserve a nonzero sequence before
release, scope it to the exact binding, and supply the prior per-generation
sequence floor on restart. The fixture advances its floor for each newly
reserved sequence for this binding even if final authorization refuses
release. A lower or equal sequence is refused. A stale or foreign pin cannot
steal the current record. Only the authenticated MLS owner may eventually
send the typed bytes, using the supplied signed time and revalidating the
native egress gate and any frozen fanout.

## Permission issue and projection

The server view constructor uses the engine-validated `CommandPolicy` and
calls `describe_for_principal` with the exact bound client key. It uses
`Description::from_engine` and `PermissionPlan` to preserve additive default
and key-specific rules and the canonical digest. The capabilities come from
the same revisioned owner view, with explicit Unsupported, Denied or Granted
for interactive and shell-command use. The future owner derives interactive
capability from the implementation state and exact effective terminal grant
such as `config.terminal_allowed(key)`. It derives shell-command capability
from the applicable command mode. The fixture requires a supplied nonzero
policy ordinal. A heartbeat may annotate only the plan in its exact active
view. Keep a per-room last known generation and last issue timestamp for
that generation, optionally seeded with durable fences. Refuse a lower
ordinal and refuse an equal ordinal paired with a different digest.

Only one permission issue can be active. It has a stable creation time,
shared-view epoch pin and 60-second deadline on both suspend-aware and
wall clocks, inside the codec's 900-second acceptance window. An issue time
must strictly exceed that generation's previously accepted issue time.
Issue start has a 30-second monotonic spacing floor. Reserve its issue time
on acceptance, including abandonment. Each caller request emits at most
one page through the existing bounded codec iterator and rechecks current
authority, epoch and deadline. A failed gate, clock or expired deadline
abandons the issue. No stale issue can resume after a pause or a new
revision. The plan retains at most 368,640 canonical description bytes
with up to 64 chunks or a single TooLarge disposition. The transient
canonical-length upper bound is 533,440 bytes during conversion, excluding
the existing engine policy clone. Each output fits the existing 4 KiB
heartbeat or 8 KiB permission limit. A fixture owner retains one current
view, one sample slot and one bounded permission page index. The native worker limit is one
per process, and multiple deterministic planner fixtures use only their
independent scripted ports.

The client projector wraps the existing codec receiver. It requires the
current accepted client-side proof and, before input, the pinned server as
both authenticated application author and verified active leaf account at
the current epoch. The authenticated application creation time is supplied
separately. An unavailable or mismatched current proof hides the view and
refuses input without mutation. A restored exact proof may expose its
existing data. Authoritative retirement is a separate terminal operation
that discards all derived projection state. Replacement needs a new
projector with a new binding. Reuse all receiver framing, replay watermarks,
first receipt, clock/freshness calculations, retention and permission policy.
A duplicate, replay or stale-on-arrival record cannot renew displayed first
receipt or status. These test fixtures add no grant and no durable journal
behavior. A production owner must persist receiver decisions before
acknowledging the app event and maintain existing profile staging budgets.

## Qualification and handoff

Deterministic tests use a scripted sampler with bounded manual ticks.

- Disabled, invalid role, wrong account or generation and unavailable route
  never start the port. A Tor-class route starts with addresses withheld.
- New and changed eligible views become due once, within the minimum spacing.
  A busy start stays due. While sampling, a second tick produces no ticket.
  Clock jumps produce neither catch-up nor an early retry. Bounded jitter,
  deadline overflow, clock rollback and recovery use injected readings.
- Pause and resume at the same revision, a newer view, and a changed
  same-revision settings token revoke a live or ready ticket. Its late result
  never attaches to a second ticket. Release refuses foreign pins, expired
  pins, a wrong binding or reused sequence. Gate rejection still fences an
  exact newly reserved sequence.
- Test real engine validation and default-only, key-only, and additive layers.
  Cover absent, empty and patterned allow lists, denies, and all explicit
  capability states. Exercise absence of ineffective layers, wrong prior
  ordinal/digest, same-issue rejection, per-page change and deadline expiry.
- Wrong signer, leaf, room, generation or role refuses input and preserves
  the view. Duplicates preserve first receipt and original observation.
  Unavailable views hide data, restoration requires exact proof, and
  terminal retirement or replacement discards the old view.

An ignored native qualification uses the real continuous time, native
sampler and codec receiver. Only the scheduler and projector clocks
receive the benchmark offset between completed samples. Stage latency is
measured against unmodified elapsed time. It runs at least 200 local end-to-end
samples after verifying one fixture round trip. Report p50/p95 and timing
by sampling, planner/encoding and projection stages, along with actual
platform and cross-check limits. No host or policy values are printed.
Run the planner and existing sampler and codec targets, formatting and
strict Clippy. The new target and benchmark stay outside required CI.

The future durable Peer owner constructs these exact proofs only after
acceptance and routes private status solely through the MLS publication
owner. Configuration loading, local inventory IPC, restoration, retrying
frozen attempts, real MDK egress fencing and platform/network qualification
require later reviewed integration.
