# MDK master, native schema preflight and Linux regressions

This checkpoint resolves upstream MDK master to
`122ad45ece73c1d6f687a4e92a541b9b2febde34`, the branch head observed for the
build. The manifest follows master and all ten MDK lock entries move together.
The remaining lock entries are unchanged. [Upstream change](https://github.com/marmot-protocol/mdk/commit/122ad45ece73c1d6f687a4e92a541b9b2febde34)

The preceding MDK checkpoint introduced account migration 213. The native
startup preflight still treated migration 212 as the newest supported schema,
so it refused a closed account created by MDK itself. The fix raises the
known migration boundary to 213 and keeps the newer-schema refusal. In the
new test case a real SQLCipher account uses its normal WAL lifecycle. A
future migration inserted into the account registry is refused with its
previous file inventory and bytes preserved.

The updated lock, the native startup fix and the integrated Peer proof
sources were built at `cede6ec6b4bda36aca5e88efb28db6544af6a9a1` on Linux AMD64.
The five focused preflight tests also passed in 14.84 seconds in a separate
checkout with identical lock and preflight blobs. Strict native all-target
Clippy passed in 42.91 seconds, and locked offline Cargo metadata and
`git diff --check` passed.

| Test | Result |
| --- | --- |
| Previously failed native cases | 32 passed, 0 failed in 66.35 s |
| Private WSS history and durable native PostJoin | 4 passed, 0 failed in 212.27 s |
| SQLCipher quota fixtures | 7 passed, 0 failed in 51.55 s |

The first broad test run lost its executable path while a rebuild replaced it.
The stream subprocess error was independently reproduced by deleting a
disposable running test's link. The corrected rerun used a uniquely named
copy of the exact test executable and stable sibling fixture binaries.

On the local WSS relay, complete history converged in 2.062 s. Omitted history
retained the prior epoch in 168 ms, while a withheld end marker expired in
5.117 s. The durable PostJoin test recorded three physical Group writes,
including a byte-identical retry. It measured 316 ms for cold reopen,
5.065 s with a deliberately withheld acknowledgement and 2.590 s for the
retry. These are individual controlled loopback samples. No Peer traffic
was sent to an external relay.

The separate durable repeat passed in 67.09 s and again produced exactly
three physical Group writes. The ordinary Command regression passed in
65.97 s during a Peer relay outage, with 8.449 s for command execution.
The native SelfRemove case passed in 53.20 s, including a 4.000 s
cumulative removal interval and 238 ms for independent cold readback.
Both ran with disposable local test state.

Claude Code Sonnet 5.5 in Auto mode approved the preflight and lock scope
in session `51981b34-53da-4b66-9d84-be91ed187663`. The focused source
`f5ec3844e6ce61d91a1581995263f8fd603fc7b3` has the same preflight and lock
blobs as the integrated build. The reviewer inspected the exact changes,
the upstream account migration and the passing validation logs.
