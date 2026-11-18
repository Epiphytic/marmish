# Terminal redraw feasibility

Beads `marmish-dwt.2` follows the
[lossless Terminal rendering design](terminal-renderer.md). Measure whether
screen diffs justify a separate opt-in design. Start with a test-only replay
of generated redraws. This increment has no installed renderer, feature
switch, negotiation, wire change, dependency, or external relay traffic.
The earlier lossless packing experiment and its acceptance gates still apply.

Claude Chill models a VT screen and emits a diff from the last displayed
screen. Its upstream local proxy also recognizes synchronized-output
boundaries. These boundaries tell an emulator when to present accumulated
changes. They do not reduce bytes or confer delivery authority by themselves.
Keep the existing README credits. This experiment copies no upstream code.
[Pinned upstream proxy](https://github.com/davidbeesley/claude-chill/blob/66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9/crates/claude-chill/src/proxy.rs)
[Pinned upstream license](https://github.com/davidbeesley/claude-chill/blob/66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9/LICENSE)

## Offline comparison

- Use a fixed generated workload and sizes. Include sparse repeated redraws,
  dense repaints, synchronized redraws with repeated writes to the same
  cells, ordinary echo, and scrolling. Use a common initial screen and
  explicit final cursor. Only test-generated typed writes into a normal
  80×24 screen with default rendition and printable single-width ASCII
  are eligible for replacement. Restrict writes to absolute cursor
  positions and leave the last column untouched to avoid wrap state.
  Include cells changing to spaces and cursor-only changes. Generate
  baseline bytes from the ordered writes. Generate comparison bytes from
  cell runs that differ from the independently expected screen and cursor
  at the immediately preceding checkpoint in that window, with exact
  final cursor restoration. The base advances after each checkpoint,
  including eligible typed updates that fell back to ordinary bytes. Cost
  the initial full paint separately in both arms and include it in the
  total and decision denominator. Freeze workload mix, seed and fixture
  version before measuring.
- Drive baseline `Batches` with scripted consumed-input times and
  partitions. Validate every resulting batch using
  `Filter::validate_batch`. A baseline batch can be replaced only when
  it consists entirely of one completed typed update. For each eligible
  update assert that concatenated filtered baseline bytes equal its
  typed serialization with the two optional markers removed.
  `Filter` drops DEC synchronized-output `CSI ?2026 h/l` today. Pin
  this fact in a test. The independently generated typed boundaries
  own comparison readiness.
- Push the comparison bytes to a separate `Batches` at the virtual
  consume time of the last raw byte of that update, including any close
  marker. Use one push by default, or repartition the comparison bytes
  into one-byte and near-cap slices at that same virtual timestamp for
  partition tests. Computation inside this model costs zero virtual
  time. Count a timed substitution only if ordinary `ready` and `take`
  produce one validated complete record by the baseline's first
  release opportunity and the comparison can use that exact opportunity.
  No forced flush or changed clock. A no-op can remove that single record
  only with identical expected grid and cursor and no additional
  effects. Fall back across any fixture boundary, size or deadline
  violation. Cap one held update at 4,096 raw bytes and 8 ms from its first
  raw byte. The live supervisor has a separate first-byte 8 ms stage.
  The replay models only the subsequent `Batches` seam and a separate
  idealized checkpoint comparison omits release eligibility. Neither
  measures installed latency.
- Any unsupported or opaque fixture span always runs the original bytes
  through the baseline policy, with output, release sequence and error
  checked against baseline. Exercise Unicode and invalid UTF-8, scrolling
  and wrapping, alternate screen and SGR, bracketed paste, terminal queries,
  OSC 8 links, OSC 52 clipboard, title/window controls, fragmented or
  incomplete markers, and embedded control strings. Resume comparison
  only at a fresh pane initialized as described below. Treat tick, close,
  resize, attach, observer and takeover boundaries as ends of the modeled
  comparison window. Do not replay attachment handoff or infer a shared
  writer/observer display base from a fixture grid.

## Display oracle and evidence

Inject the exact replayer batches into separate fresh panes of a private
tmux server and record its version. The pane test child uses raw output
with `OPOST` cleared, and reads an ordered byte stream from a private local
Unix socket. Use a fresh normal 80×24 pane for each window. From that
clean pane, explicitly initialize default rendition and ASCII G0, normal
origin and replace mode, full margins, wrap enabled, visible cursor,
grid and starting cursor. Assert the measured dimensions, normal screen
and cursor visibility. No reset of unknown VT state is modeled.

Poll `capture-pane` without line joining and read cursor coordinates.
Pad only trimmed trailing blanks to the known width and preserve row
boundaries. Both panes must independently match the fixture's expected
grid and cursor. Follow the exact stream with a fixed cursor-relative
probe carrying a new printable marker and check the post-probe state
under the same deadline. The probe traverses the same ordered socket
and pane PTY, and its unique marker prevents a stale snapshot from
passing even a no-op checkpoint. Restore its cell and cursor using
fixture controls before continuing the window. Exclude probe and
restore bytes from replay counts. Verify that a wrong cell and a wrong
cursor each fail the oracle. Unsupported controls are validated against
the baseline filter and ordering. A pane snapshot measures cell and
cursor continuation equivalence in this subset and contains no GUI paint
timing evidence.

Use a finite fixed workload (at most 128 checkpoints and 512 KiB per
run), disposable sockets, and only volatile generated bytes and expected
screens. Stop on an oracle timeout and remove only the test-owned tmux
server. Publish only aggregate results and categorical failures. For each
workload report eligible/total and fallback reason counts, permitted
payload bytes, ordered Output frame counts, and serialized Marmot app
bytes using the existing `terminal_frames::encode_app`. Include fallback
and the initial paint in overall totals. Distinguish timed replay and
idealized comparison, and report workload dimensions and sample counts.
These are local app-layer observations. Do not label them signed event
counts, encrypted wire bytes, or physical display latency.

A null or inconclusive result completes the feasibility proof. A
follow-on design request requires fidelity at every eligible checkpoint,
exact fallback, and at least 15% fewer application bytes in the timed
comparison on the fixed mixed redraw workload including fallback.
Separately report whether frame counts changed. Substituting one record
usually changes only bytes. The earlier lossless packing criterion of
15% fewer frames still governs changes to packing. A separately
reviewed installed experiment would pair the same workloads on private
loopback, count actual unique and repeated signed Output publications
and serialized WebSocket bytes by leg apart from setup and Ack/control,
and check echo, cold first render, scrolling, and slow observers using
the paired noise and percentile gates in the
[rendering design](terminal-renderer.md). The
[installed observer baseline](../validation/terminal-observer-benchmark.md)
records 20 echo samples per role and 321 accepted relay publications
across the whole run. It supplies context, separate from the paired
signed Output measurement.

Any later live substitution needs negotiated capabilities, separate
geometry-, generation- and attachment-bound bases with positive local
TTY commit evidence, and bounded volatile state. Resolve ambiguity or
partial commit through the existing fence and fresh lossless tmux redraw.
Keep the original approved stream path for generic PTY output, input,
resize, observer/takeover, and terminal policy. A standalone RedrawThrottler
that replaces its pending byte vector does not satisfy that contract.
[Pinned upstream throttler](https://github.com/davidbeesley/claude-chill/blob/66cc2ea0b8920d54c403d6bc1f8ada58e7ca7ae9/crates/claude-chill/src/redraw_throttler.rs)

## Implementation boundary

After design approval and coordination, ownership is one test-only
`terminal_redraw_evaluation_tests.rs` module with a `#[cfg(test)]` mount
in `terminal_output_batches.rs`, and a test-only
`terminal_redraw_tmux_oracle.rs` child of that module. Run both replay and
oracle in the same `--lib` test binary, keeping `Batches` and `encode_app`
crate-private. The driver follows the existing local tmux fixture's
private 0700 root, absolute explicit tmux binary, foreground `-D`,
`-S` and empty config, owned child cleanup, and bounded socket readiness.
Its injector invokes that same test binary with the exact ignored test
selector. Reuse helpers already visible to the library test crate when
available and keep the independent `terminal_local_tmux` integration
target untouched.

The only core-eligible selector is `terminal_redraw_evaluation_core`
for deterministic byte and fallback smoke. The
`terminal_redraw_evaluation_tmux` oracle and reporting selector stays
ignored and opt-in on a supported native host with explicit tmux.
Publish aggregate evidence under `docs/validation/`. The phase changes
no shipping terminal path or distribution artifacts.
