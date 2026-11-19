# Registered passive recipient lifecycle validation

The native source under test was
`f149ad656631678e18e3f6c81a0d461de9ccd39f`. Linux AMD64 used the glibc
2.39 baseline. The exact native release tool prepared a candidate
archive and the native installer checked the pinned SHA-256 manifest.
An external OpenSSL check separately signed and verified that manifest
with a disposable Ed25519 key. The native installer used the digest
rather than that signature. The test-only signing key was removed.

The test ran from the installed version's directory against the exact
daemon, worker and gated qualification example. It prepared disposable
keys and distinct private WSS relays for Peer and ordinary Command.
Closing the provisional startup report caused startup refusal without a
Peer cursor or connection. Holding the report for two seconds before
ACK also left both absent. After Commit the same recipient opened its
cursor and authenticated exactly one subscription.

The ordinary worker executed the binary-safe Command fixture with Peer
active and again after the relay closed that Peer socket. The complete
local inventory contained exactly the two expected invocations. The
Peer route did not reconnect or publish application data. Local Peer
reads returned the exact `peer_unavailable` response. A fresh foreground
run preserved an unrelated inherited descriptor and drained its
exact Peer socket on SIGTERM. The installed layout then reopened its
SQLCipher binding through the stock owner path in the test controller.
The gate does not add Peer startup to the user-facing stock launchers.

| Observed milestone | Author run | Claude run |
| --- | ---: | ---: |
| Test wall time | 33.29 s | 32.66 s |
| Child to provisional Ready | 6,258 ms | 6,300 ms |
| Intentional hold before ACK | 2,001 ms | 2,001 ms |
| ACK to authenticated Peer subscription | 189 ms | 162 ms |
| Ordinary Command with Peer active | 5,871 ms | 4,805 ms |
| Ordinary Command after Peer disconnect | 1,623 ms | 1,789 ms |
| Foreground stop and exact socket closure | 78 ms | 52 ms |

The columns are individual private fixture samples. Command measurements include
native event delivery and result collection. They do not estimate
public relay or SSH performance.

Strict all-target Clippy passed on Linux AMD64 for stock in 32.71 s
and the candidate in 30.01 s. The exact candidate executable passed the
registration kernel-path test, panic-before-join test and private
argument test. The isolated actor subprocess suite passed in 50.96 s.

| Installed artifact | SHA-256 |
| --- | --- |
| Manifest | `0d861f8d7dc99ccea40fc17eae92c9856219693399210919dbba78f2ba5efe80` |
| Daemon | `c66450d7194a6506699efedd3961f85751e2c260e31f0aa4874787ff478b3a98` |
| Command worker | `1e02e8bae8b275281fc2f201ea02d281ff6882844d6604d5081cc3194c8d6a44` |
| Qualification example | `d5a042664af68d9aa2c42b3f9849ec3fe6ae98ea2c7246f63e848fdab7d7ffb4` |
| Native test runner | `b494b89b35db304b0d200f7afa80b54760337442e4ccb5e4961207d6e163956b` |

[Independent review](../reviews/peer-candidate-recipient-lifecycle.md).

## Dispatcher-loss repetitions

The integrated installed selector passed both pre-Commit and live
Status cuts in 27.80 s. The first independent run returned an
unclassified refusal before the snapshot frame. Two subsequent attempts
passed. The author repeated the same executable serially six times.
All six runs passed in 26.13 to 26.89 s.

Test-only source `e77664a` reports fixed refusal categories while keeping
the same cut, comparison and completion requirements. That test runner
uses the same native source stamp as the unchanged production artifacts.
Two parallel campaigns of five runs each passed all twenty fault cases
with two cases per run. Suite times were 26.53 to 27.30 s. The isolated
failure was not reproduced, so the initial refusal's cause remains
unknown.
