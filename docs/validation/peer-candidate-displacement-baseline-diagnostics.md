# Validation of the installed dispatcher baseline diagnostics

Validation used Linux AMD64, disposable file-backed profiles and keys, and separate private WSS origins for Peer qualification and ordinary Command. Candidate and stock archives were installed locally against their exact manifests. All runs used the existing Ready and Status cases.

| Source and run | Ready | Status | Result |
| --- | ---: | ---: | --- |
| Original fixture, first run | 13,101 ms | controller header EOF | The shared reader could have been waiting for S or P. It did not inspect the child. |
| Original fixture, confirmation | 12,597 ms | 12,610 ms | Both cases passed in 25.21 s. |
| Diagnostic source `ff2ff4a`, untraced | 13,219 ms | S header EOF, child signal 6, stale socket reset | The parent had completed R/A/C and the first active Status, then sent the second request. It observed neither an S header nor an allow-listed child event. The run ended in 24.53 s. |
| Same installed `ff2ff4a`, one signal-traced confirmation | 50,010 ms | 49,517 ms | Both cases passed in 99.53 s. The trace observed no SIGABRT and substantially slowed the test. |

SIGABRT identifies how the exact child ended during the observed failure. The evidence does not locate the abort within the second Status turn or establish whether the child sent the signal to itself. The earlier categorized X is a different observation. The three-serial-run acceptance gate remains incomplete, and the paired displacement fixture remains paused. No production change, macOS result, or failure-rate estimate follows from these runs.

Candidate and stock strict all-target Clippy and local manifest installations passed on `ff2ff4a`. That exact candidate binary passed five diagnostic tests and invoked the one child-only ignored selector through its parent test.

Follow-up source `fb1d017` replaced Tokio-readiness sampling with a non-consuming kernel socket peek and added checks for malformed Status results, categorized second-Status degradation, and preservation of the direct-child guard on observation failures. Its exact candidate library linked in 43.92 s and passed nine diagnostic tests in 0.09 s. The two listed child-only ignored selectors were exercised by their parent tests. The intermediate `0645ae1` test failed when an immediate Tokio peer drop still appeared pending. The final fixture synchronizes FIN explicitly and uses a synchronously closed standard Unix peer to exercise reset.

On `fb1d017`, strict `cargo clippy --locked -p marmish-native
--all-targets -- -D warnings` passed on stock in 29.67 s and on the
candidate target in 30.62 s. The candidate used
`--target x86_64-unknown-linux-gnu` with the private candidate Rust
configuration. All compiles used the matching source stamp, offline
dependencies, two jobs, and disabled debug info and incremental state.
Candidate test archive SHA-256
`30d3ece814e37e2bd1f2d59dc9f59232fe691322e5e5b341bd199959d76d81ad`.
The [independent review](../reviews/peer-candidate-displacement-baseline-diagnostics.md)
also passed this archive. Receipt-installed live runs above used
`ff2ff4a`; the later kernel-peek and fixture changes were validated
with the focused test archive and strict checks.
