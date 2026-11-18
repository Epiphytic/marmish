# Offline Terminal redraw feasibility results

On Linux AMD64, the fixed generated workload produced **17.3% fewer
serialized Marmot application bytes** with the existing batch deadline
and **3.8% fewer Output frames**. The same cohort's idealized comparison
produced 22.8% fewer application bytes. This measures the application
serialization in an offline replay. Installed traffic, signed relay
events, GUI paint time and remote latency remain unmeasured.

Each table cell gives **Output frames / permitted payload bytes /
serialized application bytes** using the normal Terminal encoder.

| Workload | Baseline | Timed comparison | Idealized comparison |
| --- | ---: | ---: | ---: |
| Sparse redraw | 14 / 532 / 4,824 | 13 / 306 / 4,238 | 13 / 240 / 4,154 |
| Dense repaint | 14 / 25,739 / 38,416 | 13 / 10,052 / 17,218 | 13 / 4,910 / 10,366 |
| Synchronized redraw | 14 / 1,936 / 6,696 | 13 / 1,018 / 5,166 | 13 / 760 / 4,818 |
| Initial paint subtotal | 3 / 192 / 1,134 | 3 / 192 / 1,134 | 3 / 192 / 1,134 |
| Echo | 12 / 168 / 3,768 | 12 / 168 / 3,768 | 12 / 168 / 3,768 |
| Scroll | 24 / 55,296 / 80,784 | 24 / 55,296 / 80,784 | 24 / 55,296 / 80,784 |
| **Mixed** | **78 / 83,671 / 134,488** | **75 / 66,840 / 111,174** | **75 / 61,374 / 103,890** |

The fixed `redraw-80x24-v1` corpus uses deterministic positions and cyclic
ASCII values without a random seed. It includes 12 programmed redraws
in each of three cohorts, plus one final exact no-op and one initial
full paint per cohort. Every fourth programmed redraw splits its final
raw byte onto a separate 2 ms arrival. Echo and scroll run 12 baseline
turns each. The mixed total includes 66 turns and 83,879 source bytes,
including all initialization and fallback. The initial-paint row is the
subtotal for three full paints already included in the redraw rows and the
mixed result.

In the timed arm, 27 turns substituted a candidate and three exact
no-ops accounted for the entire Output-frame reduction. Nine turns
fell back because the comparison batch would become ready after the
original. The experiment compares eligible candidates even when a
particular diff costs more than the original bytes.
Twenty-seven turns were baseline-only, including three full paints,
12 echo turns and 12 scroll turns. Each redraw cohort had nine
substitutions, one elision, three deadline fallbacks and its initial
baseline-only paint. The measured corpus had zero size or split-batch
refusals. Separate deterministic cases exercised both. This modeled
stream has the existing
8 ms safe batch flush rule. The idealized arm also counts comparisons
that missed the original release. The initial raw PTY staging clock
and dispatch delays remain outside the model. Each typed checkpoint
starts a fresh replay batcher after the preceding one drains. An opaque
two-update case exercises exact preservation through a shared baseline
batch. Checkpoint discovery and combining transformed updates were
outside the experiment.

## Fidelity and scope

The same library test fed exact replay frames for all three redraw arms
into a disposable private tmux 3.4 instance with fresh 80×24 panes. At
each of 42 pane checkpoints per arm, the panes matched the fixture's
expected screen and cursor and passed the relative-write continuation
probe. The oracle detected deliberately incorrect cell and cursor
injections. Echo and scroll were baseline-only traffic cohorts. The
deterministic check covers synchronized marker filtering, fragmented
input, size and timing refusal, cursor-only and space restoration,
UTF-8 and malformed input, unsupported modes, OSC and DCS,
queries, incomplete controls, wrapping and exact fallback.

The synthetic result crosses the design's 15% timed application-byte
threshold for considering a separately reviewed experiment. The timed
comparison lost its opportunity on all nine modeled delayed final bytes.
An installed candidate would still require authenticated negotiation,
local write and recovery evidence, and actual signed traffic and latency
measurements. This increment changes no production path or CI selection.
Native ARM64 and macOS runs were outside this local result.

The exact code at `b38f16cc856e34e23e36401bb4352bc6131b0214` passed four
batch and redraw tests, four output parser tests, and strict all-target
Clippy for `marmish-native`. Two private tmux runs reproduced all
traffic counts in 2.23 s and 2.22 s respectively, including fixture
startup and cleanup. These are test runtimes with Rust 1.97.1 and
tmux 3.4 on Linux AMD64.

## Reproduce

Run the deterministic selection and then explicitly select a native tmux
binary for the opt-in oracle. Both commands build and run the library
test harness. The tests run locally against their generated fixture.

```sh
cargo test --offline --locked -p marmish-native --lib terminal_output_batches
MARMISH_TEST_TMUX="$(command -v tmux)" \
  cargo test --offline --locked -p marmish-native --lib \
  terminal_output_batches::terminal_redraw_evaluation_tests::terminal_redraw_evaluation_tmux \
  -- --exact --ignored --nocapture
```

[Feasibility design](../phases/terminal-redraw-evaluation.md)
[Independent code review](../reviews/terminal-redraw-evaluation-code.md)
