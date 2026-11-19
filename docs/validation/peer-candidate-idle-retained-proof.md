# Registered idle stop with retained Command results

Linux AMD64 validation used exact source
`b1a76b864e2c754f5ddfdd43dddbbb1577864f96` and two isolated local
installer directories. Both artifacts had that source stamp. The candidate
build used `--cfg marmish_peer_receiver_candidate` and packaged with
`--peer-receiver-candidate`. The stock build omitted both options. Each
installer verified its manifest digest and installed its own layout. The
tests used disposable identities, distinct private loopback WSS endpoints
for ordinary Command and Peer qualification, SQLCipher, and real worker
processes.

| Artifact | SHA-256 |
| --- | --- |
| Candidate install manifest | `b0c0286400aa2518388f44b75ad1a2285512620cbdac56bb86a97d7461bd19a7` |
| Stock install manifest | `8cd1c7d83e0f3350ee56dcd0782faeea4fdc337ec52d9ff1faba322f3eb5117e` |
| Candidate libtest | `fdb50070b839c336ccb92e748a12866b65bed5678bb3eb1d13e89534b4725272` |
| Stock libtest | `6e44e9693520714c1f5da86a08aafe62310daf64d71bb096efad04d2461e7b64` |

## Installed checks

The ignored `installed_registered_both_retained_results` selector drives
two ordinary commands to completion through the installed candidate.
It verifies the distinct input bytes and exactly one local side effect
for each invocation. After a quiet relay interval, the test sends one
SIGTERM, requires exit zero, checks local socket removal and reacquires
the profile lease. It then requires the separate stock process to
observe both exact retained results over local IPC and exit cleanly.
The test checks the same side-effect marker again after stock reopen.

The complete retained-results test passed in 29.99 seconds. These are
the elapsed times from that run.

| Stage | Milliseconds |
| --- | ---: |
| Registered Both startup | 6,282 |
| First remote command and byte readback | 3,699 |
| Second remote command and byte readback | 1,998 |
| Wait for the quiet relay interval | 756 |
| Candidate SIGTERM to process exit | 103 |
| Cold stock reopen and exact local IPC readback | 295 |

The test’s total includes disposable profile and relay setup and
teardown. One observed run is insufficient to estimate variation.

The `retained_rows_advance_at_exact_idle_final_observation` selector
independently checks each row’s durable observation advances across the
same idle fence, waits for cleanup to report quiescence, and asserts
the actual join of the worker thread. It passed in 11.03 seconds
with both rows checked. The
`installed_registered_fresh_exec_and_command` selector covers fresh
installed startup and a finite ordinary Command on the same candidate
build. It passed in 40.09 seconds, with readiness at 6,281 ms and
command execution and result readback at 5,175 ms. The stock
relay-outage test passed in 23.58 seconds with the validated layout.

## Build checks

Both classes passed locked offline `cargo clippy -p marmish-native
--all-targets -- -D warnings`. Candidate and stock linting each took
28.85 seconds. The stock libtest passed all 16 `command_service`
tests and both `idle_cleanup_` tests. The changed
Rust files passed rustfmt and `git diff --check`. Workspace format
testing still reports the existing formatting change in
`command_engine_admission.rs` present at the base.

An earlier run executed both real commands and stopped the candidate.
Cold reopen failed with `install_manifest_class` because the stock
child was handed the candidate installation. The correction installs
a separate stock layout from the same source and preserves an explicit
test of that refusal before reopening the unchanged profile. The next
run passed the separate layout check but failed with
`worker_installation` when Command resolved its copy helper from the
stock libtest executable. The fixture now validates the stock layout and
passes both canonical binary paths into the existing Command startup.
The commands exercise ordinary execution and retained result readback.
The fixture checks the copy-helper path supplied to startup. The
[review record](../reviews/peer-candidate-idle-retained-proof.md)
identifies the independently approved correction. This is private
Linux AMD64 qualification evidence for the registered Both stop.

## Composition with the private passive recipient

The installed comparison used source stamp
`358662019ee0e3233ea1634a73ca4760a639cfb9` in both artifacts. Each native
installer was given its own manifest digest and prefix. The same source
patches replay onto the published passive-recipient evidence as
`bf22fc1`; Git range comparison reports all 11 patches equivalent and
the native source, scripts and CI configuration match byte for byte.

| Artifact | SHA-256 |
| --- | --- |
| Candidate install manifest | `a67b4a4df48c9d8009068bd4f774641d1f2ccc826daf72f3b54586deb1fd351b` |
| Candidate libtest | `5c5043c86e1e9a9a97a7b3bcaa5fb1695c8aca867616d08665991826b7faca1d` |
| Stock install manifest | `165acc4737b27260d52c913752519f549b9050fe09b411c2301e4ec097c7fa86` |
| Stock libtest | `667a8e93365c66a461c496b483f4d3a7502966bce22c6665b59320a483b0bdbb` |

The retained comparison passed in 29.64 seconds. The first and second
command round trips took 3,605 ms and 1,770 ms. The candidate stopped
in 208 ms and the stock process read both results in 295 ms after
reopen. Registered readiness took 6,069 ms and the quiet interval
took 837 ms. These are single-run private loopback observations.

The fresh installed Command reference passed in 39.65 seconds, with
readiness in 6,056 ms and Command in 5,395 ms. The exact idle fence
observed both rows and joined the worker in 11.22 seconds. All 16 stock
Command service tests and both stock idle cleanup tests passed. The
independent passive recipient selector passed in 33.12 seconds and
exercised real Command both before and after the Peer connection closed.
The two original registered dispatcher-loss scenarios passed in 25.37
seconds, with Ready and Status cuts at 12,642 and 12,721 ms. The earlier
intermittent refusal remains recorded in the
[passive recipient proof](peer-candidate-recipient-lifecycle.md).
Strict all-target Clippy passed for the exact stock and candidate source
in 30.84 and 29.77 seconds respectively.

Claude independently reran the retained comparison with the same
immutable unsigned artifacts in a separate private temporary home. It
passed in 31.18 seconds. The two command round trips took 3,831 and 2,066
ms, the candidate stop took 156 ms, and stock cold reopen took 332 ms.
The reviewer verified the embedded source stamps, the installed
executables against their manifests, and both installed test binaries
against their staged copies.
