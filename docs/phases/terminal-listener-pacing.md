# Terminal listener pacing and rotation

Follow up on the [installed writer review](../reviews/terminal-installed-writer.md)
at `a8a434b`. Scope this increment to the daemon's listener setup and the
Command actor's snapshot order. Preserve the wire, grant, freshness,
registration, retention and room ownership limits.

## Pace registration

Keep a single selected `Server::listen` task active while the admission
ledger refuses registration of its exact peer and WSS origin. Retry at
250 ms, 500 ms, 1 second and then 2 seconds after each failed attempt.
A monotonic Tokio deadline survives stale snapshots, suspect fences and
unhealthy routes. Check daemon stop, listener eviction and retired authority
before each attempt. The first attempt is immediate when these checks
and transient eligibility pass. Poll transient eligibility every 25 ms
when the retry deadline has passed. A skipped check changes neither the deadline nor the
failure count. The latter starts at zero for each newly selected task and
ends on success or termination. Inbound transport failures keep their
separate, existing backoff. Emit the closed registration diagnostic on the
first refusal in a task and rate-limit subsequent messages to at least
30 seconds apart. The extracted wait function injects the current and
eligible gates, registration call and diagnostic sink. The production current
gate includes both stop flags and the fence retirement check.

A waiting task occupies its existing listener slot. When evicted, a task
in registration wait exits on the next check, after at most its current
two-second delay. Existing inbound and admitted operations keep their own
bounds and ownership. There is no relay connection, Probe or native unit
in the retry loop. The real ledger still enforces scope capacity and
suspend-aware quarantine. Rejected registration consumes neither a new scope
nor a probe ID and preserves existing quarantine and retention deadlines.

## Rotate the listener snapshot

Select from the verified, admitted Command rooms returned by the existing
journal query. The journal reads at most 65 active room rows in MLS group
order and rejects more than 64 across both directions, before server room
validation. The registry cap stays at 32 server fences. Match validated route context, grant and peer through the existing authority checks.

Order the eligible peers by account. For each peer order its rooms by
persisted session and MLS group. Select one room per peer in each round
and finish the round before selecting a second room from any peer.
For more than 32 candidates, retain a start account and the last credited
room key for each current peer. If the start account is absent, begin at
its sorted successor, wrapping at the end. For each peer with a credited
room, start just after that room key, with the same sorted wrap behavior
even if the room was removed. Prune keys for absent peers and reset all
scheduling state when the candidates fit.

A pass lasts 30 seconds from the first over-capacity snapshot or from the
normal refresh that consumes an expired deadline. Keep the same cursors
within that pass. After each successful complete snapshot build, record its
consecutive prefix of rooms for which `Authority::register` succeeded.
Keep the longest prefix observed in the pass, preferring the most recent
on equal length. Record account and room keys so churn never changes the
identity of credited rooms. Registry-capacity refusals are soft and the
builder still tries the other selected entries. A failed snapshot build
preserves the recorded prefix. At the end of the pass, advance the start
account by the prefix length modulo the peer count, using a minimum
stride of one before applying modulo. Advance each credited peer to the successor of the last room it registered in that
prefix. A zero-credit pass moves the start account and preserves room keys.

Reattempt the selected order on normal refreshes. An evicted registration
may remain in the registry until the prior watch snapshot and listener
drop their fences. A live attachment can hold its fence across passes.
Rotation never force-retires those live fences. Existing authority
invalidation still applies. Partially funded passes advance on completed
prefix registrations, avoiding a fixed stride that repeatedly misses later
rooms behind held capacity.

The guarantee covers opportunities to register verified Command fences with
a stable candidate set, normal actor refresh and available registry capacity.
For example, 32 peers with 33 rooms belonging to one peer and one room
belonging to each other peer can give the busy peer one room per pass. Its
rooms can then recur every 33 passes, about 16.5 minutes even with full registry
availability. That 64-server-room example assumes no active client
rooms. A registered fence can wait for the ledger. Replay tombstones
last up to 60 seconds, so a room may lose listener time or skip a pass under
transient refusal. Rotation does not promise exact timing under churn or
successful attachment during a selected pass.

## Acceptance

- Exercise the registration wait with paused Tokio time, a real full
  64-scope ledger and an attempt counter. Verify attempts at 0, 250, 750,
  1750, 3750 and 5750 ms, then one every 2 seconds. Toggle eligibility across
  a deadline, count diagnostics to show suppression within 30 seconds and
  emission on the next eligible refusal, drop a holder and show recovery,
  and evict while waiting to
  show no further attempt. Separately exercise repeated `CLOCK` refusals
  using synthetic suspend-aware samples at the ledger's private test seam
  and verify quarantine, scope and retained probe state.
- Verify deterministic same-pass ordering, 33 rooms plus 31 single-room
  peers, balanced 20-by-3 room allocation across passes, zero prefix credit,
  removal of the start peer or last room, and reset when the candidates fit.
  Register synthetic exact contexts through the actual authority registry
  across turns with up to 16 held fences. Check for bounded capacity,
  preserved generations, and access to the tail after idle clones drop.
  Exercise the initial deadline, a refresh just before 30 seconds, a
  refresh exactly on the boundary and reset when the candidate count fits.
  Verify the pass is not extended by refresh. Assert the longest complete
  prefix wins, ties prefer the latest identities, a failed build preserves
  the record, and a soft capacity refusal still lets later bindings register.
- Use the offline native MLS Welcome and SQLCipher fixture to create 34
  verified server Command rooms across a few peer accounts. Run the exact
  server snapshot entry with an explicit monotonic time. Hold existing
  snapshots and active handles across a turn, then drop the idle handles
  and assert bounded recovery and tail registrations. The synthetic
  scheduler cases cover peer counts and room distributions omitted from
  this opt-in, cryptographic fixture.
- Run the narrow core selectors, the opt-in native fixture, strict
  all-target Clippy and the existing installed writer test over the
  private WSS relay and tmux.
