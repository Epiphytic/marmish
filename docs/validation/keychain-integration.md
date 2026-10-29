# K2 native key integration validation

K2 integrates named keys, automatic native selection for new profiles, explicit
migration, confined MDK bootstrap, provided SQLCipher masters and private worker
handoff. Independent completed-code review is in progress. Real native-provider
Marmot acceptance is the separate K3 phase; these offline results do not imply
live transport validation.

The final integrated Linux workspace test suite passed 153 primary tests and four
self-reexecution children, including five default-build
native-storage/worker tests (six with fault injection enabled), bridge strict-secret framing and nine bridge
integration tests. Python tools passed 194 K2 tests with five Darwin-only skips;
the named-key controller separately passed 21 tests. Workspace all-target Clippy
passed. Eight feature-enabled engine fault-injection tests also passed using
independently preserved production and feature worker executables. After review fixes, the six-test native fault suite also passed twelve actual
crash-and-retry publication boundaries. Formatting, workspace tests, all-target
Clippy and the 21 catalog tests passed again. The working K3 harness adds 24
separate offline tests; those also passed, but are outside this increment.

The clean pinned MDK checkout, bootstrap patch and source guard passed three
Rust bootstrap tests and a release build. The initial executable digest was
`eb118247dd7867a084021701baf2fda04f53c80e76b7c986897aeeba9535a6ac`; its v2 manifest
verified. Review-requested stream authorization and complete source attestation
supersede that artifact. The rebuilt Linux daemon SHA256 is
`d1a2eefa9f5e5593040adae59f1285150de1edacd3a6bde50cc18bfae01429f2`;
its sibling v2 manifest verifies. All three exact-name bootstrap tests executed
and passed in the clean pinned checkout. The bootstrap patch digest is
`512caebb5e6352d4e5589624d74a7197724597d01796ad53e6d09df015d49621`. This is build evidence, not a live daemon acceptance claim.

Darwin helper all-target cross-check and Clippy passed with the Darwin Clang
selection. Native inherited-pipe/Seatbelt and frontend acceptance remain pending.

The K1 workflow at commit `392e8e608b5865a6275e24dea6a46a9a58c8f7ac`
[passed Linux x64](https://github.com/Epiphytic/marmish/actions/runs/40971992731/job/123790722555).
The [macOS ARM job](https://github.com/Epiphytic/marmish/actions/runs/40971992731/job/123790722529)
passed its actual disposable-keychain provider acceptance, including restart and
replacement checks (one outer test, 0.49 seconds). Its subsequent confinement
prerequisite failed `native_spike_failed`, with insufficient stage diagnostics
to establish the cause. The [Linux ARM job](https://github.com/Epiphytic/marmish/actions/runs/40971992731/job/123790722702)
failed the namespace probe with `userns_denied_private_bwrap`. Both ARM jobs were
assigned, checked out source and ran setup. Their later tests/builds and live
interoperability did not run. These are runner-gate results for K1, not K2 native
acceptance.

Static quality checks identify lifecycle growth, Rust/serde/test dynamic-discovery
false positives, historical churn and normalized short-helper similarities. The
real initialization/dispatch and source-walk complexity increases are addressed
by separating initialization, account preparation/authentication and source enumeration; the final scan reports no
existing-symbol complexity regressions. Class-level size, new lifecycle code,
short normalized helpers and historical churn still produce exit 2. No suppression or new
baseline is used. The remaining size/parameter findings reflect the added lifecycle and explicit
private-handoff interfaces; normalized test/helper similarities are intentionally
kept separate where they exercise distinct boundaries. No dead-code deletion is
justified for serde visitors, native entry points or dynamically discovered tests.
K3 harness complexity is outside this increment and is evaluated in its review.

Initial independent Claude review requested recovery and coverage changes; see
[the review](../reviews/keychain-k2-code-initial.json). The follow-up approved
the Linux increment with every medium finding resolved. Completed changes:
resume interrupted Login before setting initialized, run catalog tests in CI,
exercise marker publication faults and torn pending recovery, bind worker launch
to the held root, broaden negative framing/confinement coverage, and document
native macOS inherited-pipe acceptance as a distinct pending gate. Unrelated
lockfile downgrades are restored. Stream subscriptions receive the same injected
identity authorization as Execute; source attestation includes SQL/include files.

The review's documentation condition is resolved here: the final daemon digest,
five default/six feature native tests and final checks are recorded above. A
post-review behavior-preserving extraction separates native preparation from
authentication in `Profile`; its 27 tests passed again. Both platform patch/guard
combinations apply and reproduce in clean source checkouts; this static evidence
does not validate Darwin execution. Native macOS remains pending.
