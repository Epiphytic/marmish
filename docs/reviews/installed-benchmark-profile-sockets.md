# Installed benchmark profile socket review

Claude Code reviewed the [phase design](../phases/installed-benchmark-profile-sockets.md)
in auto mode, session `36f72a65-2996-42bc-8630-a3f6e0037262`. It approved reuse
of the existing profile socket resolver and confirmed the daemon runtime
ownership and authenticated readiness requirements.

Claude Code approved the integrated staged patch in auto mode, session
`6c40a334-051c-4477-a30c-1a82079c4b6a`. The reviewer ran rustfmt, strict Clippy,
ten benchmark tests, and four socket tests including authenticated IPC to a
real daemon. The author independently ran all 269 installed-check unit tests,
strict installed-feature Clippy, the staged ontology check and Ripwire quality
delta. All passed.

## Installed Linux AMD64 check

The original checker from candidate `45699269409/1` reproduced `activation`
failure on the installed v0.3.1 packages and pinned bootstrap. Both daemons
bound sockets under their child `TMPDIR`. The checker polled separate paths
and recorded zero command samples.

Using the same archived product binaries and bootstrap with a locally compiled
corrected checker in a disposable context, all four commands passed through
`wss://relay.us.whitenoise.chat/`. The cold command completed in 46.26 seconds
and the three warm commands in 40.01–43.31 seconds. Verified application output
goodput ranged from 5.53 to 6.40 KiB/s across four commands. Engine cleanup and
both daemon stops passed. The timings cover whole verified command operations
with 256 KiB of output each. GitHub qualification uses a fresh candidate and
will repeat the full release gate.
