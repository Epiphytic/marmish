# Charged Peer founder turn code review

Claude Code `claude-opus-5-5` independently reviewed the complete test-only
implementation in `--permission-mode auto` against the [approved design](../phases/peer-installed-founder-turn.md).

| Round | Exact source | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `4729f2e6428e8c33f12d54ab3eff7771867b2e26` | `ea53fd41-f0fd-4582-ad9f-cb7eb2b73e89` | Request changes |
| R2 | `e552b4fcb991d75830159037537cdd434cc8aacb` | `9089ea58-c7f7-4eea-8d27-084c430e8b9e` | Request changes |
| R3 | `5323b52f9849bc12902316d09a91888237e6cb17` | `e7e8c851-ac30-4041-b7d8-080b8f395a19` | Approve |

R1 requested an owning type that rechecks the current grant and qualified
route at each durable or native effect, refusal coverage that actually calls
native provisioning through the owner, and separate treatment of committed
cuts and an interruption during a native write. It also requested precise
lineage checks on both online and cold paths. The revised `FounderTurn`
owns the lease-bound port, ledger, paths and quota. It rechecks the grant,
time and exact previously authenticated Consent through reserve, file create,
native open, withdrawal, readback and freeze. The real loopback revoked-grant
case calls the owner, verifies durable `Consent` and observes no native file.
The owner derives the local signer from its leased material. The ledger seals
exact authenticated revisions and frame bytes before durable storage. Its cold
reader cross-checks those revisions and the recipient and compares the exact
encrypted row bindings. It cannot read the inner sender of an outgoing gift
wrap from local keys. The authenticated live writer establishes that provenance
before persisting it. The native verifier checks both refusal adapters after
closing the runtime.

R2 independently confirmed the authority and grant-revocation corrections.
Its exact binary reproduced two negative-test mistakes. One passed the
crash boolean into the quota constructor's recovery parameter. The other
expected pure codec expiry before the end of its allowed clock grace. The
correction enables the test-only VFS interruption directly, leaves native
recovery disabled and uses a time beyond codec grace. The owner's stricter
send and native expiry rule is unchanged. R2 also required repeating the
ordinary Command and normal binaries checks on the corrected tip.

R3 confirmed all three blocking findings were resolved. The reviewer
independently ran the exact compiled standalone native create, withdrawal and
cold readback proof in 14.19 s, and the abrupt native write and cold quarantine
proof in 10.56 s. It reproduced exit 80 with the journal outstanding and
reviewed the complete author results for the five focused cases, existing
authenticated route, physical binding, codec tests, ordinary Command, normal
binaries and Clippy. The author separately ran the explicit opt-in generic
quota recovery and offline codec benchmarks. [Exact author validation.](../validation/peer-installed-founder-turn.md)

Nonblocking findings leave distinct limits on the evidence. The generic wire
suite checks the skew boundary, while the founder standalone case uses a
sample beyond grace. The package negative exercises preparation, with valid
Delivery parsed and authenticated by the actual loopback receiver. The wrong
inode is recorded as a `Started` witness and then refused at native open.
The stock VFS refusal establishes path fencing. The abrupt exit predicate,
remaining nonempty journal and inventory refusal establish the actual
crash boundary. No founder authority is obtained on cold entry.

The R3 result text (UTF-8, including trailing newline) SHA-256 is
`679c9a94731026b204161d072b72c49af8031ce81cfc9ede203ea379f69674d6`.
