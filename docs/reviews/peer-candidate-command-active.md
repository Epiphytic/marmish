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
