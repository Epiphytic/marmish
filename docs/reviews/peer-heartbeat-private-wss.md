# Private WSS heartbeat provenance review

Claude Code ran with `--permission-mode auto` using Claude Sonnet 5.5.

| Session | Scope | Disposition |
| --- | --- | --- |
| `e9dcd988-dbc6-4c3e-b47e-8057dbcb8dd9` | Benchmark workload amendment with native 30-second retry and 600-second cap | Approved 12 fresh native samples and separate reporting of pre-existing local measurements |
| `af575dff-55ae-47f9-8dc4-f2a4b71fd96f` | First code review | Requested changes because sender refusal cases exercised a permission predicate outside the armed transport adapter |
| `e6ee4b72-c685-43ca-8c8b-3bf53e76014f` | Exact source `031edb6db380d92f9532d7173bf6e5fa62410ec5` | Approved with validation and documentation follow-up |
| `dab94c59-e053-43dc-b682-ae1d2a2428fb` | Documentation-only workload accounting | Approved the corrected seven plus eleven split with the same positive and egress caps |

The final reviewer verified that the cloned adapter shares the runtime
adapter's state, consumes each armed refusal with a typed Closed result
and rechecks current authority, endpoint and deadlines after acquiring
the socket lock. The seven refusal turns leave native attempt count 1
untouched. The positive retry and duplicate ingress go through the native MDK runtime. The review also checked
the same-handle direct database reads, callback watchdog, one accepted
WSS write per positive reservation, environment isolation, percentile
calculation, and source confinement to tests.

The reviewer requested exact source test results and a negative non-test
compiler probe. The [validation record](../validation/peer-heartbeat-private-wss.md)
includes both, describes the authority-scope endpoint negative and the
absent-authority pre-acceptance case, and records the 18 negative
invocations by layer. A dedicated comparison of room counts is outside
the replay assertion scope. The heartbeat replay asserts retained native
Duplicate and unchanged peer projection and permission state.

## Integrated MDK master review

Claude Code Auto session `7119f1d4-efce-408b-95c6-429ad7a2a61e` approved
the exact integrated source `6fe45ed` with MDK `b3cd2cb1`. An earlier
source inspection required current-lock test evidence. The final review
compared the actual two-record and twelve-record WSS results, focused
tests, strict Clippy and production exclusion probe with the updated
validation record. The reviewer used static inspection and existing
logs. The controller ran the Cargo checks and the private WSS tests.
