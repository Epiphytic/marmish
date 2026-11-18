# Interactive core selector guard

Claude Code in Auto mode approved the exact code at
`43684f6c6012841ee119ab3334e9a7af1952cc17`, in session
`16e74dcf-7235-4c76-ae50-d7a06a087a68`. All 35 selectors preserve their
arguments. Each selects a single standard libtest binary. Triggers, the
self-hosted runner, checkout, toolchain, timeout and test selection stay
the same.

The reviewer compiled an isolated libtest binary and checked the real
terse summary. Synthetic Cargo runs confirmed that an empty filter and
an ignored-only filter fail, matching tests pass, compile and test errors
keep the original exit code, and helper arguments reach Cargo unchanged.
The guard also prints the captured test output on failure.

A full Linux AMD64 local replay ran all 35 workflow selectors with the
locked dependency graph. They produced 156 passing test executions,
including overlap across selectors. One real unmatched Cargo selector
reported zero tests and the wrapper returned 1. Syntax, YAML structure
and `git diff --check` passed.
