# Retained reference-suite failure diagnosis

Manual [run 42640487074](https://github.com/Epiphytic/marmish/actions/runs/42640487074),
attempt 1, tested source `fa119c6e876f3bed779945d0a3daee4275437058`.
All three jobs ended in failure. Exact-source admission and offline contracts
passed; both report and ciphertext uploads succeeded on each platform. The
diagnostic preserved the failing acceptance result. This is not native release
qualification or a passing reference suite.

The integrator checked the six artifact IDs, names, source, API sizes and SHA256
digests, then required exactly one bounded regular member per ZIP. Decryption
used the private recipient key locally; only the public certificate was
committed. Plaintext remains private, with no downloaded code execution.

| Platform | Cipher artifact | Plaintext bytes | Plaintext SHA256 |
| --- | --- | ---: | --- |
| Linux AMD64 | 13684972865 | 168085 | `bbf20a278efe37459b3fe449c9fab47b87cb8b4ba9db94adc2931a4a724c72a7` |
| Linux ARM64 | 13684289694 | 168091 | `b8343692aca6b552277c8643da4e46a6dd8ec6b62e350ffb82dd787d72eeb213` |
| macOS ARM64 | 13685291553 | 115 | `7d44ac0ef8ac2480a24b5a8dc4140d8a8176f2309d2e3734bc9d03472a055287` |

Both Linux suites ran 996 tests with 11 skips and exactly two import errors.
`test_marmish_macos_denial_workflow` and
`test_marmish_release_automation_workflow` import `yaml`, which is absent from
the isolated managed interpreter. Both appear as `unittest.loader._FailedTest`;
the prior empty failed-test identifier list did not establish success.
Recorded suite durations were 44.816 seconds on AMD64 and 44.212 on ARM64.
These are test-suite durations, not Marmot command latency measurements.

macOS failed before suite execution. The private compiler output reports that
`stable-aarch64-apple-darwin` is not installed and suggests installing a Rust
toolchain. This establishes a toolchain-selection failure, without establishing
which selection step introduced it. It supplies no evidence of a dynamic-loader
failure. Correction and fresh validation remain required.

Actual diagnostic OpenSSL identities were Linux `/usr/bin/openssl` 3.0.13 and
macOS `/opt/homebrew/bin/openssl` 1.1.1w. Successful encrypted uploads establish
availability for these jobs only. CMS encryption provides confidentiality;
artifact metadata and verified digests provide the recorded source/run binding.
No authenticated-encryption claim is made for CBC alone.

The native macOS owned-process cleanup failure is separate. Neither these
reference tests nor this diagnostic qualify version 0.3.1. Public installation
still selects 0.3.0, and automatic publication remains held.
