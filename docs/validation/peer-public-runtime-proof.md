# Public MDK runtime Peer custody proof

Four opt-in tests exercised the [public runtime proof design](../phases/peer-public-runtime-proof.md)
on Linux x86-64 with Rust 1.97.1, using temporary SQLCipher databases and
disposable keys. The only network endpoint used was a loopback WebSocket.
The default test run ignores this target's four scenarios.

The run used Marmish base `5203e4fb397bb86f8054c0f364d3c66d6a9e6578` and MDK
`8abc51befbf71a1ae3bdbb25eb0860c731e75db2`. A clean archive received the
named integration test and this single manifest change under
`[dev-dependencies]` in `crates/marmish-native/Cargo.toml`:

```diff
-marmot-session = { git = "https://github.com/marmot-protocol/mdk", rev = "8abc51befbf71a1ae3bdbb25eb0860c731e75db2", features = ["test-support"] }
+marmot-session = { git = "https://github.com/marmot-protocol/mdk", rev = "8abc51befbf71a1ae3bdbb25eb0860c731e75db2" }
```

```sh
snapshot=$(mktemp -d)
base=5203e4fb397bb86f8054c0f364d3c66d6a9e6578
git archive "$base" | tar -x -C "$snapshot"
cp crates/marmish-native/tests/peer_public_runtime.rs "$snapshot/crates/marmish-native/tests/"
sed 's/, features = \["test-support"\]//' \
  "$snapshot/crates/marmish-native/Cargo.toml" > "$snapshot/native-manifest"
mv "$snapshot/native-manifest" "$snapshot/crates/marmish-native/Cargo.toml"

cargo tree --manifest-path "$snapshot/Cargo.toml" --locked --offline \
  -p marmish-native -e features -i marmot-session
CARGO_TARGET_DIR="$PWD/target" MARMISH_BUILD_COMMIT="$base" \
  cargo test --manifest-path "$snapshot/Cargo.toml" --locked --offline \
  -p marmish-native --test peer_public_runtime -- --ignored --nocapture --test-threads=1
CARGO_TARGET_DIR="$PWD/target" MARMISH_BUILD_COMMIT="$base" \
  cargo clippy --manifest-path "$snapshot/Cargo.toml" --locked --offline \
  -p marmish-native --test peer_public_runtime -- -D warnings
```

The reverse feature tree showed `marmot-session v0.15.0-dev` at `8abc51be`
through `marmot-session feature "default"` and the normal and development
edges from `marmish-native`. It showed no `test-support` edge. Strict Clippy
passed on the same snapshot. All four tests passed in 71.45 seconds.

| Scenario | Observed result |
| --- | --- |
| Public ingest and durable recovery | Authenticated the carrier, extracted the exact signed Welcome, and checked the approved package ID and relay hints with the pinned peeler. Wrong destination, changed relay hints and wrong pending package were rejected. The exact candidate produced one expected `GroupJoined` and every other effects field was at its default. The ingest reported no unpersisted object. Public getters confirmed purpose, Current profile, active local leaf, exact two-account roster, founder administration, route and consumed package. Restart with the stage file absent left only Candidate and created no replacement database. Restoring the original file recovered the same authority. The stage and registry reopened before Ready, after Ready and before receipt deletion, and after deletion. Exact deletion preserved a synthetic unrelated receipt. Swapping the provider key and tampering with the Ready MAC failed. The same-account ordinary provider retained its independent group, epoch, route, lifecycle and pending receipt, and had neither a message nor a dedup marker for any of the stage inputs. |
| MLS founder and roster checks | A valid outer sender rewrapped a Welcome from a different MLS founder. The actual durable `welcomer` reflected the MLS signer, and the comparison refused the candidate. A separately staged three-leaf group passed signed ingest and failed the exact roster check. Neither negative triggered an I/O call. An independent same-account ordinary provider had neither a message nor a dedup marker for either rejected input, and its native group list, outbox and package lifecycle were unchanged. |
| Unexpected publish work | The public `FoundingGroupCreated` queue processed a real signed native Welcome. With an inbox route, the adapter refused the call and latched the publish attempt. The dispatcher returned success with a failure in its effects. Without an inbox route, the dispatcher emitted a routing failure for that exact message without calling the adapter. The allowlist refused both outputs. |
| Carrier acknowledgment and raw withdrawal | A loopback relay acknowledged the exact signed carrier. Its sealed registry entry bound that acknowledgment to the stage, native raw Welcome and route. The independent cancellation had its own sealed disposition. Reopening the founder before withdrawal retained the three owed messages. Reopening after withdrawal, before transport activation, returned `AlreadyWithdrawn` for both entries. Fresh startup retry sent only the undisposed positive control. An explicit stale retry of both withdrawn messages produced no relay publication. The relay observed exactly the carrier and positive control. |

The wrong-provider test intentionally elicited SQLCipher page authentication
diagnostics. No test emitted identities, key material or database contents.

| Local measurement | Observed |
| --- | ---: |
| Exact Welcome ingest through durable receipt and authority verification | 104.60 ms |
| Inbound scenario including setup and restarts | 18,169.75 ms |
| Exact carrier acknowledgment on loopback | 3.64 ms |
| Loopback scenario including setup and restarts | 15,616.91 ms |

The fixture drives stage inputs through separate public runtime handles and
measures ordinary storage alongside them. Production isolation still requires
testing that a shared account inbox dispatches each input to the correct store.
The sealed Candidate records the wrapper digest, carrier ID, package and route
for recovery. The fixture enforces carrier authentication, exact Welcome bytes,
package and relay hints at its handoff and native authority after ingest. It
round-trips the registry fields without using the complete record as a single
forwarding authorization check. Restart was exercised with the stage file
absent. Lock contention remains untested.

The public dispatcher negative injects work already signed by the native
founder. Public signed intake ran the same dispatcher on the fresh isolated
candidate and produced no network attempt. The tests did not induce
`AutoPublish` or an automatic proposal from that initial Welcome. A realistic
signed intake that queues unexpected publish work remains a production gate.
Relay carrier acknowledgment establishes only relay acceptance. The native
disposition is withdrawal. Aggregate routing, production admission, stage
limits, key custody, Tor and additional operating systems remain outside this
proof.
