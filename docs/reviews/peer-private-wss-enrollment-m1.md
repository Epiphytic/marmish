# Private WSS Peer enrollment M1 review

## Design

Claude Code Auto independently reviewed the
[phase design](../phases/peer-private-wss-enrollment.md) in these sessions.

| Session | Outcome |
| --- | --- |
| `9401b56a-9189-4db6-96ff-dfbc485dea2f` | Blocked the original plan until M1 stopped at verified Delivery, the joined Pending owner received its own review, and authenticated carrier dispatch preceded Offer admission. |
| `5f4d5ab2-62a2-4372-918a-90d6bdaff3b3` | Approved M1 with M2 custody separated. |
| `8af93a2f-b7a9-4503-a2ab-454c2e6bfa8a` | Approved the final design amendment that explicitly rejects bare native kind 1059 on the kind 7435 Peer inbox. |

The approved design has SHA-256
`3f5d512e658fe416202e29e6c59fe7c9dc5810e6b8cde97cb7a809bec495468d`.
Implementation started from `1c6de7e` and the Prepared Consent inputs
`b7b433d`, `cfb3410`, `23acc89`, the independently reviewed `bef9e8c`
amendment, and the standalone wire test equivalent of `0003703`.

## Integrated source

Claude Code CLI ran in `--permission-mode auto` using
`claude-sonnet-5-5`.

- R1 `bc528964-293d-4832-b00d-c442e6d91a1c` blocked
  `234d800f6b3b75d8e1df45580e36849cc952c031`. Live arrivals could
  bypass the cursor quarantine. The reviewer also asked for signed index
  verification, a private origin constructor and relay refusal coverage.
- R2 `fb792e02-2c6c-4a3f-9f02-24e54e9eff73` approved through
  `8e351ba8c98a600397bc4586731b3429115adfdd`. The fix fences live, history
  and Consent work before dispatch or settlement. Delivery compares the
  signed outer index before stage lookup. The exact fixture now uses a
  private origin constructor and tests the relay's read and write denials.
  The reviewer confirmed that M2 must independently recover exact custody
  and provide durable disposition for unmatched signed Delivery.

- R3 `6ba8c5e2-963b-4fa4-87e4-d3ffe727ce7d` approved exact source
  `5ce1727051a64b4708e4f783e568f3fb724e2984`. The reviewer checked the
  16-byte signed Offer selector, standalone signed codec regression,
  recipient verification and shared wire refactor. The reviewer also
  observed the final 23.59-second WSS pass and relay refusal pass. The
  two remaining release checks were the standalone cursor result and a
  clean strict Clippy run at the exact source. Both passed after review.

The reviewer inspected source, diff and formatting evidence. Author-run
runtime and compatibility checks are in the
[M1 validation report](../validation/peer-private-wss-enrollment-m1.md).
