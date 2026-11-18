# Directional Peer heartbeat planner reviews

Claude Code 2.1.301 with `claude-sonnet-5-5` reviewed the design and
implementation in `--permission-mode auto`.

## Design

Session `0e9384e5-4958-43a3-a86e-09c1b0b14345` returned REVISE. The next
version defined the private sampler and injected clock ports, invalidation
on pause and revision changes, dual clock deadlines and recovery, checked
retry spacing, durable sequence and policy fences, and explicit projector
retirement. It also specified replay-neutral projection and permission issue
timing and bounded memory.

Session `8b732ff5-c981-4c52-8c27-9b5baf951525` approved the
[phase design](../phases/peer-heartbeat-planner.md) before coding. The
approved scope covered the stand-alone fixture and kept status opt-in,
address selection explicit, and all daemon, MLS and relay integration for
later work.

## Implementation

Session `a48277f3-fc64-42b6-9f87-81e0cca11bdd` approved the exact planner
source and test snapshot with no blocking findings. The reviewer read the
planner and test entry point, inspected the sampler poll contract, examined
test coverage by name and address checks, and reran all 31 ordinary tests
and the 200-sample Linux benchmark. The review did not inspect every test
body. The independent benchmark measured a total p50 of 112 µs, p95 of
175 µs and p99 of 253 µs.

Each permission output remains bounded to one page. The fixture's page
index re-encodes earlier pages when selecting a later one. This bounded
cost was accepted for the fixture. Future owners must treat the first
tick after pause as potentially drain-only, map verified Tor routes to
`Confidential`, and persist sequence and policy fences and receiver
decisions before live activation.

The review used base `15defccc1144dda2c1d70c70fe729f9b93616116` with
the three new Rust files and the planner design overlaid. The four-file
SHA-256 manifest hash is
`ab3ce7ff97ee644772c0c2f71ba4c6b942d479ac9c26342a83a70e4bd6bdedae`.

[Validation record](../validation/peer-heartbeat-planner.md)
