# Checked native and Journal startup review

Claude Code Sonnet Auto approved the [startup design](../phases/peer-candidate-native-owner-startup.md)
at `1a1a86c` in session `453d07ab-134e-4682-92bd-c1b970ea8aad`.
The first two code rounds, `3fdea004` and
`df17ae83-50f7-4179-8b1e-1602f04dacaf`, asked for direct evidence of
partial ownership, the Journal smoke close and reopen, and the conditional
lifecycle write. The final source `c033631` includes those probes and
preserves the paired Journal in the fresh-account case.

Claude Code Auto approved exact source `c033631` in session
`d65e9d3f-0fa6-451e-a738-74f4a4c9be07` after verifying the embedded
commit on both archived test executables and independently rerunning the
full registered qualification. Stock passed in 188.39 seconds and candidate
passed in 190.06 seconds. Session `1e7c1640-3d36-41ae-8a09-93fbb3178cc6`
independently approved the same code by static review. The design wording
now distinguishes the constructor's admission milestone from the Ready
profile binding check the later daemon caller must supply.

The focused suite cuts immediately before the fresh lifecycle write.
It does not exercise a successful fresh candidate write. Several panic
boundaries share the covered custody and Drop paths. The integrated
service still needs to exercise startup error disposal with started
workers, connector and listener handles. See the
[validation record](../validation/peer-candidate-native-owner-startup.md).

Claude Code Auto approved the composed `038562c` in session
`179e27c4-e243-41ae-915f-25ed0f85ec6a`. It independently reran both
native startup archives (candidate 135.5 seconds and stock 135.2 seconds)
and 148 Journal tests on each. A separate composition review in
`54ac577a-f5a9-43a5-aa70-953ba07f7c21` verified the binary commit stamps,
the byte-identical reviewed constructor and custody sources, and the
unchanged listener and Command startup code. It did not run tests.
