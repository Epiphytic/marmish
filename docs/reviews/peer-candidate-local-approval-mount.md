# Independent review of the same-UID approval design

[Design](../phases/peer-candidate-local-approval-mount.md) at
`7a26f6933d302d01c64b1ee240d6a4fb3debf2f8`, SHA-256
`ff82c3bf52723f9b2da4fad00397bfea57c2b45afe99ae4e5092fa3e668b53a9`.
Base `75dc6138d9ed4b08079b8bb646127c5970ec79bf`. Bead `marmish-dwt.3.6`.

- Claude Code Opus 5.5 in Auto mode requested changes for `a28edfa` in
  session `a37aca36-19b4-43ec-af03-cabe7b9fa425`. The original draft did
  not specify how it would preserve typed outcomes across the checked
  port callback. Existing sibling entry points use a generic error
  adapter. The revision carries refusal, integrity, clock regression,
  confirmed decision and ambiguous commit separately and derives actual
  lifecycle independently. It also specifies the absolute native
  deadline, borrowed socket ownership, exact client uncertainty, stock
  and v2 refusal and the fresh v3 evidence profile.
- Claude Code Opus 5.5 in Auto mode **approved** exact `7a26f69` in
  session `f62b118e-96eb-4de5-910b-6d03d7c7e056` with no remaining
  blockers. The reviewer verified actual checked-port and native-fence
  APIs and the existing router's distinction between a local refusal
  and a lifecycle stop.

During implementation, thread the same narrowed physical fence into both
SQL guard calls. The reviewer also requested a focused outer port failure
test that checks lifecycle re-evaluation and proves the decision callback
never runs. These are acceptance details of the approved design.

Both reviews were read-only and used the committed source. No Rust changed,
and this review ran no build, tests or relay. No candidate was activated.

## Integrated implementation review

Claude Code Auto preliminary review of `6145a9c..76e5436`, session
`7cceb085-36b6-4cc0-a9b2-5fcff7aaf091`, found that a fresh approval
profile was first made v2 and that a previously initialized v2 profile
could be promoted in place. The revision initializes fresh v3 cursor,
installed Offer, and custody rows in one transaction. It refuses an
opposite epoch before touching the persisted schema. Candidate coverage
checks both directions and includes the actual installed approval CLI and
SQLCipher inspection across restart.

Claude Code Auto reviewed `06f7de7` in session
`a4befe73-9838-415c-9be1-72c19c80c1a0`. The reviewer found no code-level
blocker in the fresh transaction, v2 isolation, claim and lifecycle
fences, typed request, or absence of authority activation. The review was
static. It left installed test evidence and additional lifecycle and
failure-path coverage open, including a committed response lost in
transit and installed startup with the opposite cursor epoch. The
subsequent compile correction `dca025b` uses checked integer conversion
at the SQL test boundary and permits the standalone cursor fixture to
compile the candidate attach entry point.

Claude Code Auto reviewed `06f7de7..44c4c25` in session
`32c67f94-0bba-4ea0-870b-c33754081eee`. The reviewer found no code
blocker in the updated provisioning fixtures, SQL integer conversions or
post-refusal lifecycle check. The offline retired-port test exercises the
checked port and verifies that a stop observed after the port check
takes precedence over local unavailability. The real startup and
registered route are exercised separately. The review was static.

The first installed attempt recorded approval and the immediate retry,
then failed waiting for an empty List response. Production exposes a
Pending proof only as long as the row remains verified Pending, and
refuses the projection ticket otherwise. The original test expected a
synthetic empty proof available only in unit tests. The correction
`bcc2a7a` requires Unavailable from the private List and Show while the
registered route remains live. It preserves the cold SQLCipher assertions.

Claude Code Auto session `97eebb22-a0d7-4b78-84f9-69d6b1e71582` approved
publication of the first private mounted increment at `4328f92` on the
feature branch. The reviewer reproduced deterministic offline socket,
decision, custody and lifecycle tests from the compiled archive and verified
the [installed results](../validation/peer-candidate-local-approval-mount.md).
The installed binaries were built at `dca025b`. The later lifecycle helper
extraction was read independently and covered by the offline regression.
The separate standalone cursor crate also includes the test-only binding
source. Follow-up `50ab8b7` limits the compatibility correction to that
fixture's visibility and leaves its body unchanged.

## Follow-up timing and recovery review

Claude Code Sonnet 5.5 Auto approved `b5c0671` in session
`291b8bcb-b573-4c76-86d9-e1efd06ae4f3`. It confirmed the timing boundary
and the fixed debug log fields. The test now reads a complete bounded log
to avoid cutting off the two approval readings.

Claude Code Sonnet 5.5 Auto reviewed the schema fixture in session
`6017c302-8a4f-4d17-ba1c-0866280bf1a3`. We applied the bounded log reader
to the schema check, handled an exiting task during enumeration, and
documented the matching startups and unchanged cursor as the schema
evidence. The local exit diagnostic indicates where the worker refused.
Claude Code Opus 5.5 Auto also approved the
[withheld-reply fixture](peer-candidate-approval-reply-loss.md).

Claude Code Sonnet 5.5 Auto reviewed the integrated source through
`824623d` in session `2a921183-ffd4-4ffb-883a-c09ae239271c`. It approved
the code, including the candidate timing, the corrected schema assertions
and the exact dropped-reply mechanism. The reviews were static and the
operator ran the separate installed tests. The
[validation report](../validation/peer-candidate-local-approval-mount.md)
records exact package and harness provenance, both mismatch refusals,
the committed decision recovered after reply loss, and internal timings.

The [later Offer review](peer-candidate-later-offer-qualification.md)
covers authenticated history while the original Pending invitation is
retained. Current one-invitation-per-founder custody refuses the new Offer
and preserves the original. Replacement and its durable cutoff would need
a separate reviewed transition. Authenticated founder convergence and
receipt, group and heartbeat publication remain open.
