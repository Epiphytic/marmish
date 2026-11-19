# Manual recipient service design review

Claude Code CLI in Auto mode reviewed the service design and its cited
source on base `d2d014b4824d3d1d0cee3776d65df3256597faf4`. These were
read-only design reviews. No build, daemon or relay test ran.

- R1 `4939ef14e9eca9084f4ffd1ac141b6eefe02189b`, session
  `facd00b2-e37f-4a4b-a079-444e2d5a3c90`, requested changes. It required
  a finite buffered-live drain, explicit busy-poll and cancellation bounds,
  and final guards for every durable ingress path.
- R2 `afc826088e11c379b97df818d909c94d29dd7084`, session
  `63f82200-6f65-484e-ba8b-4d6791e34d7d`, Claude Sonnet 5.5, approved.
  It verified the cursor's Recovery/Periodic distinction, exact EOSE and
  committed short-page completion, the four-record buffer, Consent exposure
  accounting and the final guard matrix. The first increment ends with
  guarded same-owner cursor/port ingress. The mounted rotor, native expiry
  and read-view publication are separate dependencies.

Apply the reviewer’s nonblocking implementation clarifications within that
scope:

- Bind the final synchronous guard to the opened Owner's exact scope and live
  bit. Take the full checked physical observation at the port boundary and
  retain existing startup and per-turn horizon checks.
- Preserve the current same-founder Offer Backlog refusal. It yields no
  receipt, including no Deferred receipt, and cannot complete the recovery
  page. Retire the local owner on a drain classification failure before
  entering its selected History or Consent turn. Test these refusals with
  durable scan state and no read availability; cold replay can encounter
  the same backlog until the durable conflict is resolved.
- Each mandatory drain processes at most four queued inputs with their
  existing native deadlines. This can delay the selected turn and cause
  view freshness misses. Measure the aggregate elapsed time in installed
  qualification. A cold resumed cursor preserves its original history
  bounds and then takes live traffic from the newly opened subscription.
  It does not start a fresh lookback on the same incarnation.
