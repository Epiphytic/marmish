# Candidate e9686c0 native verification and packaging

[Candidate run 42367702241](https://github.com/Epiphytic/marmish/actions/runs/42367702241), attempt 1, completed **PASS** on 2026-11-07 (final render job ended at 12:14:56 UTC; run `updated_at` proxy is 12:14:57 UTC) for exact source `e9686c0799e78010c350d4990406c93ddb8bc474`. All three package jobs and the final provenance/manifest-render job completed successfully. The [structured evidence](streaming-candidate-e9686c0.json) records complete job/step conclusions, private log hashes and GitHub artifact API metadata.

| Platform | Default native library: pass / fail / ignored; runtime | Installed-feature native library: pass / fail / ignored; runtime | Engine library: pass / fail / ignored; runtime |
| --- | --- | --- | --- |
| linux-amd64 | 610 / 0 / 3; 867.26s | 791 / 0 / 3; 812.89s | 138 / 0 / 1; 9.52s |
| linux-arm64 | 610 / 0 / 3; 846.88s | 791 / 0 / 3; 876.93s | 138 / 0 / 1; 9.38s |
| macos-arm64 | 609 / 0 / 3; 579.44s | 790 / 0 / 3; 632.85s | 145 / 0 / 1; 35.92s |

These are separate, overlapping suites, not additive totals. Ignored tests are excluded from passes. Suite runtimes come from Cargo test result lines and exclude compilation. They are neither relay latency nor transfer speed measurements.

Each platform passed strict all-target Clippy for the default native/engine crates and the installed-checks native variant, built release product binaries, inspected dependency/ABI allowlists, collected locked licenses, packaged immutable product assets, then built and separately packaged installed-test tools. macOS product executables were ad hoc signed and strictly verified before hashing; separately packaged tools were signed as well. Signing here does not mean notarization. Successful before/after SHA-256 inventory comparison confirms that building the installed tools preserved the packaged product assets.

The final render job passed the complete provenance tuple check and rendered all three pinned manifests. Eight workflow artifacts were uploaded: three product packages, three installed-tool packages, one host render tool and one pinned bootstrap. Their exact IDs, compressed sizes and API-provided ZIP SHA-256 digests are in the JSON. This harvest checked the API metadata and complete job logs; it did not independently download or execute those artifacts.

This result establishes native verification and candidate packaging. It does **not** establish installed cross-platform client/server qualification, completion of the eight installed cohorts, or resolution of the earlier cross-host multiple-identity macOS server failure. It changes no public release or installer pin. Raw logs remain private.

Independent evidence review: Claude Code `claude-sonnet-5-5`, `--permission-mode auto`, session `2e0a6cdd-1666-4b14-904c-e60dfd44c74a`, **APPROVE**, 83.746 seconds and 12 turns. The reviewer checked the exact run, job, artifact and log hashes; all nine main suite counts/timings; build/Clippy durations; signing order; unchanged product hashes; and all eight API artifact pins. No blocking findings. Completion-time proxy and workflow-artifact wording were clarified after review. The render provenance claim rests on the successful checked-in step; no separately logged tuple output is claimed. This review executed no product or relay workloads.
