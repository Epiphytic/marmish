# Native v0.2.1 corrective release — published with qualification caveats

The user explicitly authorized publishing the corrected quickstart for an
out-of-band macOS test before complete platform qualification. This record does
not assert full cross-platform qualification. Observed installation,
Linux provider and installed-command results are bounded below. Its release facts remain separate from the historical
[v0.2.0 release evidence](initial-release.md).

## Frozen source and candidate

Reviewed source: `7c49c3785eab010bf017c1bb7f03511524312b95`.
Native package version is 0.2.1. The public bootstrap now selects the matching 0.2.1 assets.

[Full candidate 41698628787, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41698628787)
completed with overall **FAILURE**. All three package jobs, render and all three
installed quickstart jobs passed. Both Linux provider jobs passed. The macOS
provider job failed in its offline supervisor tests (19 tests, three errors),
before invoking the real Keychain qualification; no macOS provider report was
produced. This is not an observed Keychain-provider failure. The user-authorized
corrective publication threshold of three successful packages plus render was
met, while complete platform qualification remains unfulfilled. No guards were
changed. The existing HTTPS resolver requires whole-candidate success and thus
cannot qualify this failed candidate.

The previous full candidate
[41683843521, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41683843521)
built source `c05ab00e93be75a01c81296d65abce249aeccfdf`. Both Linux package jobs
passed. macOS passed 351 default native tests, then its installed-tool suite
passed 445 and failed one matrix child fixture timeout. This invalidated the
attempt as whole-candidate acceptance. Its passing steps do not establish the
new source's results, nor is a timeout fixture correction a production deadline
change.

