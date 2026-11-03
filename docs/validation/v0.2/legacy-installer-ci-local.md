# Retained legacy installer CI: local validation

Beads `marmish-21i`; isolated base
`39e402773066403d69b814e8b933969d59f7ef16`. This is **legacy v0.1** test/reference
repair, not native v0.2 installation or protocol acceptance.

The public native bootstrap SHA256 remains
`46b4f369792f4f4ea36726c24414c5cca2ef977fec61712d821a1b81f985367a`.
The retained fixture is byte-identical to pre-cutover git blob
`5294acb6cc474a89e4c2de4f51c9f429b188d023`, SHA256
`eac417b4748f0252adef82fe18c1260ef744523ec0ed6b624927dc6b803d0768`.
No public installer, product, dependency or runtime code changed.

Local Linux offline commands and actual results:

| Command | Result |
| --- | --- |
| `python3 -m unittest discover -s tools -p test_marmish_install.py -v` | 34 passed; 0.659 seconds |
| `python3 -m unittest discover -s tools -p test_marmish_quickstart.py -v` | 18 passed; 2.260 seconds |
| `python3 -m unittest discover -s tools -p test_marmish_managed_runtime_spike.py -v` | 79 passed; 11.464 seconds |
| `sh -n tests/fixtures/legacy/install-v0.1.sh` | passed |
| `sh -n scripts/install.sh` | passed |
| actionlint 1.7.12 on `installer-validation.yml` | passed |
| `git diff --check` | passed |

The added archive regression pins the reference path, SHA256 and unique embedded
Python markers. Existing archive, controlling-terminal and managed security
assertions remain. The managed extractor now reads the pinned legacy reference;
its byte-exact test compares the extracted output to the same fixture.

`ripwire tools --quality-delta` reported growth of the existing test class by the
12-line provenance regression and a dead-code false positive for its dynamically
discovered unittest method. No production abstraction or control-flow complexity
was added; no test-class refactor was introduced for a reference-path correction.

This did not execute the managed proof's real pinned downloads, dispatch hosted
CI, use Cargo, open a provider or exercise live transport. The retained workflow
still has all three runner targets and its existing download proof. Actual
Linux ARM64/macOS and hosted network results remain pending. Native HTTPS,
provider and setup/PTY qualification remain separate.

Actual independent [Claude code review](../../reviews/v0.2.1-legacy-installer-ci-code-r1.md) approved the increment and independently reproduced all 131 tests, shell syntax checks, modern workflow lint and diff check. Its local test durations were 0.572s (archive), 2.464s (PTY) and 11.460s (managed). Its hosted/network limitations remain.

## Root integration

Integrated byte-identically atop47d26a0 after the canonical helper fix. Root independently reproduced34archive+18PTY+79managed unit tests, sh-n both installers, actionlint1.7.12 and diffcheck. Native public bootstrap SHA256 remains46b4f369792f4f4ea36726c24414c5cca2ef977fec61712d821a1b81f985367a. Ripwire delta reports five gating rows: four recent-churn rows for the retargeted loader symbols and test, plus12line test-class growth from the provenance regression. These are reviewed, narrow legacy test changes; no product complexity change, suppression or artificial baseline. New unittest is flagged dead by the static graph despite its actual execution. Actual hosted six-job legacy validation remains pending.
