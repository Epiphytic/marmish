# Manual recipient native deadline window validation

Exact source `de4c852cae819ce391f6153d3fd6db68f1a343c3` was compiled on Linux
AMD64 with the private candidate cfg and a locked offline dependency graph.
The scoped Cargo libtest build took 40.24 seconds. The archived executable
SHA-256 was
`d8d260aaa1dbe7c191fd7f0b0d65ef2de7b0053925717c0e64c27005ff50d453`.
The opt-in `peer_registered_native_window_private_wss` ran one harness with
four fresh subprocess cases in 16.58 seconds against generated private
WSS credentials. All passed.

The registered path verifies the scope's account and origin and clamps the
signed end by the live route's earlier TLS or qualification end. The cases
exercise the paired fatal latch, registration cancellation, absolute
monotonic cutoff, and a test-only route lacking a fatal latch. The tests
observe zero publication attempts and distinguish process fatal from
registration cancellation. Eight separate deterministic fence and startup
cases passed under the independent reviewer.

The qualified window is an input to the future actor. This evidence covers
the native-window boundary on Linux AMD64. The mounted receiver,
latency and other platforms remain for subsequent gates.