The prior candidate
[41680669762, attempt 1](https://github.com/Epiphytic/marmish/actions/runs/41680669762)
built source `ed4b2ed0cfae92a2addb71afe8c84dcadef9e93c`. Its macOS default native
unit run passed 349 tests and failed one test-only child-marker parser fixture.
That failure invalidated the attempt as release acceptance. The reviewed fixture
correction is present in the fresh source above. Earlier individual Linux steps
are diagnostic evidence only and are not stitched into the new attempt.

## Observed CI verification times

Root observed these completed `Verify native components` step timestamps in the
same source/run/attempt above. All times are UTC on 2026-11-03.

| Target | Verification started | Verification finished | Elapsed | Outcome |
|---|---|---|---:|---|
| Linux AMD64 | 18:57:28 | 19:25:40 | 28m 12s | PASS |
| Linux ARM64 | 18:57:27 | 19:15:07 | 17m 40s | PASS |
| macOS ARM64 | 18:57:36 | 19:09:06 | 11m 30s | PASS |

These are CI build/test/Clippy step durations on different runners, not remote
command latency, relay latency, bulk throughput or measured installation time.
They include compilation and a test workload and are not an architecture speed
comparison. Optimized product builds and installed-tool compilation are separate
steps. All three installed-tool stages and package jobs subsequently completed successfully.

## Installed quickstart and provider reports

The exact run/attempt artifacts were downloaded independently and checked against
closed success schemas, expected finite values and positive bounded elapsed
milliseconds. Provider reports bind source, target, run and attempt internally;
quickstart reports do not contain those fields, so their binding is provided by
exact artifact names and the run's immutable head SHA. Original finite reports
and their SHA256 values are retained in
[the qualification binding record](corrective-release-0.2.1/qualification-bindings.json).

| Target | File-backed PTY quickstart | Real default-auto provider |
|---|---:|---:|
| Linux AMD64 | PASS, 5,629 ms | PASS, 19,062 ms |
| Linux ARM64 | PASS, 5,613 ms | PASS, 17,244 ms |
| macOS ARM64 | PASS, 4,812 ms | Not invoked; supervisor tests failed |

Quickstart proved default both/no-grants disables inbound execution, repeat setup
keeps the same identity, joined stop completes and owned state is removed. Its
explicit file storage does not establish Keychain custody. Both Linux provider
reports prove distinct named identities, fresh reload, locked refusal, unlocked
reload, no plaintext fallback and owned child/state cleanup. Provider supervisor
failures and their cause analysis remain separately tracked.

## Installed Linux command execution

Root installed the actual generated installer against the downloaded candidate
assets on Linux AMD64 (exit 0). All three installed release-info records matched
source `7c49c3785eab010bf017c1bb7f03511524312b95`, version `0.2.1` and format tuple
`[2,6,1,1,1]`. A subsequent installed command-check used generated keys and an
explicit direct relay with Tor off. It passed in **19,882 ms** report time
(**19,889 ms** outer wall time), with this finite
[command report](corrective-release-0.2.1/installed-command-check.json):

| Case | Output bytes | Elapsed | Observed output goodput |
|---|---:|---:|---:|
| Running-command recovery | 27 | 6,856 ms | — |
| Literal argv | 53 | 1,545 ms | — |
| Binary stdin | 16,384 | 2,689 ms | 6,092 B/s |
| Stderr and exit 7 | 0 stdout; 12 stderr | 1,713 ms | — |

Recovery observed one execution, matching recovered output, identical-submit
attachment and local/remote conflict refusal. All engine quiescence, joined
engine/MDK stops and runtime/state cleanup flags passed; no local state was
retained. The probe explicitly reports `purpose_ownership_unknown=true`, so this
is not proof of automatic probe-room deletion. `probe_native_retained=true`
records the deliberately retained native probe obligation during the conflict
check; it is a passing fixture signal, not a deletion claim. This is one installed Linux
protocol check, not the full public-CLI matrix or cross-platform lifecycle gate.
Historical v0.2.0 measurements remain unchanged and separately labeled.

## macOS package completion

The same-source macOS package job `126135726239` completed **SUCCESS**. Root
retrieved its terminal job log through the jobs REST endpoint while the overall
workflow was still running. Its native default test summary was **351 passed**
in **168.16 seconds**; its installed-feature summary was **446 passed** in
**220.93 seconds**. Both the legacy diagnostic child fixture and matrix deadline
fixture explicitly passed. These test execution times exclude preceding compiler
work and therefore differ from the whole CI step duration above.

Strict linting, release/tool builds, final ad-hoc signing, ABI/import inspection,
license collection, packaging and artifact uploads passed for macOS. Product
artifact ID: `13265453071`; separate installed-tool artifact ID: `13265258332`.
Root downloaded all 15 release assets and independently checked their manifests
and hashes. This successful macOS package is not proof of installed real
provider custody or a successful whole workflow.

## Publication and qualification boundary

The [v0.2.1 prerelease](https://github.com/Epiphytic/marmish/releases/tag/v0.2.1)
is public (release ID `433170913`, draft false, prerelease true). All 15 exact
asset names, server sizes and SHA256 digests match the verified catalog. Annotated
tag `8c03656861340066119bf89b139dddceec4c8488` resolves to the frozen source on
GitHub and Radicle. All three public manifests matched anonymous downloads.

Bootstrap-only commit `22014b98a415ecf05b422ac2c754dc18b3311d46` is a direct
child of SOURCE and changes only four generated constants in `scripts/install.sh`.
Its rendered and independently fetched Iris/GitHub bytes match SHA256
`ce693bc2977f7473ce0dd0c9c510dc3e84b989644c61be06072837ced947a018`;
POSIX shell syntax passed. Both branches on both mirrors were verified.
The bootstrap quality check exited nonzero (exit 2) and reported one historical short-horizon-churn finding
(main 0→4); root manually dispositioned it as the four generated pins, with no functional/API
or complexity change and no suppression or baseline adjustment.

A real Linux AMD64 public-asset install (no asset-directory override) passed in
10,285 ms, selecting active generation 2 and matching all four installed asset
hashes. See the [public install report](corrective-release-0.2.1/public-install-verification.json),
[public script verification](corrective-release-0.2.1/bootstrap-url-verification.json),
[release asset verification](corrective-release-0.2.1/release-server-verification.json)
and [asset catalog](corrective-release-0.2.1/asset-catalog.json).

The existing HTTPS whole-success guard blocks this failed candidate; no six-case
v0.2.1 HTTPS run was dispatched and no guard was bypassed. The later CI-only
supervisor repair at `6010511907995a359c5a8543eaaa26e4ce45475b` was independently
reviewed and published after bootstrap cutover; it does not change release assets
or retroactively qualify their Mac provider behavior. Existing v0.2.0 assets and
measurements remain unchanged.

The user-authorized corrective release is not contingent on full command-matrix,
OS-restart lifecycle, benchmark or provider acceptance. Those outcomes must remain
explicitly incomplete where unobserved. A file-backed setup check does not prove
real default-auto keychain custody. Tor remains optional and experimental.
macOS distribution is Apple Silicon, macOS 15+, ad-hoc signed and not notarized;
Linux targets require glibc 2.39+.

Automatic upgrade replaces only recognized owned legacy launchers, retaining old
runtime files, launcher backups and private state. It neither migrates old
profiles nor performs automatic disk cleanup; unrelated launchers are refused.
The user's actual macOS upgrade result is independent new evidence.
