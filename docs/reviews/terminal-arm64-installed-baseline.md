# Local installed Terminal baseline review

Bead `marmish-dwt.1.5`. Reviews used Claude Code in Auto permission mode
with Sonnet 5.5. The author used a detached worktree and a dedicated warm
target for the AMD64 control. The qualification harness itself requests a
fresh target.

| Review | Exact scope | Claude session | Verdict |
| --- | --- | --- | --- |
| Design | `742d0c6a33986d8e85c083a7a3fc6237afd25b25` | `bd7b3693-4b3b-46ec-bce2-4e49c6dafcb5` | Approve |
| Code R1 | `ac6bff3ccad07b07679d943781f9d00babb11228` | `75d6f74a-08ee-4968-b4c3-0d58bfeaeaf7` | Request changes |
| Code R2 | `068cad6982b7e0574b46badf9d4806e60f9dc26d` | `fbc5e3d8-8ffe-42a9-9637-1073e562e7f7` | Approve |
| Code R3 | `bc39ea06f9503060d0db1abbb86e63de93c3d27b` | `8496ee96-aa0e-42dc-a5da-af643d75a102` | Approve |

Design review confirmed the Unix native fixture selector, package
provenance and isolation. The implementation bounds Cargo JSON capture at
16 MiB, selects the exact test library artifact, uses a short mode-0700
runtime root and reports separately what was exercised and what is unrun.

Code R1 found that the production package tool creates its own output
directory. The harness now leaves it absent and tests that contract.
R2 confirmed the fix, independently ran the six focused Python tests, and
found no blockers. It also checked the package and digest chain, exact Cargo
controller, private TLS scope, tmux ownership and runtime cleanup. The
report now includes each package asset digest and preserves the typed fixture
failure if backstop cleanup also fails. No reviewer ran Cargo or live fixtures.

The author ran both qualification Python test modules with 10 passing tests,
compiled the native library test target with locked offline Cargo, parsed
actual Cargo output to select the exact controller, and checked installer shell
syntax and whitespace. The broader native-tool suite exposed a previously
present identity diagnostic vocabulary mismatch, reproducible at `42d5c0f`
and tracked separately as `marmish-yqb`.

The first artifact-backed AMD64 control reached the native fixture and failed
at the raw PTY/tmux attach deadline. An exact repeat failed at the same
point. The direct fixture passed with its default runtime base on identical
debug binaries. The fixture owner used the harness's `TMPDIR` while the
child CLI cleared that setting, which split IPC discovery across bases.
The test now forwards the owner's temporary base into the child and
the harness uses short private siblings for IPC and PTY/tmux. R3 approved
this test-only correction, checked BSD socket lengths and cleanup ownership,
and reported 23 passing Python tests. The author then passed the actual
warmed-debug package, installer and installed controls sequence on R3.
[AMD64 validation](../validation/local-native-qualification.md).

Claude Code Auto approved exact composed source `49a740f` on the reviewed
Offer base in session `58a1083b-1ef6-473b-a346-a24873792267`. The review
checked that the integrated test harness retained the existing CLI mode,
Observe and ForegroundHandoff behavior and gave the controller and children
the same private IPC root. The Terminal patch's added and removed lines
matched the approved author source. The reviewer ran the baseline and
qualifier Python tests. The installed AMD64 result remains from the author
source documented above.
