# Installed Peer relay qualification review

Claude Code ran with `--permission-mode auto` and Claude Sonnet 5.5.
The [phase](../phases/peer-installed-heartbeat-first.md) author owns the
focused Linux [validation](../validation/peer-relay-qualification.md).

| Session | Reviewed scope | Disposition |
| --- | --- | --- |
| `5e24f6ca-aa4a-45bd-a523-020b42b91d6a` | Installed Peer design and relay CLI contract, original final blob `b1304c7` | Approved after revisions to production boundaries, CLI grammar and qualification evidence |
| `2dbdbefc-4773-4569-a83b-dc15a6a4d713` | Initial code, `3cad1d0` | Requested exact relay refusal classes, targeted validation and a bounded wall clock |
| `25569d5b-609d-469b-8c8f-5dd404fe99e8` | Revised code, `08ec7c9` | Requested an enforced socket schedule and fault coverage at each observed relay stage |
| `23bd973d-ac60-49cc-ad2f-c3c0da41d828` | Design amendment `d5576b8`, blob `5e02de6` | Approved strict four-open sequence, one active socket and typed check failures |
| `7ddbfd3c-f43b-47ff-95f8-4d40485a84c9` | Exact implementation `9159298` against `dbe3605` and amended design | Approved with no blockers |

The final reviewer inspected the full integrated source and tests and
reviewed the final corrective increment separately. They confirmed that
the adversarial fixture faults each of the 19 probe-check categories,
that the empty wide-query fallback requires independent positive history
on the same authenticated socket, and that four ordered opens, one active
socket, frame and byte limits, and the deadline gate completion. They
also checked that diagnostic output uses fixed categories, TLS remains
bound to the exact profile and configured origin, and the shared fixture
changes only activate when a Peer qualification test opts into them. The
reviewer used static inspection. The author ran the native tests and
strict Clippy.

Optional test refinements remain for repeating wrong refusal classes
across every read, a distinct outsider wrong-AUTH-ID fault, and altering
exact bytes on reconnect as well as on the first authenticated history
page. The current checks share the same validation functions and already
exercise every fixed stage category. The probe conservatively counts a
Ping and its outgoing Pong separately. The fixture's no-fault-marker
Group test disconnects the actual socket.

## Integrated review

Claude Code Sonnet 5.5 approved exact composed source `738d1be` in Auto
session `0babd060-1f8b-48d4-96ec-857333775e52`, conditional on the native
suite and opt-in CLI test passing. It inspected all 16 changed files,
including local grammar, confirmation before any network I/O, owner-file
and TLS binding, traffic and socket budgets, refusal messages, atomic
record issuance and the shared fixture's opt-in failure modes. It ran no
extra tests. All changed Rust paths were byte-identical between the
reviewed author and composed revisions.

The documentation now describes the observed reconnect history and the
independent authority required at runtime. Outstanding refinements include
actual expiry mid-probe, SPKI mismatch through the CLI, and live malformed
frame coverage. Ripwire recorded higher complexity in the test relay's
fault branches. The review confirmed they are inert in earlier fixture
modes. The normal Group and Terminal cases passed in the author run.
