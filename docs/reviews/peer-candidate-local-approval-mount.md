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
