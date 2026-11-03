# Native macOS IOKit import diagnostic and inspection correction

Failed candidate [41647399978](https://github.com/Epiphytic/marmish/actions/runs/41647399978), attempt 1, macOS job `125956606138`, source `e9a134f779da0cc0fbac5b43e6f8524f69504a0c`, reported `native inspection: unqualified Mach-O dependency` at 2026-11-03 14:08:47 UTC. The original rejection did not name the library. This failed candidate is not acceptance.

Separate diagnostic [41652314447](https://github.com/Epiphytic/marmish/actions/runs/41652314447) succeeded building the same product source using diagnostic workflow HEAD `9f57f22fc9fea91da0cf68543105db7947f5c46a`. The product-source binding is printed in its log; the parent verified run/workflow metadata. At 14:28:39–40 UTC, `marmish`, `marmishd` and `marmish-worker` each reported ARM64, minimum macOS 15.0, SDK 15.5, and exactly these imports:

- `/System/Library/Frameworks/Security.framework/Versions/A/Security`
- `/System/Library/Frameworks/SystemConfiguration.framework/Versions/A/SystemConfiguration`
- `/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation`
- `/System/Library/Frameworks/IOKit.framework/Versions/A/IOKit`
- `/usr/lib/libiconv.2.dylib`
- `/usr/lib/libSystem.B.dylib`

The exact Apple system IOKit path was the sole import absent from the original five-path inspector allowlist. The [approved amendment](../../phases/v0.2-native-iokit-inspection.md) adds only that literal. Unknown/external/near-match paths still fail; no wildcard, runtime change, ABI-baseline change or ELF policy change was made. Rejection diagnostics now name the product and sanitize/bound the displayed static dependency; missing libSystem and minimum mismatch have fixed attributed messages.

Actual local build-tool validation:

- Seven offline inspector tests passed, including the observed six-import list on all three products, per-product external/near-match rejection, bounded/control-safe diagnostic, absent libSystem/minimum, minimum 11.0 rejection, and unchanged ELF ABI/dependency vectors.
- `sh -n scripts/v02-inspect-native.sh` and `git diff --check` passed.
- Root repeated the seven tests and static checks successfully. `ripwire . --quality-delta` exited 2: the existing fake-tool fixture grew from five to eight parameters and the unittest class from 35 to 88 lines; four discovered unittest methods were classified as dead code. These reflect the explicitly reviewed per-product result fixtures and added refusal coverage; no runtime symbol changed. No clean quality-delta claim is made.
- [Actual Claude auto completed-code review](../../reviews/v0.2-native-iokit-inspection-code-r1.md) approved. The reviewer independently repeated all seven tests/static checks and caught removed-IOKit/removed-length-bound mutations on temporary copies.

Reviewed SHA256:

- Inspector: `c41c7088d8c190096a985df2c94ae2703e9730eb1ab85a7da172926329bf9831`.
- Tests: `904a395a07b73b9b4b05e7725c5125b728134fe41e26bbf6f6db4b8528108601`.

Fake-tool tests verify inspector policy, not actual artifact qualification. No Cargo builds, live transport, installed/provider acceptance or release publication were performed for this correction. A fresh whole candidate must validate the real binaries and remaining release gates; neither the failed candidate nor diagnostic build substitutes for it. Raw logs are retained privately outside the repository; no runtime identities or secret state are included here.
