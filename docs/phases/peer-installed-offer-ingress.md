# Installed recipient Offer admission

This increment of [the installed recipient owner](peer-installed-recipient-daemon.md)
closes the gap between the authenticated physical route and the installed
custody successor. The current successor deliberately rejects the generic
`Checkpoint::admit` entry. A private fixture can seed a v2 Offer in the
predecessor and upgrade it, which proves migration and cold recovery. The
qualified owner must be able to receive an Offer after opening the successor.
This increment leaves daemon startup, provisioning, send, Delivery and native
Group work outside its scope.

## Interface and authority

- Add one borrowed observation for one carrier. Its fields are private to the
  authenticated recipient port. The port creates it only in a synchronous
  callback, after checking an exact page or live record belongs to its active
  socket and observing the physical route again. A page selector is one index
  inside the checked page. Use the current authenticated account, normalized
  origin, qualified wall sample and exact raw JSON bytes/ID/stamp. No public
  constructor, saved capability, or caller-supplied clock. Reject an observation
  from another port or retired socket before the callback. Preserve existing
  page/live readers and bounds.
- Add a narrow successor method accepting that observation and this exact
  checkpoint's source. Require `InstalledCustodyV2`, same source instance,
  account and origin, and installed Manual receive mode. Do not grant rights
  from the ordinary execution or trusted-remote lists. This installed scope
  classifies founders as Unknown. Reject local ineligible mode or stale binding
  without a durable disposition.
- Reuse the existing codec to verify the outer event and compare its authenticated
  index. Authenticate the inner carrier classification before applying an Offer.
  For a real v2 Offer, use the exact configured route, verify its signed ancestry,
  interval, intended recipient and founder, and apply the exact self/block policy.
  A verified Delivery and unsupported carriers return to the later dispatcher
  without committing an Offer, invalid receipt, or cursor progress. The general
  invalid-carrier classifier will be integrated separately with Delivery. An
  Offer with a stable signature, route, policy or expiry refusal can return a
  typed invalid classification without progressing this fixture's cursor. A
  local index/SQL/ownership failure is an error, not a remote classification.
- A verified current Offer writes under the existing stock capped SQLCipher
  cursor transaction. Check the exact successor epoch, local wall floor,
  quarantine and preserved ancestor constraints. Reuse the existing installed
  replay sweep and cooldown, bounded `rows::admit`, and immutable v2 marker.
  Revalidate the successor and exact signed parent before committing an applied
  Offer. A replay does not rearm provisioning or change the original ancestry.
  Create no slot, decision, Prepared Consent, or outbound work. Use exact
  deferred-row removal for trusted dispositions. Full or unavailable writes
  yield backpressure with no usable receipt. Durable backlog staging and page
  or live cursor advancement remain with the later combined owner so a caller
  cannot move past an unconsumed row.
- Keep the successor entry test-scoped with the existing successor layout.
  Direct legacy or marker-predecessor entry points cannot mint this observation
  or write into the successor. The future startup owner must still prove the
  live lease, read-only Ready binding, current filesystem and bounded cursor
  inventory before opening the port. This proof creates no installed runtime
  mount and does not claim end-to-end enrollment.

## Validation

Use the existing private checked WSS fixture, real authenticated page and live
records, disposable identities and an empty capped SQLCipher successor with no
Offer seeded through the predecessor. Observe a genuine newly inserted v2
marker, exact retained signed bytes and original proof after cold reopen.
Exercise an exact replay and a new unrelated Offer delivered live. Show absent
writes for wrong account, origin, retired port and source, wrong recipient or
route, legacy/wrong flight kind, self, blocked/expired, and local state
quarantine. Use targeted deterministic cases for policy and storage faults
that require corrupt or blocked local state. Verify no legacy decisions,
outbox, slot or Group effects, SQL rollback under failure, and the bounded
Unknown retention behavior. Run strict native Clippy and nearby Offer, cursor
and route regressions. Obtain Claude Auto approval before code and on the
exact composed result.
