# Terminal relay physical cut validation

Source `91061a44e94c442191f4a6a54e8be34b7cefd61e` was built offline from the
locked workspace on Linux AMD64. The stock debug libtest SHA-256 was
`180f2aa29bbfb43f560d07a73cbc4c4b1c0b4348e58463c7c4c27a62678632cf`.
The build took 44.36 seconds. The tests used disposable identities and loopback
private TLS WSS fixtures.

| Libtest selector | Result | Elapsed |
| --- | --- | --- |
| `terminal_fatal --include-ignored --test-threads=1` | 5 passed | 0.22 s |
| `dispatcher_fatal::tests --test-threads=1` | 3 passed | <0.01 s |
| `transport::tests --test-threads=1` | 18 passed | 0.01 s |
| `mdk_adapter::terminal_enrollment --test-threads=1` | 15 passed, 2 opt-in cases counted above | 0.19 s |
| `mdk_adapter::terminal_live_enrollment --test-threads=1` | 9 passed | <0.01 s |

The live Lane case opened two separately scoped signed subscriptions, then
observed raw EOF for both exact relay connections and a clean reaper. No
protocol CLOSE was initiated before the cut or observed on the wire. The cut
blocks new transport polls. Bytes previously forwarded to the kernel may
still arrive. The held-halves case uses a real fixture-owned socket to occupy
the SDK dial slot and retains sink and stream separately. It exercises pending
SDK dial cancellation and requires both halves to drop before the route reaps.

The deterministic tests observed the actual registered wake and pending future
drop before a second deactivation poll. Canceling a pending Lane close kept its
pump join counted until the pump exited. A guarded connector refused the native
Room constructor before a new socket or relay request.

The stock failed-bootstrap path now retires its authority and uses the same
Lane cleanup as later failures. Successful stock startup and the public relay
and Terminal privacy settings are unchanged. The candidate path remains
unmounted. These results cover the physical relay Lane only. Full daemon stop,
queued native work, SQLCipher custody and tmux lifecycle require separate proof.

`cargo fmt --all -- --check` and `git diff --check` passed on the exact source.
Strict all-targets Clippy passed for the initial stock implementation `5e512f2`
and for the candidate wake-witness revision `f8e07eb`. The final Room-scope
and pinned-borrow changes were rebuilt and exercised on stock Linux AMD64.
Exact composed stock and candidate lint is tracked in the integration phase.

An additional run of `terminal_selection_native_wss_echo_and_resize` in a
Cargo-shaped debug fixture exposed a 32 MiB stack overflow during the first
Command relay connect at `1a6e6da`. The pinned-borrow change removed a second
owned copy of the operation from the cancellation future. The exact final
debug run passed the previous stack-overflow boundary and reached Command
`not_started` at 20.07 seconds, the same Command outcome seen on two
pre-change debug binaries with the same older worker. This run did not
reach Terminal echo or resizing. The full fixture needs a source-matched
worker in the integration build.
