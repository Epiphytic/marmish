# Candidate sealed Pending observation review

Claude Code Opus reviewed the design and implementation in Auto mode. The
approved design is `9c7d7b78e40222018f81819a00d78ee639a4c6f2`, session
`4b8c28a8-41a2-4d86-a385-cb5a63cdfbad`. Earlier rounds added a second physical
authority check after SQL, made refusal conditional on the current ticket and
specified freshness across suspend. Approval was recorded in `6f3ed4e` before
implementation started.

Claude approved initial source `f9684a7` in session
`7b624ccb-0af4-4a0e-9060-a126438bfb90`. Candidate execution then required a
physical fixture with a real fatal signal and native window. The follow-up adds
this only to candidate libtests. Claude independently reviewed and approved
exact source `cb468b59e5cf269707e12fc102e0d4135c669a8d` in session
`5824a0f4-1557-4292-ba83-534b954ba542`. The reviewer checked ticket order,
signed ancestry, exact same-row SQL, the final live fence and the guarded
fixture's one-use start and reap. The reviewer left runtime qualification to
the author. [Validation](../validation/peer-candidate-pending-observation.md)
records the exact source-matched run.

The pre-Pending fixture checks refusal and all three effect counters. Its
assertion leaves the exact error variant unspecified. On a wall regression,
the reader stays unavailable until the next qualified refresh. Time spent
reading SQL and native state counts toward the one-second view lifetime.
These details remain relevant when integrating the actor cadence.
