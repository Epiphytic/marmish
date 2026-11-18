# Installed Terminal runtime proof review

Claude Code used Auto permission mode with Claude Sonnet 5.5. The review scope
was the opt-in native runtime harness, its test-only mount and relay support.
The installed production CLI and daemon are covered by the integrated review.

| Round | Claude session | Disposition |
| --- | --- | --- |
| 1 | `6fa0d67a-38e1-4bd4-a738-08e510cad9de` | Revise. Offline `cargo check --tests -p marmish-native` compiled. The review found weak detach and restart assertions, a potential Busy race during resume, stale-bin and teardown diagnostics, and labels that overstated benchmark throughput. The reviewer did not run the optional native tests. |
| 2 | `8a175650-f419-4f5d-beb8-477ec82d6935` | Revise. The reviewer confirmed the round 1 harness changes by inspection. The frozen snapshot omitted the validation document and two build resources. A concurrent owner change in the frozen source also failed the library test compile. The active source compiled independently and the three non-restart native tests passed. |

| 3 | `752c1f29-fd60-44c1-87f0-cf1538de8aa6` | Approved the matching opt-in harness and support changes. Writer activation requires the separate integrated code review. |

The next snapshot requires typed Close on detach, bounds retries to Busy under
one deadline, checks exact tmux socket counts, warms ordinary Command before
measurement, and reports only serial shell display rate. Fixture teardown
completes before assertion failures are reported. The instructions require
building the matching worker binaries and limit the raw database marker claim.
