# Terminal redraw feasibility code review

Claude Code reviewed the implementation independently in Auto permission
mode using `claude-opus-5-5`. The reviews compared the integrated test-only
patch to the approved design and evidence at `b91eae5`.

| Round | Exact code commit | Claude Code session | Verdict |
| --- | --- | --- | --- |
| 1 | `bf52bb77f856a6470f6573aeac506229b6fbfd9c` | `0d18271c-fdac-4993-bd61-97a026a32e1e` | Approved |
| 2 | `b38f16cc856e34e23e36401bb4352bc6131b0214` | `655ac2a5-7555-4886-9097-e08204ead227` | Approved |

Round 1 verified actual batch, filter and app-encoder reuse, comparison
timing and fallback, independent tmux grid, cursor and continuation
checks, and the test-only mount. It independently passed the deterministic
test and two private tmux runs. The refinements added an explicit
cursor-only substitution assertion, verified opaque wrap bytes across
partitions, and reported the three initial paints as a separate subtotal.
Round 2 confirmed the subtotal counted each paint once in the mixed
workload. Its independent deterministic and opt-in tmux runs passed,
and the private tmux run reproduced all author traffic counts in 2.20 s.

Both rounds approved the fixed typed checkpoint and virtual clock model.
The opaque-input tests verify the original policy’s bytes and ordering
under the scripted partitions. They do not qualify a classifier for
arbitrary PTY output. The tmux oracle measures visible state and cursor
continuation at eligible checkpoints. Echo, scroll and wrap stay with
the baseline policy, and the measurement covers serialized app output
locally. No blocking findings remain.

In the final author checkout, `git diff --check` and
`cargo clippy --offline --locked -j2 -p marmish-native --all-targets -- -D warnings`
passed. The batch and parser regressions and repeated opt-in oracle
results are recorded in the validation note.

## Integrated review

Claude Code `claude-sonnet-5-5`, Auto mode, approved integrated commit
`a06410f8fd7ad22580ecaa0ce5ca7c26dce5d01f` in session
`322cdc45-d686-4c88-8d63-752bfbe7e63d`. The reviewer verified identical
test blobs, the test-only mount, exact timing and fallback boundaries,
private process cleanup, workload totals and the scope of the four adjacent
native restart and cancellation reports. Formatting and diff checks passed
in the review. The integration author ran the integrated tests while the
reviewer inspected the patch. The report now labels those responsibilities
and identifies the measurements as offline in the agent specification.

The change audit confirmed that existing runtime code only gained a
`#[cfg(test)]` module mount. Ripwire flagged normalized similarities in
fixture-only process cleanup, tmux command construction and simple data
copies. The separate oracle retains its own lifecycle and bounded control
calls. The validation note records the test selections that exercise the
flagged types.

[Approved design](../phases/terminal-redraw-evaluation.md)
[Validation](../validation/terminal-redraw-evaluation.md)
