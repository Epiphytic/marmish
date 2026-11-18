# Peer native stage binding review

Claude Code independently reviewed the [design](../phases/peer-native-stage-binding.md)
and the exact code in Auto permission mode with Sonnet 5.5. The final source
review verified the staged tree and all 13 file hashes before and after
inspection. Its scope was Git tree `01c424fceb2c754c3d7d1138d990b9f0ffec9e12`
on `8e14bc6075e86580e0760a5486fb2bd0fee6df52`. The SHA-256 of the scope
manifest was `3d6402c87aedfa167636a8a74ea05080df41de9aa8da8601ed48f74ab09ddf4a`.

| Review | Claude session | Fingerprint | Verdict |
| --- | --- | --- | --- |
| Design 1 | `a2e8c4bc-d3fe-4bc9-a7b2-75231ac5c01d` | SHA-256 `fbe9a161e4060f4df5b6d0f6974076fca224b68effaa1721b87cc12a8da9fda4` | Changes required |
| Design 2 | `5a9675de-71a3-4f5b-b975-e363f08c0428` | SHA-256 `deef2d774962dca5325fa2dac03be278d90de9bd9a74297258b8c9cd1f252f33` | Changes required |
| Design 3 | `3af2fe07-1cb3-43cb-97c9-e9024724c7b9` | SHA-256 `fff896fad010bd5d990badb5e1ff7bb0035b488bce59572616f69bf9244093f2` | Approved |
| Code 1 | `b6e4284c-7988-4650-b19b-55daa7128dbe` | Tree `cae13ea6a885cf8e789708ae03189667e8633059` | Approved |
| Code 2 | `be769978-cd56-4c93-b1aa-c9588f730210` | Tree `9a0999986ccc418db6ad46c644dc6f3df1a9bb56` | Approved |
| Code 3 | `c92e953d-3267-4732-81df-88e26b33f454` | Tree `01c424fceb2c754c3d7d1138d990b9f0ffec9e12` | Approved |

The design added monotonic state restrictions, SQL collision checks,
public MDK migration-count inspection, a 120-second local clock tolerance,
and an artifact-level oracle for the production constructor. Code review
led to reuse of the authenticated Offer and local owner checks, explicit
quarantine coverage, trigger-specific error assertions, exact Offer
presence even when foreign keys are disabled, and missing or altered
Prepared database coverage. The final reviewer independently passed the
SQL reference-graph and native exact-recovery cases. The author ran the
full focused stage and standalone cursor suites on that exact tree. See
[author validation](../validation/peer-native-stage-binding.md).

The final approval covers a test fixture with no physical relay or
application authority. The stable stage class is the primary key in
version 1, and each of the two classes can own one binding. Production
activation still depends on native-safe retirement, cooldown,
provisional Consent and Welcome custody, and once-only consumption of
the native package. It must also enforce checked file and artifact
proofs across the Rust API boundary and handle re-attestation or
retirement after remounts.
