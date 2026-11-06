# Native streaming runtime qualification

Phase 2a, Beads `marmish-8ka.2`. Source-only implementation, not a released
installer capability. Integrated and scoped independent Claude code reviews approved the runtime.
Final live qualification passed; source publication is being completed.
Linux AMD64 results below do not establish macOS or Linux ARM64 support.

## Actual relay execution

The native CLI and two native daemons used freshly generated disposable
file-backed identities and explicit `wss://relay.eu.whitenoise.chat/` selection.
The harness initialized identities, ran native setup, configured a private
server executable policy, and exercised actual Marmot/MLS commands. No mocked
relay, plaintext transport or direct subprocess shortcut supplied the results.

The corrected integration build passed the final expanded cases in
[the sanitized report](streaming-runtime-live.json):

| Case | Observed result |
| --- | --- |
| Binary stdin/stdout, no compression | 393216 exact bytes, stderr preserved, exit 7; 87.085049 s, 4515.3 bytes/s |
| Binary stdin/stdout, zlib negotiated | 393216 exact bytes, stderr preserved, exit 7; 66.435655 s, 5918.7 bytes/s |
| Early stdin closure | `true` and `head -c1`, raw and zlib; all four exit 0 with exact expected output |
| Small command latency | 5.538228, 5.412073, 5.777887 s; median 5.538228 s |
| Literal argv | 10 arguments: spaces, quotes, backslash, metacharacters, newline, empty, leading dash and Unicode preserved |
| Ctrl-C, TERM-ignoring process with bounded output backpressure | Remote terminal confirmed, exit 130, 7.527215 s after SIGINT |

Both binary cases used repeated byte values 0–255, exceeding the 256 KiB raw
window. They independently checked the entire byte sequence, not only the
SHA256 recorded in the report. The cancellation fixture continuously filled
server output without the frontend consuming it, then interrupted the frontend;
it required a confirmed remote terminal response. Owned daemons were stopped
and successful private test state removed. Total harness time was 224.368797 s.

These are single-run wall-clock observations, not controlled compression or
cross-machine benchmarks. The first case includes initial command-room
preparation, while the second reuses that room; compression cannot be credited
with their timing difference. Both endpoints ran on the same Linux AMD64 host,
with relay transport going over HTTPS/WebSocket to the public relay. No Tor,
installer, rsync or macOS result is implied.

The final measured debug binary SHA256 was
`3b4b21d653fb7699303946b497f26c12aa2c198dc5258f8ce6bf34848b936625`,
built from phase-2 work atop `eac9410a45fcc9251dbb84d47fd760bbdb1dfacf`.
This binary reports version 0.2.3 but contains the unreleased runtime increment;
it is not the published 0.2.3 release asset. Latency measures full new-stream
command completion using an existing room, not network RTT.

## Failures discovered and corrected

The first live run returned all 393216 expected binary bytes and stderr, then
failed with `stream_stdin_ack` and exit 255. Finished-runner eviction had lost
final input/output offsets. Those offsets are now retained in encrypted stream
metadata; the same live case passed on the corrected build. A focused fallback
regression test also passed.

The first two focused native runs passed 24/25 and 26/27 tests respectively.
The remaining failure was the adversarial author fixture: upstream MDK refused
forged application authors before the fixture reached native projection. The
fixture now injects inconsistent delivery metadata after a genuine encrypted
message, and its targeted rerun passed. This does not substitute for the full
regression suite on the final integrated implementation.

Nineteen native/installer license-generation tests passed after adding the new
production path dependency. Engine and runner tests cover ownership, descriptor
isolation, cancellation races, output backpressure, queued-input suspension,
no-relaunch fences, authority checks and publication credit. Final full-suite,
formatting, strict lint and independent review evidence will be recorded with
the completed increment.

During the first integrated Claude review, `cargo test --locked -p
marmish-engine --lib` passed 120 tests with one ignored (22.24 s). The native
library suite passed 421 tests and failed one installer recovery test (401.93 s):
`every_commit_boundary_recovers_without_adopting_version` returned `install_busy`
on immediate reacquisition. This was a failed suite. Investigation identified
that close-on-exec descriptors can still
carry a shared installer lock across fork until exec; the log alone does not
prove that scheduling caused this occurrence. A deterministic cloned-descriptor
regression now passes with scoped explicit unlock. All 21 affected installer
tests passed separately; a final full-suite run is still required.

