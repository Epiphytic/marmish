# Copy qualification amendment

Beads `marmish-8ka.4`; applies to the acceptance section of
[the copy design](streaming-copy.md), not its protocol or ownership invariants.
Independent Claude design approval is recorded in
[the review record](../reviews/streaming-copy-qualification-design.md).
This amendment is not evidence of a passing test or a completed phase.

The original acceptance text mixes payload sizes, installed interoperability,
filesystem fault injection and an exhaustive crash/race matrix in one list.
Qualification needs explicit layers so a local filesystem test cannot be
presented as a network or installed-platform result. The following gates retain
the functional delivery scope while bounding public-relay workloads.

## Required delivery gates

- Local actual-helper qualification transfers more than 65 MiB in each of four
  raw/zlib Put/Get combinations, using bounded buffers and incremental hashes.
  The fixture contains both compressible and deterministic incompressible
  regions. Require exact destination hashes and metadata, checkpoints, natural
  helper exit and final output acknowledgement. Count acknowledged unique codec
  payload bytes separately from raw stream and source-file bytes. This measures
  local codec payload ratio, not encrypted Marmot or network traffic savings.
  Label this debug-build, parallel four-way, explicitly ignored qualification
  with its exact test invocation; it is not release throughput or routine CI
  regression coverage.
- Live Marmot qualification transfers more than the 2 MiB durable window in
  both directions with both codecs, plus compressible, empty, missing-source
  and existing-destination cases. Use explicit relays, disposable identities,
  finite deadlines and independently verified bytes/metadata/errors/cleanup.
  Publish observed end-to-end timings and the actual deadlines. No more-than-
  65-MiB public-relay result is implied by the local helper test.
  The large live fixture is incompressible random data; the compressible fixture
  is about 57 KiB. Zlib here is a functional check, not a wire-savings measurement.
  This harness does not exercise live cancellation or resume; those end-to-end
  gates belong to the installed cohorts below.
- Installed qualification must run the actual candidate installer and binaries
  on Linux AMD64, Linux ARM64 and macOS ARM64, with all nine directional platform
  pairs for streaming, rsync and copy. Candidate/source/run/attempt/archive/tool
  provenance, client and server evidence, exact bytes and physical cleanup must
  agree. Separate same-platform edge cohorts exercise explicit cancellation,
  detach without cancellation, quota/refusal/error distinctions, and rsync
  signal behavior. Interrupted Put and Get must reach an intermediate durable
  checkpoint, prove the old attempt remains charged, explicitly cancel it, and
  resume through a fresh stream with exact final bytes and metadata.
  Each of the 27 tool/platform-pair cells must independently pass exact-byte,
  client/server-evidence and physical-cleanup checks. Skipped or partial cells
  cannot pass. The `package` job in `.github/workflows/streaming-candidate.yml`,
  step `Verify native components`, runs unfiltered native and engine tests on
  `macos-15`, including `only_macos_exact_public_alias_prefixes_are_normalized`
  and the closed-stdio/original-FD3 launch tests. The job must assert non-root
  execution; record its exact job/run in validation evidence. The
  [installed design](streaming-installed.md) and reviewed drivers/workflows were
  integrated in `493f059`; source integration does not satisfy the installed
  execution gates above. Fresh exact-source candidate and cohort evidence is
  still required.
- Local recovery tests must demonstrate uncheckpointed-suffix truncation,
  corrupted-prefix refusal, source/destination binding conflicts, dropped
  checkpoint and commit replies, committed receipt recovery without Data, and
  post-receipt protocol failure classified as uncertain. Test authenticated
  replay/revocation and local administrative cleanup, inherited-context bounds,
  executable denial, no-follow/special-file/hardlink rejection, admission and
  receiver lock contention, quota/retention and local worker ownership.
  Require actual-helper Put and Get cancellation with buffered data and a full
  bounded window: preserve the last durable checkpoint, release ownership and
  confirm cancellation. Also race cancellation against commit: receipt recovery
  must classify committed versus partial, and never roll back committed bytes.
  Deterministic negatives remain required: static symlinks at every component;
  read-only/missing parents; a destination created after Reserve before commit
  (no-clobber refusal preserves its bytes); duplicate Create; Resume without a
  checkpoint; preserved modes lacking owner read/write; source mutation;
  wrong account, direction or metadata on resume; non-UTF-8 and leading-dash
  names. Only exhaustive scheduling interleavings are deferred.
  Context tests must cover forged/missing/oversize inherited context, initially
  closed descriptors 0/1/2, a context pipe initially allocated at FD3 and then
  relocated, helper FD leak prevention, and typed-FD cleanup on spawn errors.
- The released-format-6 component fixture must migrate to format 7 without
  changing complete existing Exec rows, including launch digests, identities,
  offsets and terminal state. This fixture comes from commit `78641a4` DDL and
  invokes `copy_init(6)` in memory; it does not exercise SQLCipher or
  `Journal::open`. Malformed rows must leave the component conversion's rows,
  schema and version intact. A genuine installed v0.2.3-to-candidate upgrade
  with an Exec stream record is separately required to claim an installed
  upgrade; if not executed, disclose that limitation (`marmish-8ka.6`). Formats
  0/3/4/5 and other historical/old-binary combinations remain unqualified.
  Rejection guards do not establish historical binary compatibility.

Each gate needs independent source review and actual execution evidence.
Compilation, validator tests, publication acknowledgements and parser success
cannot substitute for it. An infrastructure failure is a failed/unqualified
result, not a protocol PASS. Any production fix requires affected regression
tests and review; retain earlier failures and bind results to their exact source.

## Deadline amendment

The original design's at-least-4x margin is not established by a successful run
under a smaller ceiling. Record `headroom = deadline / observed` per operation.
Headroom below 4x is labeled **passed, below the 4x margin**, and does not satisfy
the original margin rule; timeouts remain failures. Future live budgets must be
at least 4x the longest observed operation of the same size and codec, within
the harness's 86,400-second cap. Routine CI uses the small bounded case. A
successful observed operation remains functional evidence, but headroom below
1.5x requires a corrective run with the 4x budget. Retain earlier failures and
report both. Do not claim margin compliance from a run whose deadline is below
4x its applicable observed baseline.

## Explicit extended qualification

The following remain tracked hardening work rather than claims of exhaustive
coverage: faults immediately before and after every sync/rename; cancellation
inside every filesystem syscall; every adversarial scheduling of parent/entry
replacement, mkdir/Create/Resume and overwrite/no-clobber races; actual device
nodes and remounts requiring unavailable privileges; unsupported network or
removable filesystems; every historical journal/old-binary combination; and
more-than-65-MiB relay transfers and actual wire-savings measurements. Every
listed extended item is tracked in `marmish-8ka.6` and must have a separate line
in the validation report. Live cancellation/resume remain unqualified until the
mandatory installed cohorts pass, rather than being waived as extended work.
This amendment does not weaken fail-closed
path handling, authorization, atomic commit, replay fencing, process ownership
or the requirement to fix a reproducible defect found in those paths.

Only qualified local filesystems and observed platform behavior may be claimed.
Orderly crash recovery is distinct from guarantees about hardware/power loss.
Same-UID code remains trusted as documented in the original design. Missing
extended qualification must remain visible when describing durability limits;
passing the functional delivery gates does not establish exhaustive crash or
race resilience.
