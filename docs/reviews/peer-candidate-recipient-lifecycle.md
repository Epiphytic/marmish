# Registered passive recipient lifecycle review

Claude Code Auto approved the detailed
[lifecycle design](../phases/peer-candidate-recipient-lifecycle.md) at
`6f5b0b1` in session `900dd3bc-e2b5-4143-9a66-9f373318badf`. The
reviewed boundary is a private passive actor with one checked Peer
route, no invitation processing, and unavailable Peer reads.

Claude Code Auto approved the standalone lifecycle source
`8f78776` in session `cce7dc50-9f7c-4402-a1dc-126997b37690`.
The review recommended moving the injected reap panic after actual
revocation. The integrated test now panics at that boundary.

The [independent fixture review](peer-candidate-passive-recipient-wss.md)
required an actual on-wire refusal and complete unpaginated Command
inventory. The revised fixture also holds startup acknowledgment for
two seconds and checks that no cursor file or route appears in that
interval.

Claude Code Auto session `3b66ab63-e097-4263-abf6-cd41c3e98ee4`
approved the integrated source `3a224b9`. It checked the registered
loss observers, passive readiness and raw drain ordering, actual worker
join, custody retention on an unproved end, and private socket path
permissions. A seven-commit range-diff matched each patch at the final
source `f149ad656631678e18e3f6c81a0d461de9ccd39f` after a docs-only
base update. The review was source-only. Injected panic and no-start
evidence comes from the actor subprocess suite. A simultaneous real
Peer/ordinary dispatcher displacement needs a combined fixture.

Claude Code Auto session `b2e96c5e-4b10-400d-b2bf-0168f8e45a97`
independently passed the exact passive candidate fixture and kernel-path
test. It blocked the overall review when the dispatcher-loss fixture
returned a refusal before the snapshot frame on the first attempt. Its
next two attempts passed. This initial independent run did not qualify
the dispatcher-loss fixture as repeatable.

Claude Code Auto session `ec10b586-17f3-4b1d-84e2-69adaad49a44`
approved the diagnostic delta `e77664a` and the passive increment
with a recorded limitation. It independently passed three exact
dispatcher-loss runs in 25.0 to 25.4 seconds. Both five-run author
campaigns passed as required by the review. The original intermittent
refusal remains unclassified. Control-channel startup and frame-write
failures can still share the generic `control` category.