The corrected focused stream run passed **39 tests**, with one deliberately
ignored large-volume qualification, in 57.11 seconds. It covers retention after
160 completed streams, historical publication pressure, ACK-relative retries,
definitive failure versus ambiguity, cancellation, invalidation and IPC diagnostic
categories. The large-volume test was then explicitly run and passed in
**7.86 seconds**: 65 MiB + 17 deterministic pseudorandom/NUL bytes passed through
an actual `tee` process into both stdout and stderr, with incremental independent
hashes, exact offsets, deliberately withheld output ACKs, slow consumers and
256 KiB bounds checked on all three queues. This is local runner qualification,
not relay throughput. Further lifecycle corrections require fresh checks.

The coordinated full native library suite subsequently passed **438 tests**, zero
failures, one ignored, in **425.27 seconds**. This includes the installer lock,
lifecycle and startup-diagnostic corrections. Claude's second code review then
found further retention/publication/cancellation issues; those corrections need
fresh affected checks and independent signoff. The passing suite is evidence for
that reviewed snapshot, not proof that later fixes or the final source are tested.

After the third review corrections, the focused native stream run passed
**57 tests**, zero failures, one ignored, in **53.03 seconds**. This includes
real early-exit stdin races, immutable client launch intent retention, uncertain
invalidation/reconciliation, and descriptor flag restoration with aliases.
The pure stream library passed **17 tests** in **0.12 seconds**, including the
mandatory stdin closure field. Strict Clippy across the native, engine and stream
packages and all targets passed with warnings denied (**4.81 seconds**).
The corrected full native suite and independent fourth review are still pending.

The coordinated native library run for that snapshot passed **454 tests**, zero
failures, one ignored, in **456.65 seconds**. Subsequent changes are an isolated
preparation-interruption test and the uncertainty Status correction identified
by the scoped follow-up review; their affected checks are pending. This full
suite result is not attributed to those later changes.

Final-source affected checks then passed **59 native stream tests**, zero
failures, one ignored, in **57.70 seconds**. Both the real-child uncertainty
snapshot regression and isolated subprocess SIGINT-during-preparation regression
passed. Strict all-target Clippy passed again with warnings denied (**4.16 s**),
formatting and whitespace checks passed. The scoped uncertainty correction has
independent Claude approval; see the review record for the preceding failed
review and fixture compile correction. Large-volume and live reruns follow.

The explicitly selected final-source large-volume runner test also passed:
**65 MiB + 17 bytes**, exact independent stdout/stderr hashes, slow consumers,
withheld ACKs and queue bounds, in **8.21 seconds**. A native-only binary build
then succeeded. An initial combined engine/native build warned about their shared
`marmish-worker` filename; building the native package separately restored its
worker, verified by its native `release-info` response before live execution.
The live rerun is using those rebuilt binaries, with three latency samples and
the expanded early-stdin-closure cases.

The first final-binary relay rerun failed **before command traffic** in
**7.435865 seconds**, with `daemon_unavailable` from the stream frontend.
[The sanitized failed report](streaming-runtime-live-startup-failure.json)
records zero completed command cases and both owned daemons stopping with exit 0.
Inspection found an activation readiness race: the harness accepted a successful
Status while its state was still `activating`, and `--no-autostart` then refused
that already-running daemon instead of waiting for readiness. Product and harness
regressions are being added. This is a failed live run, not a throughput result.

The startup correction's native Unix-IPC regression passed (**0.54 s**), the
broader CLI selection passed **39 tests** (**1.00 s**), and strict Clippy passed
(**3.91 s**). The fixture's initial permission failure was corrected to create a
0700 temporary profile, preserving production ownership checks. The harness's
**18 hermetic tests** passed; both fixes have independent scoped Claude approval.
The native package was rebuilt separately and a fresh live rerun started.

## Final qualification outcome

After the reviewed readiness fix, the rebuilt native binaries passed every case
in the final live report above. Both owned daemons shut down with exit 0; the
successful fixture's private state was removed. The earlier startup-failure
report remains evidence of the failure corrected before publication. The final
affected native, CLI, harness, large-volume, formatting and strict Clippy checks
are recorded above; the full 454-test native run applies to the stated earlier
snapshot, with subsequent changes covered by affected checks and scoped reviews.

Cross-platform installer testing, real rsync transfers and resumable native copy
are separate acceptance phases. This Linux relay pass does not establish them.
