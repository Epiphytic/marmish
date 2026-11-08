# Native HTTPS fixture independent design review

Actual Claude Code AUTO, model `claude-sonnet-5-5`, session
`51c51d27-98a7-4fbd-bf0e-796be56acdbe`, reviewed the private design and
reproduction/source read-only (113.831 seconds, 15 turns). Part 1, the single
HTTPS test fixture correction, was **APPROVED** with strict canonical-parent
resolution, short prefix, stated socket margin and Linux-only local evidence
conditions. Part 2, a separate macOS diagnostic workflow, was **NOT YET
APPROVED** and is excluded from implementation.

Private full design SHA256:
`4805520fa00fa8327b98ebe3b7972e41fd1dd7287d08c37398ea3fe2faa6d08b`.
Review JSON SHA256: `4e9048560d2376c42840d6a03408e3738099fd35489260be3f8b774a38361839`.

The bounded [implementation plan](../phases/streaming-native-https-fixture.md)
includes only approved Part 1 and its conditions. Approval is recorded before
implementation. Production path guards, all four subcases and all existing
assertions remain unchanged. This one fixture does not resolve the legacy full
suite or establish installed-product behavior. Integrated code review and local
default/long-TMPDIR focused execution remain required.

## Local implementation checks

The implementation changes only the named test's temporary parent/prefix and
comment. In separate Linux Python processes, default ambient and owned long
managed-style TMPDIR runs each passed exactly one test with the same four
subcases and no skips. Process wall times were 0.086816 and 0.080914 seconds.
Both verbose logs SHA256:
`f163f0d635bcce0e6805b6c741a5056e4140d7e4c0f5a30a1624da92ffe8812a`.
The local result record SHA256:
`c71183a921722bf8e18321a8e9bd96796041160cb7a35c02e635f0b4ca1b0f57`.
Scoped Python compilation and diff checks passed; the owned long temporary tree
was removed. No product installer, network or relay ran. Hosted macOS, other
legacy failures and the complete legacy tools suite remain unverified.
Independent integrated code review is still required.

The integrated rsync increment review also inspected this entire fixture diff
and approved it; see [actual review evidence](streaming-rsync-edge-correction.md).
Claude independently reran the exact unit under default TMPDIR, relying on the
retained root/agent log for long-TMPDIR. No macOS result was inferred.
