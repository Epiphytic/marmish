# Private WSS heartbeat and native leave journal design reviews

Claude Code in Auto mode approved the two phase designs for isolated proof
work. The Beads records are `marmish-dwt.4` and `marmish-dwt.3.4`.

## Heartbeat provenance over private WSS

Claude review session `b92bb300-738b-4a1e-8fe2-ae173e3d2e35` approved
[the revised design](../phases/peer-heartbeat-private-wss.md) at
`3288fe37fb650d01405620128db4a013412d2554`. Its SHA-256 is
`62a8abf7815e1f440489ec86f417bff5c88624f7483af7cb5bd5f4d7204f67a2`.
The revision defined the same-storage callback boundary and watchdog,
separated the heartbeat size from that of permission pages, described
the in-memory prepared state, and specified the negative and counter checks.
The limit of 16 negative cases applies to relay-backed cases.

## Native leave journal, checkpoint C1

Claude Sonnet 5.5 review session `cf7eff92-1251-4b48-b98e-dddef1226332`
approved [the revised design](../phases/peer-native-leave-c1.md) at
`2eb54fb81192832bdde6124cb0bfdf1a14009ce7`. Its SHA-256 is
`012edc1fb2f9a269f7a98c2c6a196959f7bf03db06b69f183fedfa5cb156eeae`.
The revision excludes held rows before the cleanup limit, makes the
recovery data immutable through independent database constraints, and
selects recovery records before applying its limit. The implementation
check also covers refusal to add a hold retroactively through either
the repeated-decision path or the journal insertion API.
