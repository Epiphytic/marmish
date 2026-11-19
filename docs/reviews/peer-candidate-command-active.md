# Active candidate Command owner design review

Claude Code Sonnet in Auto mode independently reviewed the docs-only design at
`2970168f79f02ee3a52793e1d6daaa69aecdd2f6` against `c990cf9`.
R2 **approved** the design in session `242973be-8bc3-4157-9731-1a88c5645973`,
with no blocking findings. The reviewer inspected existing source and the
pinned MDK checkout. This was a design review.

R1 `ad413b7b-6ed2-4249-8769-0b0d27cf1482` asked the plan to account for
registry verification cost, preserve typed dispatcher loss through error
mappings, and separate the unmounted owner exit from the caller and
startup-reporting contract. R2 requires full default verification on
native-bearing polls and keeps the atomic check for narrowly enumerated
wait-only polls. This resolves the case where the next native wake is the first
observer of unannounced default displacement.

Carry the following accepted qualifications into implementation and mounting.

- Start with full checks by default. Enumerate latch-only cases from audited
  wait-only contracts, initially SDK acquisition completion and an
  already-finished forwarder.
- Before mounting, verify that the exact transport graph has no unfenced
  detached durable write or unfenced connector. The source base directly
  constructs `transport_nostr_adapter::NostrTransportAdapter::new` using
  `NostrSdkRelayClient` backed by `RelayConnector::sdk_client`. The MDK
  `AccountDeviceRuntime::open` takes that supplied adapter. The distinct
  `transport-nostr` relay plane's optional delivery-spill writer is outside
  that direct constructor path. Recheck the graph, including anonymous and
  worker-owned connection paths, at the actual mounted revision.
- Count exact full admissions and measure time under representative load.
  Keep quiescence assertions at each selected cut ordinal. In the composed
  caller explicitly prove no degradation marker, failed-room retry entry,
  or generic fallback reply from fatal dispatcher loss.

## First owner source increment

Claude Code Sonnet in Auto mode approved exact Rust at
`776487bc946f6c5be3c5f4586f0971aeb800dd9b` in session
`39647f9f-80e7-47fd-9869-2c338142cb9d`, subject to the author checks
linked below. The review inspected the implemented boundaries and ran a
scoped `rustfmt --check`. Claude independently reran the exact displacement
selector in session `25389fd7-b22f-4f43-b12c-0c3377dbb40c` and recorded
one passing test.

Preliminary review `2ddfebab-ca76-4036-9eb9-315f08013e23` requested the
correct Tokio clock type in the test and a new admission check before each
orphan compensation effect. The final source corrects the test import, checks
each selected orphan before rejection or removal, and checks again around
native forget and journal cleanup. It also checks retained Welcome
compensation before entering that path.

The approval covers the inert owner source increment. The owner exit still
needs mid-orphan cut coverage, the other per-site cut cases, byte-exact keyed
restart projections and verification of SDK callback refusal. The mount must
preserve the Command request allowlist and check fatal before reply
classification, including timeout and generic fallback. [Validation and
remaining checks](../validation/peer-candidate-command-active.md).
