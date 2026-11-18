# Isolated Peer Welcome custody proof

The [staged inspection design](../phases/peer-staged-inspection.md) was exercised
with three opt-in native tests against MDK
`8abc51befbf71a1ae3bdbb25eb0860c731e75db2`. They used temporary SQLCipher
providers, disposable account keys and a loopback relay. The tests are ignored
by default.

```sh
MARMISH_BUILD_COMMIT="$(git rev-parse HEAD)" \
  cargo test --locked --offline -p marmish-native --lib \
  peer_staged_proof_tests -- --ignored --nocapture --test-threads=1
```

All three tests passed on Linux x86-64 with Rust 1.97.1 in a debug build.
The shared worktree run took 42.73 seconds, while an earlier clean snapshot
run with the same three scenarios took 29.94 seconds.

| Test | Observed result |
| --- | --- |
| Pending package and provider reopen | The signed, never-published pending package produced a valid native Welcome. A signed package-ID change was rejected by native ingest without consumption. Invalid and duplicate relay tags and a wrong recipient failed the native peel. A valid but different relay set failed the fixture's comparison. The untouched candidate joined with an active leaf and the exact package reference consumed. The registry and provider were reopened after join and before Ready, after Ready and before receipt deletion, and after deletion. A bad registry MAC and a mismatched provider failed the fixture checks. |
| Outer sender and extra roster | A rewrapped candidate authenticated its transport sender. Native ingest attributed the Welcome to the actual MLS founder, which the fixture compared to the offered sender and refused. A three-leaf room joined the separate encrypted stage and failed the exact two-leaf check there. |
| Outbound loopback recovery | The loopback relay acknowledged the signed Delivery carrier. The fixture reopened the founder after recording that acknowledgment and before settling the native raw obligation. After completion and separate withdrawal, native startup retry published only the unrelated positive control. The loopback relay observed the exact carrier and the raw positive control. |

An established Command group encrypted and authenticated a real Command wire
status request before and after the Peer join. Its group identity, epoch,
route, client KeyPackage lifecycle and pending output were unchanged. The
Peer raw artifact referenced a package held only by the stage. That check used
fixture entry points. Command process authorization, a deployed ingress
classifier, and a raw Welcome aimed at an ordinary public KeyPackage remain
outside this evidence.

## Local measurements

These are observations from one serialized shared worktree run. The latency
timer starts before staging the pending package and includes a deliberate
reopen. The encrypted-file total is a single sample across the fixture's
directory, including SQLCipher sidecars. It does not establish the maximum
admissible slot count or peak file size.

| Measurement | Observed |
| --- | ---: |
| Pending package setup through registry Ready | 4,536 ms |
| Stage and registry reopen | 81 ms |
| Complete positive scenario | 12,868 ms |
| Encrypted fixture files | 13,689,480 bytes |

The suite uses MDK's test-only session accessor for inbound native processing
and exact Delivered marking. The first proof covers custody and ordering
under the chosen fixture inputs. The production-facing API, the full
Offer and Consent policy matrix, revocation recovery, multi-room resource
caps, macOS and Linux ARM64 runtime coverage and Tor remain open.
