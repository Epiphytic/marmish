# Terminal presentation and transport measurement

This phase follows the [interactive roadmap](interactive-peers-roadmap.md)
and Beads `marmish-dwt.2`. The [lossless Terminal increment](terminal-frames.md)
ships independently. Begin the measurement work on the reviewed integration
from `marmish-dwt.1`, with authenticated attachment, ephemeral delivery and
fresh redraw after both orderly detach and lost relay detach. Use the pinned
MDK route and a bounded disposable relay. Writer-only byte packing can be
evaluated with one admitted writer. Testing a negotiated screen mode also
requires the roadmap's reattach, observe and takeover gates.

## What to reuse

Claude Chill's pinned local PTY proxy runs a VT screen model and emits
coalesced updates with `contents_diff`, `contents_formatted` and
`cursor_state_formatted`. It updates its diff base after a successful local
write, treats the alternate screen separately and maintains lookback history.
Its separately exported `RedrawThrottler` replaces one pending vector when a
new redraw arrives. It supplies neither a byte limit nor an acknowledged
screen base, and the pinned proxy does not call it. Overwriting pending
output on Marmish's asynchronous route could lose ordered escape controls,
stream text or a required diff base. Reuse the idea of consolidating bursts
through the existing lossless path. Keep the Claude Chill and Marmot README
credits, with the pinned MIT notice required if any source is copied in a
later phase. This phase adds no source or dependency from Claude Chill. A measured
comparison against its screen renderer remains part of the future screen
mode evaluation.
[Pinned proxy](https://github.com/davidbeesley/claude-chill/blob/66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9/crates/claude-chill/src/proxy.rs)
[Pinned throttler](https://github.com/davidbeesley/claude-chill/blob/66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9/crates/claude-chill/src/redraw_throttler.rs)
[Pinned MIT license](https://github.com/davidbeesley/claude-chill/blob/66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9/LICENSE)

| Reviewed component | Existing contract |
| --- | --- |
| [Native tmux backend](../../crates/marmish-native/src/terminal_backend.rs) and [PTY dispatcher](../../crates/marmish-native/src/terminal_live_supervisor_io.rs) | The worker reads and writes the actual attachment PTY. The current supervisor stage collects up to 4,096 raw bytes and moves the whole stage at the first-byte 8 ms deadline or when full, subject to its output slot and authority. A raw PTY read or stage can end mid-control or mid-code point. |
| [Safe output batches](../../crates/marmish-native/src/terminal_output_batches.rs) | `Batches` is a tested safe-unit seam with a 4,096-byte batch limit, one overflow unit and an 8 ms flush clock. It consumes fragmented source bytes. Its only callers in the reviewed baseline are its tests. Verify the final .1 owner and timer composition before proposing a change. |
| [Display filter](../../crates/marmish-native/src/terminal_output.rs) and [foreground display](../../crates/marmish-native/src/terminal_frontend_display.rs) | The bounded parser accepts complete permitted display units and drops disallowed operations. `Display::record_committed` requires parser ground state and drained writes before receiving credit for an IPC output record. A non-ground record boundary fails closed. |
| [Terminal delivery](terminal-frames.md) and [socket controller](../validation/terminal-socket-controller.md) | Native sequence, authenticated age, lease and fence remain authoritative. Output receipts follow actual local TTY writes or an explicit discard under the display policy. Close, Detach, and partial framing retain their established ordering. |
| [TTY frontend](terminal-frontend-tty.md) | The dedicated foreground owner opens the controlling TTY, enters raw mode, applies bounded polling and exact signal and resize handling, and restores display state and saved termios. |

Losslessness here means preserving ordered input and the baseline's
permitted output bytes and effects. Malformed UTF-8 and forbidden ANSI
controls keep the same disposition as in the reviewed filters. A PTY read
or partial socket write may split UTF-8 and escape sequences. The output
record that the frontend commits must end at a complete safe unit. No
packing step computes glyph widths, normalizes Unicode, synthesizes ANSI or
holds a text fragment waiting for a redraw marker. Wide and combining
Unicode, cursor movement, SGR color, alternate-screen and bracketed-paste
transitions are preserved in baseline order. The application continues to
own screen state through tmux. The existing policy continues to handle
clipboard, terminal queries, title and window operations. The client keeps
its independently enforced filter and raw-mode restoration.

## Measure the actual dispatch before changing it

After .1 lands, add its reviewed commit and published qualification to this
design before implementation. Identify the actual safe-unit producer, queue
and age clock, eligibility time and output cap, publication permit and single
unresolved room send slot, successful handoff/commit point, and ordered
control, close and fence barrier owners. Identify who consumes raw PTY bytes
into complete safe units, whether the integrated path wires in `Batches`,
and where filtering first happens. That placement affects wire-byte counts.
Baseline equivalence uses the permitted ordered output and display effects,
including when two implementations discard forbidden bytes at different
points. Compare traffic only against the same reviewed .1 filter placement.
Pass the mapping and an exact test selector to Claude. Record the effective
limits from the running baseline.
Some earlier plans describe adaptive 8–24 ms preparation and a 32 ms
ceiling. The current local PTY dispatcher implements a fixed 8 ms timer.
The experiment preserves whichever bound the reviewed integrated path uses.
Adding or resetting a timer to collect more output would invalidate the
comparison.

Start with an opt-in fixture and aggregate instrumentation. Use `cfg(test)`
for hooks internal to unit tests. Integration tests and the instrumented
installed peers need a non-default feature such as
`terminal-presentation-bench`. Compile those hooks out of standard distribution builds.
Build the benchmark peers through the normal installer and label the
instrumented artifact in the report. At each eligible publication turn count
oldest-byte age, available contiguous safe-unit bytes and size handed off.
Capture ordered unit lengths, their availability times, barriers, the
proved cap and permit/slot opportunities from the real tmux burst,
redraw, scroll and echo fixtures running the reviewed .1 implementation.
Use only length and timing metadata from the fixture in bounded test
memory. Replay that trace under the same release times and barriers to
estimate legal packing. A whole unit must fit and already be eligible.
Keep fabricated traces for deterministic boundary tests. The trace gives
an estimate under the original schedule. It cannot model how dispatch
changes later permit and ACK timing. The paired actual runs decide whether
traffic falls. Publish only aggregates and sample counts. Persist
and log no payload.

The 4,096-byte native cap already matches the local stage size. A blocked
supervisor slot lets that raw stage grow to its cap because the owner
keeps reading while space remains. At most one cap-sized record can be
sent at a time and only eligible sub-full records can benefit. A complete
unit that straddles the cap can reduce the usable size. Even with ideal
eligibility, a span between barriers with `B` permitted bytes needs at
least `ceil(B / 4096)` frames. Sum that floor across spans as an optimistic
bound on savings, and use the real release/slot trace for the stricter
estimate. An additional queue, larger frame or changes to sealed-record
boundaries are outside this phase. The existing stage may already capture
most of the opportunity. A measured null result completes this evaluation.

Stop before implementing a candidate if even the optimistic frame floor
rules out a 15% reduction on the fixed burst workload. Otherwise proceed
only if the more restrictive replay estimate also reaches 15%. If baseline behavior already
fills the available prefix or packing is blocked by the safe-unit limit,
publish that result and keep the existing path. The likely small change,
if justified, is in the reviewed send owner. When its existing conditions
permit dispatch, fill the ordinary v1 Output from the longest available
contiguous prefix of complete permitted units that fits. Use existing bounded
staging and count all held bytes, including parser carry or an overflow
unit, against the corresponding budgets. Preserve the oldest timestamp and
applicable deadlines, binding, generation, epoch and author. Honor control,
resize, Close and Detach barriers and priority. A speculative assembly leaves
the cursor intact until the existing handoff commits. Transient refusal
retains the same accepted prefix and deadlines. A sealed record stays
immutable for permitted retransmission. Follow existing fencing and fresh
redraw after stale or ambiguous progress. This experiment leaves input
pacing, wire schema, local acknowledgement, replay rules, confidentiality
and persistence as defined by their existing owners.

## Correctness and measurement fixtures

1. **Deterministic byte and boundary tests.** Drive the reviewed .1
   dispatcher and any candidate with identical scripted reads and a
   virtual clock. Reuse `Batches` unit tests for its local contract. Vary
   partitions at raw reads and socket writes, including one-byte and
   near-cap splits. Check every committed record ends in ground state.
   Compare the exact concatenated allowed bytes and required mode effects.
   Exercise wide and combining UTF-8, malformed bytes, colors, cursor and
   alternate-screen transitions, bracketed paste, fragmented discarded
   controls, and unterminated or over-limit control strings. Require the
   actual foreground filter to enforce its query, clipboard and window
   policy even on direct untrusted IPC bytes. Include blocked/partial TTY
   writes, delayed permits, full queues, continuous output across the
   deadline, resize/control order, gap, duplicate, expiry, epoch change,
   revocation, detach and fresh reattach. Assert no Ack or native output
   commit before the complete permitted local write. Exercise ordinary
   v1 decoding using unchanged peer code.
2. **Measure real PTY to foreground display.** Use a direct tmux attach to
   a controlling TTY as the local anchor, without any Marmish process or
   filtering. Use the same tmux configuration, dimensions, terminal type,
   harness clock and benign workload as in the following local Marmish
   path. Report each path and paired Marmish-added key-to-visible latency.
   This anchor applies only to the local harness. Apply the candidate
   performance gates against baseline Marmish in each matching harness.
   Run hostile control cases only in the filtered harness. Extend the reviewed
   [local tmux fixtures](../../crates/marmish-native/tests/terminal_local_tmux.rs)
   with a bounded marked workload through the actual worker and
   `tmux` pane, framed IPC, frontend parser and controlling TTY. Run
   one key-to-visible test from a synthetic key written to the client's TTY
   master until its output marker is read there, and one output-only
   measurement from the first matching read of the server attachment PTY
   until the full marker is observed on the client master. Detect markers
   across read boundaries. Use one monotonic clock in the same-host
   harness, explicitly excluding instrumentation setup. Observation at the
   client master proves bytes reached the local TTY output stream. For
   visual state check typed cells, cursor and active buffer at fixture
   checkpoints with a specified test-only xterm-compatible emulator or
   fixed expected sequences plus before/after screen assertions on a
   named real emulator. Record the actual oracle. PTY observation alone
   does not measure emulator paint time. Verify normal-screen restoration,
   fresh redraw with correct geometry, resize and saved termios for close,
   detach and a TTY signal.
3. **Measure signed traffic and end-to-end latency.** Repeat through two
   installed peers and the disposable bounded relay on the same host for
   comparable timing. Set bounded timeouts and rate and capture the
   number of distinct application Output handoffs and signed event IDs
   at relay ingress, alongside total signed event submissions (including
   repeat sends), retransmissions, ingress payload bytes and fanout payload
   bytes as separate columns. Count actual serialized WebSocket message
   payload bytes at the relay. Correlate aggregate per-type counts using
   test instrumentation at the authenticated application owner. Separate
   server-to-client Output traffic, control/Ack and total bidirectional
   traffic. Exclude setup from warm samples and report it separately. For
   traffic volume identify counted legs and measurement layer,
   excluding TLS or TCP overhead unless separately measured. Use simulated
   key echo, fixed burst redraw, repeated synchronized redraw, full-screen
   interaction, resize and fixed 256 KiB scrolling under the approved
   limits. Publish latency, goodput, TTY write count, queue peaks, failures
   and server CPU. Include mixed ordinary Command/Stream/Copy and rsync
   regression checks, with observer isolation measured only after its
   separate owner is qualified.

Use fixed workload versions and sizes, geometry, terminal profile, transport
settings and pacing. Establish variation with at least five paired runs of
baseline against itself, then at least five interleaved baseline/candidate
pairs per qualifying platform. Publish per-run p50/p95 for runs of at least 100 warm successes per
latency workload, all paired differences and the median paired change.
Use p99 only with at least 1,000 warm successful samples per cohort. Report
small or cold cohorts with explicit counts. Report failures and detach
counts alongside successful samples. Require at least 15% median fewer
Output frames on the burst and median reductions in signed event
submissions and wire bytes each exceeding their measured noise.
Require correctness parity and zero unexpected timeouts, detaches or
correctness failures in qualifying runs.

For each metric, estimate noise as the largest absolute difference between
paired baseline runs. Compare baseline and candidate by the median paired
difference of per-run p95 latency or goodput. Reject an increase in idle
echo, burst or mixed Command p95, or a decrease in scrolling goodput,
when the median regression exceeds the corresponding noise. Apply the
same mixed latency gate to slow-observer testing once that owner is
available. For latency, noise must fit both 5% of the median baseline
p95 and 5 ms. For goodput it must fit 5% of the median baseline goodput.
If either cohort is noisier, repeat the cohort at most twice under
controlled conditions. Report continued excessive variation as
inconclusive and retain the existing path. Write sanitized measurements under `docs/validation/` and
obtain Claude Code auto review before any product activation.

Keep only deterministic byte and fence smoke checks in routine self-hosted
Linux AMD64 CI, with the roadmap's under-one-minute warm core target.
Installed runs, tmux, benchmarks and timing remain opt-in. Qualify Linux
AMD64, Linux ARM64 and macOS ARM64 natively before activating the candidate
on all supported platforms. Run native qualification sequentially on shared
capacity.

## Future screen mode

A clear-screen command or synchronized-output marker does not make a
replacement checkpoint. Scrollback, saved and alternate screens, margins,
mode changes, Unicode width and combining behavior, bells and terminal
queries can escape a cell-grid model. A separate reviewed opt-in protocol
would need authenticated capabilities; explicitly bounded VT semantics and
storage; an independently reconstructable geometry- and generation-bound
base; complete, acknowledged local apply; and recovery through a fresh
lossless redraw on ambiguity or unsupported behavior. A dependent diff
would need exactly the receiver's acknowledged base. Only unsealed
independently recoverable checkpoint candidates could be superseded.
No screen-mode activation is proposed here.
