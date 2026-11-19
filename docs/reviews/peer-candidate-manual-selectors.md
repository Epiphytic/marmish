# Private Manual work selection review

The selector and package effect leaf follows [the approved Manual scheduler
design](peer-candidate-manual-scheduler-design.md). This code is a dependency
for the private scheduler. The full mounted lifecycle is reviewed separately.

Claude Code ran in `--permission-mode auto` against the isolated source.
The review scope covered the read-only work and Pending selectors, exact
charge and journal classification, native issuance and attestation, and
transaction checks at each guarded package transition. The review inspected
source and evidence. Author and integration checks run independently.

| Reviewed source | Reviewer session | Verdict |
| --- | --- | --- |
| `4906677c697b0bbe8c3b7f321fd87d959dc302c8` | Claude Opus, `41cf1531-6512-425c-9541-94f013941a63` | Request changes |
| `20bb4e58e2c7915c0c5579277424bd22319a2aaf` | Claude Opus 5.5, `92baf0e2-7161-4dfa-a896-ab801f2febc8` | Approve |

The first review found that a cold `JoinStarted` row with an exact
`JournalCandidate` could never reach readback. The approved revision pairs
the journal with its original charged stage and activation coordinates,
allows only exact `JoinStarted` recovery, and tests wrong-generation and
wrong-operation refusals. It gives reservation and signing separate windows
and rechecks the active window immediately before the Bound, NativeStarted
and Prepared transactions commit. Refusal rolls back both durable wall floors
with the stage transition.

The final reviewer kept one integration caution. A package stage commit can
return `Expired` after a window, forwarding or stop refusal. The scheduler
must end the Peer generation conservatively on that result. A separate
checked retirement operation remains the authority for actual Offer expiry.
Full cold journal recovery through the mounted scheduler and stage expiry
after each native transition remain integration coverage.
[Validation results](../validation/peer-candidate-manual-selectors.md).
