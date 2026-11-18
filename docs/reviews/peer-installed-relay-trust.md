# Installed Peer TLS and local qualification review

Claude Code Auto, model `claude-opus-5-5`, approved the first [design](../phases/peer-installed-relay-trust.md)
draft in session `14e95b98-13eb-49a6-95cb-250e1c5f9efe`.
The revised design added connector and owner lifetime, pinned-root signature
constraints, and exact scope and staging recovery checks. Claude approved the
revision `3ae9966d0d9ee4e7f07488bedf3fcfc8539e513b` in session
`0db96a4e-32a5-4983-a22e-b46d7fa383de`. The design SHA-256 is
`c6eba14708c9eede576e00f533adfc94d2c2c6f4c6555246175813237a0f2ef9`.

Code R1 for `d784357c6ee88b5547c81acd5f1973aa625ad308` requested changes in
session `7f5e2314-c495-4b0d-9ca1-703fd45e1c6c`. The reviewer confirmed the
transport, owned trust and record boundaries. It required tests of time
refusals, CA file and chain bounds, and cleanup after staging. It ran 5 new
tests and 160 existing transport, configuration, identity and Peer tests.

Claude Code Auto, model `claude-opus-5-5`, approved the complete R2 code
`359d9d8821277ad7e2922d92e4073f294eab7d3c` against `9bfdc1b` in session
`91152ad2-54b9-4c29-8fd7-54b5c00518bc`, with zero blocking findings. It
checked all three fixes and independently ran the exact eight-test suite
successfully. The update also calculates the expected SPKI independently,
exercises additional root-algorithm and KeyUsage cases, and checks the upper
record size. The author ran the [exact regression and build checks](../validation/peer-installed-relay-trust.md).

Remaining coverage notes include direct attribution of oversized-file
refusals to the bounded read, additional RSA and algorithm mismatch
fixtures, standalone lock-provenance parser cases, and record-specific
directory replacement cases. A future installed owner needs independent
issuer evidence, a relay contract, and a reviewed cancellation and
admission lifecycle. This phase supplies local substrate and no installed
qualification or activation.
