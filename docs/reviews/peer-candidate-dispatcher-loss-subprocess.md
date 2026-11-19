# Registered daemon dispatcher-loss code review

Claude Code ran independent reviews in Auto mode with Claude Sonnet 5.5.
The review covered the private fixture, all shared production refactors in
the source range, and the approved
[dispatcher-loss design](../phases/peer-candidate-dispatcher-loss-subprocess.md).

| Round | Exact reviewed source | Session | Verdict |
| --- | --- | --- | --- |
| R1 | `d7a472283ebcf5a90a2e152bfae2a907c31d0206` | `355a9e9e-b1cf-4548-a59d-235f718005a8` | Request changes |
| R2 | `775e55663ebe3e791a429c24921f1fec71417f41` | `6b035acb-99c3-40a0-9e37-79b4b86b3637` | Approve |

R1 caught the mismatch between the crate-qualified module path and the
libtest child selector. The corrected selector reaches the exact child.
The final source also samples owned ordinary relay connections before the
cut, records KeyPackage publication on the accepted ordinary write path,
and reads the private log without changing its shared file offset.

R2 checked the real startup report, stale and fresh Status closure, the
fixture-only fault controller, byte and inode witnesses, stock reopen and
the physical closure report. The reviewer independently reran the installed
fault selector. It passed in 28.14 s, with one closed ordinary WSS socket per
case, no Peer connection and no successful startup Commit in the pre-Commit
case. The reviewer used the packaged `d7a4722` production executables and the
`775e556` test executable. Only test-gated recording and assertions changed
between those sources. The reviewer found no blockers.

The reviewer noted that the 128 KiB log cap and observing an accepted
KeyPackage before startup Ready depend on this small private fixture. The test
reads a bounded log prefix and fails if the expected diagnostic or accepted
write is missing from the corresponding witness.
The caller of the private physical report accessor is Linux AMD64 only and
the accessor itself is excluded from normal builds.

See the [validation record](../validation/peer-candidate-dispatcher-loss-subprocess.md)
for the exact source-matched package and author checks.

## Independent composition review

Claude Code Auto session `15cff011-7f85-4731-846a-9776306bda72` approved
`01bbff1`. Git range-diff matched all six original patches. The review
checked the idle prelude handoff and passed stock and candidate test
compilation in separate private targets. The installed tests above
identify artifacts built on the original reviewed lineage. The
composition review did not rerun the installed layout.
