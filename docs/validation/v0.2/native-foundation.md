# Native foundation validation (in progress)

Date: 2026-11-02. Phase: `marmish-7nw.2`. This records transport-foundation evidence, not a command-execution release or completion of the platform matrix. Source is the uncommitted native implementation based on `668ea74af911b521ab4e94fdf819bbfe800e7ed2`; the final reviewed commit will be recorded separately. No v0.1 disk migration is required or tested.

Latest integrated code verdict: **APPROVED**, [Claude's actual round-5 review](../../reviews/v0.2-native-code-r5.md). It independently verifies the round-4 fixes and passes 91 native tests (90 repository tests plus its extra regression), vault 8 unit and 3 helper tests, workspace clippy, formatting and whitespace checks. Its scope is offline; root's completed direct/onion checks are recorded separately below. Hosted platforms remain untested, and no phase acceptance, remote-command release or full matrix is claimed. Earlier snapshots are historical and do not validate later changes.

## Independently reviewed final-fix snapshot

Root's full final offline run passes **90 native tests** in 187.08 seconds,
vault **8 unit and 3 helper tests**, with the provider gate explicitly ignored
in the ordinary run. Native/vault all-target clippy with warnings denied,
formatting, actionlint and diff whitespace checks pass. Claude's independent
scratch run passes those tests plus its own additional collision regression.
The separately run real Linux provider gate is described below; macOS provider
and hosted transport results are still pending.

The report now uses the wire protocol constant. An allowed peer's conflicting
session is refused, forgotten and acknowledged without allocating rejection
metadata or advancing the admission frontier. The actual encrypted collision
regression failed before the fix, then passed, preserving the original room and
later valid room across encrypted reopen. All 24 adapter trust tests pass in
113.67 seconds. Additional tests verify that a noncanonical receipt cannot
change delivery state and that the failed-open wrapper immediately cleans an
existing native orphan while preserving its original error. The latter does
not inject a fault specifically between native creation and journal acceptance.

Executable SHA256:
`a27ba1a284cf6658c88fce0d67680d09b2f3c5d85790dfa1948a5f435771b542`.
[Direct report](linux-amd64-direct-v2.json): PASS in 12.656 seconds.
[Owned onion fixture report](linux-amd64-onion-v2.json): PASS, native flow
84.030 seconds, supervisor exit 0 and readiness published only after the full
native flow. Both report `marmish-foundation/2`, preserve four authenticated
66-byte payloads and verify receipts, duplicate/conflicting IDs and two-owner
encrypted restart. This validates the report fix as well as actual transport.

| Stage | Direct milliseconds | Arti milliseconds |
| --- | ---: | ---: |
| Both identities' cold activation | 1,200 | 44,922 |
| Alice → Bob with encrypted receipt | 699 | 2,620 |
| Bob → Alice with encrypted receipt | 750 | 2,500 |
| Both identities' restart/open/activation | 1,453 | 19,716 |
| Alice → Bob after restart | 1,149 | 2,651 |
| Bob → Alice after restart | 693 | 2,293 |

These remain individual tiny-message observations, not percentiles, bulk speed
or command-execution benchmarks. No actual network-loss fault was injected.
The onion supervisor reaped its relay children and removed private state. Its
cleanup record correctly says it did not reap itself; the separate parent
explicitly waited for the supervisor and removed the outer fixture root and
private logs/binary. Only the public report remains. Local fixture IDs and the
base commit identify the test binding; they do not identify an implementation
commit. The binary, script and lock hashes identify the tested snapshot.

## Retention-build validation, before final review fixes

The frozen N2 snapshot passes 87 native tests in 170.04 seconds plus vault 8 unit
and 3 helper tests. The gated provider test remains ignored in that ordinary
run; its explicit isolated-provider results are recorded separately below.
Native/vault all-targets clippy with warnings denied, formatting, actionlint and
diff whitespace checks pass. Claude independently ran the same 87/8/3 suite in
an isolated copy, then reproduced the session-collision bug with an additional
production-MDK test. That failing test is fix evidence, not a passing gate.

Executable `b3f3790761d800ee92afc698781a132445be056d66dc3b42a0d84f8d78b1f90a`
completed the real direct encrypted/restart flow in 12.552 seconds and the Arti
flow in 78.622 seconds. Both preserved all four payloads and cleaned native
state, but their report incorrectly named `marmish-foundation/1`. The strict
`/2` fixture check therefore failed and withheld readiness. [The actual failed
fixture result](linux-amd64-report-mismatch.json) records exit 1, no readiness,
relay-child reaping and private-state cleanup. Its parent waited for the
supervisor and removed the outer private fixture root; no fixture is left
running. Neither result counts as full report/fixture acceptance. A fresh
executable and affected live reruns are required after the fix.

The session-collision regression separately failed before the fix with the
reviewer's exact `conversation_conflict` trigger. Correction and final review
remain in progress.

N1 follow-up: two production-API tests first reproduced disjoint-route acceptance
failure and duplicate transport-group-ID acceptance. With validation moved before
durable room acceptance, both pass, including rejection/forget/pending-event
acknowledgment, subsequent valid room operation and encrypted account reopen.
The expanded 13-test trust suite passes in 70.41 seconds and clippy passes with
warnings denied. These are fix-validation results, pending independent code
re-review; N2 remains under design review.

A subsequent full parallel run passed 62 tests and failed immediate profile
reacquisition once with `profile_in_use`; the isolated repeat passed. A new
deterministic test reproduced the underlying open-file-description behavior:
closing the lease descriptor alone leaves its flock held by a duplicate, as can
happen briefly between concurrent fork and exec. Explicit unlock at lease drop
fixes the deterministic regression; all 16 identity tests now pass. The test
also proves closing the inherited old descriptor cannot release a replacement
owner's new lock. This change awaits full integrated validation and code review;
the earlier 61-test snapshot and scoped identity approval do not approve this fix.

The subsequent lock-fix snapshot passes the full offline suite: 64 native tests
in 84.30 seconds, vault 8 unit and 3 helper integration tests. This was compiled
before the N2 implementation began; it does not validate that later source.

[The retention amendment](../../phases/v0.2-phase1-retention.md) received actual
[Claude design approval](../../reviews/v0.2-retention-design-r4.md) after four
rounds. N2 implementation and independent code review remain in progress.

## Actual direct transport

Linux x86_64, Rust 1.97.1, native release build. Fresh disposable Alice/Bob file-backed identities, separate SQLCipher stores, explicitly selected `wss://relay.us.whitenoise.chat/`, Tor off. No namespace privilege or Python runtime is used by Marmish.

Command: `marmish --debug --relay wss://relay.us.whitenoise.chat/ --tor off foundation-check`. This explicit endpoint is test configuration, never a product default.

[Machine-readable result](linux-amd64-direct-foundation.json): PASS in 11.623 seconds, all runtime/state cleanup checks true. Four messages preserve a 66-byte payload containing NUL, non-UTF8, quotes, backslash, dollar, newline, carriage return, tab and UTF8 bytes. Each sample requires authenticated peer decrypt, durable local projection and encrypted application receipt; relay acknowledgment alone cannot pass. Each also checks that an identical application-ID retry returns the existing intent and changed bytes under that ID are rejected. Both owners close and reopen their encrypted state before the last two exchanges.

| Measurement | Milliseconds | Samples |
| --- | ---: | ---: |
| Both identities' cold activation | 1,231 | 1 |
| Alice → Bob with encrypted receipt | 619 | 1 |
| Bob → Alice with encrypted receipt | 668 | 1 |
| Both identities' restart/open/activation | 1,347 | 1 |
| Alice → Bob after restart | 990 | 1 |
| Bob → Alice after restart | 594 | 1 |

These are individual observations, not percentiles or isolated network latency. The tiny payload's reported bytes-per-second divides application bytes by receipt latency; it is **not a bulk throughput benchmark**. Larger workloads and cross-platform command speed measurements belong to the later command matrix.

Validated executable SHA256: `72e6fb77b9844c754e2987b20c6f010d434bf1390af5a23730ea613ec750ffa7`. Cargo.lock SHA256: `0d937b2ab2a1e04afa4b0cbf965fba92ca67c63437c77a39be7609aefd9c4a25`. Further reviewed fixes require another build and affected checks; these hashes identify this observed run only.

## Failures found and corrected

Initial live runs exposed missing explicit KeyPackage publication, then the missing SDK notification forwarder required by the injected relay client. Earlier direct runs consequently failed at discovery or recipient Welcome timeout. All emitted cleanup checks passed; none were counted as success. The notification forwarder now has owned lifecycle and loss supervision, exercised by an offline test.

The first Arti run completed bootstrap/connection but failed activation. The pinned Deaddrop fixture accepts exactly seven standard KeyPackage tags; omitting an optional advisory `client` tag resolved its policy mismatch without changing MLS payload or authentication. Another cold attempt timed out connecting. These failures were not counted as successful exchange.

## Earlier local checks and remaining gates at that snapshot

Native library at the successful direct/onion snapshots: 45 tests passed; subsequent recovery and secret-buffer changes require a new integrated run. Identity-focused suite after secret-buffer fixes: 15 tests pass. Vault: 8 unit and 3 helper integration tests pass, including the 15-second watchdog. The real Linux keychain probe returned `uninitialized`, reported separately from passing file-backend flows; no real provider creation test passed on this host.

An actual independent Claude interim review returned CHANGES_REQUIRED; [the review and recovery findings](../../reviews/v0.2-native-code-r1.md) remain to be resolved and re-reviewed. Subsequent scoped reviews approved [root/identity/vault](../../reviews/v0.2-native-root-r2.md) and [the two CI files](../../reviews/v0.2-native-ci-r2.md), with explicit exclusions: neither is integrated phase approval. Native hosted Linux AMD64, Linux ARM64 and macOS workflows have been authored but have not run. Platform runs, final code approval and source publication remain required. Remote commands, installer end-to-end checks, the nine-pair matrix and release binaries are later phases.

## Actual embedded Arti transport

[Machine-readable onion result](linux-amd64-onion-foundation.json): PASS in 83.045 seconds on Linux AMD64. Fresh native Alice/Bob identities performed the same four authenticated encrypted byte/receipt exchanges, conflicting-ID checks and encrypted restart through an explicitly selected ephemeral v3 onion relay with `--tor always`. Cleanup/runtime/state all true. This is real MDK interoperability through embedded Arti; no SOCKS proxy or direct relay fallback was used.

The separate relay fixture was Deaddrop's native server pinned at `b38b2841b081952dc7728da391fe0e26fc7c6181`, with private disposable state and bounded lifetime. This host exposes a `USER` alias inconsistent with the system group name; the disposable test invocation alone used `env -u USER` so fs-mistrust resolved the actual account metadata. UID/groups and permission enforcement were preserved. The normal product and hosted workflow do not override `USER`.

| Measurement | Milliseconds | Samples |
| --- | ---: | ---: |
| Both identities' cold activation over Tor | 43,897 | 1 |
| Alice → Bob with encrypted receipt | 3,386 | 1 |
| Bob → Alice with encrypted receipt | 2,327 | 1 |
| Both identities' restart/open/activation | 19,235 | 1 |
| Alice → Bob after restart | 3,678 | 1 |
| Bob → Alice after restart | 1,890 | 1 |

The same single-sample and tiny-payload limitations apply. This observed binary predates the interim review's recovery fixes: success is evidence for the happy path, not dropped-connection or interrupted-send recovery.

Observed onion-run executable SHA256: `cb4b0029f9e070956e77ccac75def14318b62c7807ecda5a18f2f46372cc352f`. Its Cargo.lock hash matches the direct run above. Recovery changes after this run require fresh validation.

## Recovery-build repeat

[Direct recovery-build result](linux-amd64-direct-recovery.json): PASS in 11.812 seconds, executable SHA256 `cdc0e1f0096a507b0272cb3bd3490049db9228e27052be5c3347629b3ab5c7f6`. It includes the retained-publication/receipt recovery fixes and finite room-capacity policy handling. The same six stages measured cold activation 1,193 ms; receipts 652/618 ms; warm restart 1,341 ms; post-restart receipts 1,041/641 ms. All cleanup checks passed; byte and ID checks are unchanged. This repeat still exercises the happy path, with recovery fault boundaries covered separately by offline production-API tests and pending integrated review.

An actual shell-fixture run exposed a nested `TMPDIR` exceeding the Unix socket path limit; mocked relay/lifecycle tests had not caught it. The fixture now allocates a separate short private temporary directory, and cleanup requires both owned directories to be removed after child reaping. A subsequent run using the earlier executable passed both initial encrypted exchanges but failed warm Tor reactivation and correctly withheld readiness; cleanup passed. Neither failed run counts as readiness success. Final script review and a fresh recovery-build onion repeat remain pending.

## Earlier recovery-build offline checks

The frozen recovery/capacity source passes 61 native tests (85.51 seconds), vault 8 unit plus 3 helper integration tests, full-tree formatting, native/vault clippy with warnings denied, and diff whitespace checks. The native total includes 11 real production-API Current-profile trust tests: accepted two-party Welcome; unsolicited/wrong-purpose/extra-admin/third-account/sibling-leaf rejection; authenticated purpose/admin mutation followed by native convergence and revocation; finite room capacity; and exact retained native Welcome/application obligations across paused drains. Wrong/missing-welcomer projection checks deliberately mutate already trusted upstream event metadata to exercise the application gate; they do not demonstrate wire forgery.

The existing Linux isolated D-Bus/GNOME-keyring gate was explicitly run and passed in 2.61 seconds, including create/load/conflict/list/lock behavior with disposable state. This unchanged gate exercises the legacy provider namespace, so it does not establish full v0.2 default-provider profile initialization or macOS Keychain behavior. Ordinary test runs still report this gate ignored unless explicitly selected.

The gate was subsequently extended to exercise the real MarmishV2 provider
namespace: two named Nostr/storage pairs create, load and list independently,
including a reference colliding with the legacy namespace. Cross-version
references are refused and legacy secrets/listing remain separate. The extended
gate passed in 2.46 seconds inside the same disposable private bus/keyring.
This adds actual v2 provider evidence, but still does not demonstrate full native
profile auto-initialization or macOS Keychain behavior.

[Final CI scoped review](../../reviews/v0.2-native-ci-final.md) approves the exact short-temporary-path fixture and native workflow bytes, plus making the historical v0.1 workflow manual-only. Hosted workflow execution and the integrated Rust review remain separate gates.

## Recovery-build embedded Arti repeat

[Independent onion recovery-build result](linux-amd64-onion-recovery.json): PASS in 78.638 seconds using the same `cdc0e1f…` executable and locked dependencies as the direct repeat. Four authenticated 66-byte messages, encrypted receipts, application-ID checks and restart passed through the fresh explicitly configured onion relay. Cold activation was 42,935 ms; receipts 2,193/2,103 ms; warm restart 19,109 ms; post-restart receipts 2,269/1,936 ms. Runtime/state cleanup checks all passed. The test-only `env -u USER` workaround described above was retained on this host.

Separately, the actual final shell fixture using immutable script/executable copies passed its full native encrypted readiness flow in 62.310 seconds; it withheld public readiness until all four exchanges and restart succeeded. The independent repeat above used the same still-owned relay, and then requested its bounded supervisor to stop and reap children. Relay-owner cleanup evidence is recorded separately from native profile cleanup. Neither run is a hosted platform or command-execution result.

[Final shell-fixture evidence](linux-amd64-native-onion-fixture.json) records the immutable script/executable hashes, full native readiness result, exit zero, exact child reaping and both private temporary directories removed. Its owned parent also waited for the supervisor and removed the outer private fixture root; the script correctly does not claim it can reap itself. All owned network fixtures from these repeats have stopped. Local run identifiers in this report are fixture bindings, not GitHub run evidence.

## Static quality disposition

The final `ripwire . --quality-delta` run is not clean: it exits 2 and reports 26 gating findings and 243 new-symbol findings, including unqualified-name comparisons with historical code and duplication of small SQL, enum, random-byte and private-test setup patterns. Unchanged legacy engine methods/tests also appear in its reachability/verbosity findings. These were not suppressed by changing a baseline or deleting tests. The real new code still contains dense security validation, journal maintenance and discovery/lifecycle functions, notably `helper_exchange`, `maintain` and `discover`; these remain explicit maintenance costs accepted for this reviewed increment. The original 52-complexity effects handler was split along join/message/receipt projection boundaries while implementing recovery. Further metric-driven restructuring is deferred to avoid mixing another structural rewrite into these correctness fixes. Compiler/clippy, real production-API tests and independent code review provide the separate correctness checks; the static report is not represented as a passing gate.

## Optional legacy protocol prerequisite audit

Outcome: **unverified-prerequisite**, not incompatible. Bounded read-only source
inspection of the legacy MDK pin `37c248` found Current profile selected by default
in both `EngineBuilder` and `SessionConfig`, with legacy compatibility disabled;
neither maintained Marmish patch changes that selection. Thus profile names alone
do not establish a v0.1/v0.2 incompatibility. Upstream APIs and generations still
require an actual fresh-key Welcome and authenticated-content probe.

The existing local `wnd` executable has no adjacent provenance manifest, and the
bounded known-artifact search found no qualifying verified legacy fixture.
Neither legacy preflight nor a live cross-version probe was run. This audit also
does not validate v1 application codec vectors. The preserved Python fixture can
be exercised separately once its trusted pinned executable and required legacy
prerequisites are available; it is not a native Rust runtime dependency or a
reason to reinstate namespace/WASM requirements in v0.2. Native-to-native message
acceptance remains independent of this optional compatibility result.
