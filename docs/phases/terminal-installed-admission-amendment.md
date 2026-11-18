# Installed Terminal admission and cancellation amendment

The installed Terminal opens a fresh ephemeral MLS room for each
attachment on the verified ordinary Command binding. The existing
one-new-Probe per 60 seconds per listener would refuse an immediate
Busy, New or reattach on that same binding. The prior loopback writer
proof used independent bindings.

## Server admission and replay

The profile-owned Terminal runtime holds one suspend-aware Freshness
anchor and a 64-entry table keyed by canonical peer and configured
origin. All decisions take place in one synchronous current-thread turn,
including wall/tick sampling, clock check, ordered sweep and insertion.
Keep the clock and entries behind one standard mutex. Poisoning fails closed.
No await or thread dispatch may occur inside that turn. The Freshness
anchor and its 41-second uncertainty hold survive every listener or
binding generation change and every table-entry eviction for the life
of the profile owner. A replacement listener or transport subtask must
reuse the anchor. Only releasing the profile lease discards it. Any unproved
time fails closed. The anchor detects wall
rollback against continuous time using the approved jitter bound.

An entry holds eight authenticated unique Probe IDs in insertion order.
Retain each row in the half-open window [charged, charged+60s). Sweep
using the checked continuous tick before evaluating capacity. An exact
duplicate under its original signed timestamp never charges, renews
expiry or creates any effect, including on a different current Command
generation. The table may reclaim an empty entry only after its last
registered listener reaps. If no entry can be reclaimed, refuse the
65th distinct scope. Both the old 45-second replay retention and the
old single-row last/interval gates in ConsumedProbes and Exchange are
replaced by this owner gate; Exchange still enforces one live
transcript and its absolute deadline. The Command and relay-bound
signature, recipient and wall freshness checks run before admission.
Charge before generating any key, response or other protected effect.
An entropy, encode or later send failure consumes the ID until expiry.
Do not accept or charge a competing Probe during an unsettled flight;
the exchange does not replace its transcript. A later exact retry of
an uncharged contending Probe can start a flight after reap if the relay
delivers it to the next listener. A consumed Probe can never do so. Set the
verified room and listener quotas independently to the already
approved 16 volatile rooms per direction and 32 eligible listeners.

The in-memory ledger does not survive daemon restart. An otherwise
fresh Probe from before a restart may trigger a fresh Challenge within
its signed 30-second age policy, 5-second skew and 2-second tolerance
(at most about 37 seconds under approved clock assumptions). The new
Challenge transcript is random and the old Confirm/Ready cannot create
a room, writer or physical effect. Preserve durable Command authority
verification after restart. No additional startup delay in this slice.

## Client retry and owner cancellation

Replace the runtime client's single 5-second Challenge wait with exact,
already-signed Probe retransmission at elapsed 2, 5, 10 and 18 seconds
after the initial Probe issue, until the first authenticated Challenge.
A cadence point may be skipped if a prior send is still in progress.
One initial budget remains in effect through the last reply and enroll,
capped at 30 seconds. This supersedes the fresh-attempt schedule in the parent design. Stop
resending after an authenticated Challenge or a failed or uncertain socket
publication. A verified OK for the exact event ID carrying the standardized
`duplicate:` prefix means the relay already has the exact event. Continue to
wait within the budget. Neither storage nor duplicate OK proves delivery. The stored Probe bytes and
timestamp remain exact. The receiver can act on a previously unseen
retry; the protocol may preserve a timeout on lost Challenge because a
consumed same-ID retry must never start another flight. Tests and error
descriptions distinguish lost Probe and lost Challenge. Pass the slice cutoff to the AccountInbox raw channel-readiness branch of
`Lane::read_exact`. Apply it only to the cancellation-safe `incoming.recv()`
and give queued notifications precedence over a simultaneous tick. After
taking a notice, complete authenticated native placement and acknowledge it
under the original absolute flight limit. A tick during placement defers
the resend until ownership returns to the event loop. Preserve bounds and
check authority before every resend. Keep the normal signed freshness and
relay alias time filter on the replacement subscription, so a still-fresh
stored event can replay if the relay honors the subscription filter. Validate
both a relay that suppresses duplicate broadcasts and a relay that dropped
the initial event before storing it. No recovery guarantee applies to an
uncharged Probe discarded by a relay or a flight that still occupies the
listener at the final retry (18 seconds, versus a 30-second server cap).

Create the local EOF flag before route open. The owner sets it, and
injects its clone into `Authority::from_fence` so `check()` and retirement
waiting see it even before the post-Confirm grant OnceLock exists. Native
retirement still sets its own terminal latch and triggers scoped cleanup.
Selection callbacks also reject an EOF, independently of fence validity. Keep polling the scoped task through its explicit
close and release; do not drop an in-progress future in the normal
cancellation path. Once observed, block new effects at the next guarded
turn. During relay readiness waits the existing 10 ms retirement
backstop applies. An already issued native publish remains owned until
its checked finish/reap. Track failed or uncertain room retirement
separately from slot capacity and report the affected direction unhealthy
through a static diagnostic and typed terminal_owner_unavailable. Keep
its permit fenced until proven reaped or process exit. Each direction can
own at most 16 such rows, using its existing room permits. Retry proof in
the same owning reaper every 250 ms, retaining a row on indeterminate
results. Refuse new work in that direction with the typed unavailable
status while any retirement remains uncertain, including exhaustion by
16 fenced slots. Clear the status only when every fenced row is reaped.
On shutdown the profile lease stays held until all tracked children and
reapers finish. Use process monotonic elapsed time for the 15-second stop
diagnostic if continuous-clock sampling fails, without using that fallback
to authorize new effects or release custody.

## Validation

Core tests cover eight unique, ninth refusal, the half-open expiry,
inert ID replay and continued clock quarantine across binding replacement.
Exercise 64 ledger entries plus one refusal, idle/expired reclamation
only after reap, and cancellation before custody, during readiness and
after receipt with exact slot accounting. Test EOF between Probe and
Challenge, a contested Probe left uncharged with exact retry after reap,
byte-identical 2/5/10/18-second sends under the guard, skipped ticks during
publish/placement, suppression after Challenge or nonduplicate refusal,
and quarantine across eviction and listener replacement. Update the old
45-second retention test. An eligible pre-restart Probe may issue only a
new Challenge; an old Confirm must create no room. Test a permanently
fenced permit, typed unavailability and health reset after proved reap,
and lease retention through shutdown plus continuous-clock failure. The manual private TLS/tmux
test covers Default, pending or established Busy, New and immediate
reattach across fresh volatile rooms on the same Command binding, and
ordinary Command concurrently. Add a dropped-then-republished Probe
fault, a Challenge later than 6 seconds and covered cancellation cases.
Measure retry and already-ready latency separately.
