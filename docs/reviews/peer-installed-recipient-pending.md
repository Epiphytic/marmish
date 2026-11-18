# Installed v2 recipient Pending design review

Claude Code Sonnet 5.5 independently reviewed the design in Auto permission mode. This is design approval and does not qualify implementation, deployment, or a platform.

| Round | Exact reviewed commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `d481095ab3117cd052f9368010ee498b9a8f0856` | `c8b45828-d453-4df9-b944-eea0221f805d` | Request changes |
| R2 | `a2745b93df1088608fdfbc1aab050601934eefcb` | `fee5ad79-722d-453f-8105-3f9b408bc8c6` | Approve |

R2 requires a separate private stage quota proof for the actual Welcome ingest and guarded readback. It captures per-founder mode at the atomic Held commit using the existing schema, keeps all successor work test-only, and checks one exact catalog including the terminal and cooldown objects required by epoch 3. Independent source inspection confirmed the current Generate/Reopen dispatcher cannot qualify Join. The review also checked that legacy writers and the marker-only opener cannot attach the successor, and that the same signed v2 ancestry and fresh lease/route checks govern each local transition.

Implementation acceptance includes a newly blocked or disabled selection between reservation and Held, independent listed and unlisted founders on one profile, a manual Held mode that cannot automatically activate after a policy change, isolated capped original and snapshot handles and fresh samples after snapshot preflight and before the Pending commit. The release review must check normal and `installed-checks` builds for the unchanged marker-only path and zero newly mounted recipient effects. This review used read-only source inspection. It ran no builds or live fixtures.

## Test-only schema and checkpoint implementation

Claude Code Sonnet 5.5 approved exact source `08683dee521625893d0ddeea5071f0171eab41d6` in Auto mode, session `49794a2b-0a59-4931-8181-75aac9b18efc`. The review read the full non-test diff, the five new tests, and the approved design. The reviewer's run of the author's compiled binary passed 5 custody tests and 11 installed Offer tests. The review exercised the schema and codec component without opening a live transport or building a separate release binary.

The reviewer confirmed that the release marker-only opener retains its previous checks, that the successor revalidates the exact schema and signed ancestry on each turn, and that the uncapped fixture cannot drive native Join, Pending, or terminal transitions against the successor. There were no blocking findings. The reviewer suggested additional direct assertions for refused legacy inventory shapes and inert-table corruption. The owner integration still needs to show that each turn rebinds the checked configuration and independently resolves the selected founder. See the [validation record](../validation/peer-installed-recipient-pending.md) for author checks and the remaining capped Welcome proof.
