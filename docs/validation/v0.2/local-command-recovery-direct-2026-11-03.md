# Native direct running-command recovery

The local Linux AMD64 extended `command-check` passed on 2026-11-03 using fresh
file-backed identities, an explicitly selected direct WSS relay and the actual
Marmot command path. Source baseline was `25c534059a888152aea363245d147639b18640e9`
plus the recovery extension reviewed separately. This is a local direct result;
it does not establish installed cross-platform or Tor command acceptance.

[The finite report](local-command-recovery-direct-2026-11-03.json) records a
19,872 ms run. A real command remained Running with committed empty-stdin EOF
while its server owner and profile were joined, closed and reopened. The client
then attached using the original Submit request ID. Two separately authenticated
remote probes returned Existing for identical content and InvocationConflict for
changed content. Recovered Status proved the exact 14/13-byte prefixes, produced counts and gap-free
streams before the gate was released. Stop, reopen and activation took 16, 130
and 575 ms respectively. The unchanged invocation and one-line execution marker proved
one execution, followed by the exact pre/post-restart stdout and stderr and exit
code 7. This case took 6,410 ms. The intentionally unowned probe publications
retained their actual native fanouts; the fixture did not invent application
ownership or publication ACKs for them. Both foundation and command ownership
queries returned unknown for those exact retained publications.

The other cases preserved literal metacharacters and a non-UTF8 argument
(53 bytes), all byte values in 16,384 bytes of stdin/stdout (3,118 ms, 5,253
bytes/second end-to-end), and independent stderr plus exit code 7. These are
single-sample end-to-end figures, not network-only latency or throughput claims.

Final engine quiescence, joined engine and MDK stops, runtime cleanup and private
state deletion all returned true. The process exited zero and no private state
was retained. No identities, private paths, relay addresses, payload contents or
raw logs are included in the report.

The [approved recovery design](../../phases/v0.2-phase2-command-recovery-check.md)
and [round 4 design review](../../reviews/v0.2-command-recovery-check-design-r4.md)
define the fixture. Focused offline checks passed eight command-check receipt, recovered-prefix and
marker tests plus one authenticated probe-matching test; the three native debug
binaries built successfully. Strict native all-target Clippy also passed. [Actual completed-code round 2](../../reviews/v0.2-command-recovery-check-code-r2.md)
approved the revised component. Installed cross-platform acceptance remains a
separate gate.

This repeat followed the first code review fixes; completed-code approval is
linked above. The report SHA-256 is `4ee3c72ab0606590c12373c3cf74b6fd4ead107616cb7e70e2ded53a35ce139f`.
