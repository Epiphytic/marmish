# Read-only progress inspection review

Scope: [phase design](../phases/streaming-progress-inspection.md), original
installed stream-matrix failure investigation. Reviewer: actual Claude Code
`claude-sonnet-5-5`, `--permission-mode auto`; session
`49e97981-02bf-4621-8b86-fc983e563252`. Design-only inspection of the phase and
actual relevant source; no Cargo, Git, relay traffic, workflow dispatch or edits.

- Round 1: **REQUEST_CHANGES**, 219.071 seconds, 30 turns; result SHA256 `cbd8725f8803a68bfed9c9302ca58278e9c76fc35a54105a0fc166822d316837`.
- Round 2: **APPROVE**, 22.910 seconds, 2 turns; result SHA256 `817ab88c343238bd8f4f1c6600b4ebb6b0b00d975daa84c7ce0d0e87ada4d34f`.

The first review required signal-first cleanup, truthful queued/started IPC
execution after caller abandonment, explicit release non-goals and fail-stop
recovery, and a frozen interface before disjoint harness implementation. The
revised design resolves all four. It also selects newest publication headers,
requires query-plan checks, rejects direct harness journal access and records
nonsimultaneous snapshots and their interpretation limits. The outer 20-second
inspection clip retains the existing 15-second per-RPC cap; no actor availability
or cancellation guarantee is claimed.

The coordinator inspected the actual final approval and authorized bounded
implementation after it, with the IPC author owning enum-match fallout and the
sole Cargo lane, and the harness author waiting for the DTO freeze. Nonblocking
review suggestions are included in the implementation brief: deterministic
queued actor tests, pending/during signal cases and clear effective RPC timing.

The integrated code reviews below approve the implementation; hosted qualification remains pending. No version, release, installer,
remote-wire or acceptance change is authorized here. A fresh complete three-target
candidate and the original matrix are required; diagnostics cannot substitute.

During implementation the author found that the existing IPC test authenticates
an actual same-UID peer but never exercises rejection. The coordinator authorized
a minimal private extraction of the unchanged UID comparison in `ipc.rs` and a
unit rejection check using a distinct UID. This checks the predicate, not a real
foreign-UID process; kernel credential retrieval and dispatch remain unchanged.
The integrated code review includes this narrow coverage change.

Author validation on Linux AMD64 (UID 1001, Rust 1.97.1), using the shared target
with serialized Cargo ownership: the `inspection` filter passed 18 tests in
0.66 seconds, `ipc::tests` passed eight, `journal::stream_store::tests` passed
19 in 11.07 seconds, and the authenticated open/replay/grant-withdrawal test
passed in 6.40 seconds. These filters overlap; counts are not summed. Strict
Clippy passed for all targets with `installed-checks` and with default features;
format and diff checks passed. Initial fixture type errors and one newline-size
Clippy warning were corrected before these results. The full installed-driver
filter then passed 211 tests in 5.48 seconds; its nested socket-fixture process
is not an additional outer-suite test. Final feature Clippy and format/diff checks
also passed. The independent integrated reviews are recorded below.

Ripwire quality delta exited 2. Four gates concern deliberate private test-fixture
clones (journal setup and the required full `StreamRecord` shape); sharing them
would introduce a cross-module fixture API. Two other gates conflate generic
`failure` and `run` names with unchanged functions in other modules. New projection
size/complexity findings remain descriptive review signals. No green static
quality result is claimed; compiler, targeted tests and independent review are
separate checks.

## Integrated code review

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`, session
`893a435d-7421-4bb7-b3bf-cfeae31fb825`: **APPROVE**, no blocking defects,
137.873 seconds / 21 turns. Private result SHA256
`dfc9502bcd783ca74e4fd3564f547b58fc70bc5ece99a5a9ea5a5901bfbc75c2`. Scope included all new source
and tests, the narrow `ipc.rs` coverage change, the approved phase and spec.

The reviewer independently passed formatting/diff checks, 18 inspection tests,
eight IPC tests and feature Clippy (cached, no recompilation). It did not rerun
the complete installed-driver/store filters or default Clippy, and did not open
their author logs; those results remain author validation, not reviewer runs.

Useful nonblocking findings were incorporated before publication: distinguish
a dead daemon from invalid diagnostic data; directly test attempt result mappings;
name the timeout/sink test for its real seam instead of implying physical cleanup
or a real orchestrator run; clarify that invalid slot integrity does not invalidate
independently validated record fields. The corrective review below inspected them.
The reviewer noted the record query-plan assertion checks absence of a temporary
sort, and no actual full orchestrator ordering/signal test runs. Source structure
and bounded seam tests are distinct from physical daemon cleanup proof. Hosted
qualification remains pending; no root cause or release readiness is claimed.


### Corrective integrated review

The same actual Claude Code AUTO session returned **APPROVE**, no blockers,
36.321 seconds / five turns. Private result SHA256
`ee83e6381619a7595d211dcb93042092027a349d64866abbab8064765073b424`.
It inspected the complete diff and verified the unavailable/invalid distinction,
direct result/liveness tests, honest timeout/sink assertions and DTO comments.

Author corrective validation passed 20 inspection tests (0.55 seconds), 213
installed-driver tests (6.67 seconds), feature/all-target strict Clippy
(8.50 seconds), formatting and diff checks. The earlier private inspection log
was accidentally replaced by this corrective run; no retained original raw log
is claimed. The first independent reviewer run still verifies the earlier 18-test
result. The reviewer independently passed 20 inspection tests (0.68 seconds),
213 installed-driver tests (5.81 seconds), cached feature Clippy (0.23 seconds),
default Clippy with recompilation (5.65 seconds), formatting and diff checks.
Counts overlap and are not summed. It did not independently rerun the separate
journal-store or authenticated replay filters in this round; those are not
covered merely because the inspection filter passed.

Remaining explicit limits: no real orchestrator signal/cleanup integration test,
no deterministic mid-request child-death test, no real foreign-UID peer process,
and limited query-plan assertions. Corrective Ripwire delta still exits 2 for
private fixture clones, generic-name conflations and short-horizon IPC-helper
churn; no green static-quality claim is made. These are distinct from the passing
compiler/tests and independent code approval. This increment provides diagnostic
observability, not a demonstrated cause or a qualified release.
