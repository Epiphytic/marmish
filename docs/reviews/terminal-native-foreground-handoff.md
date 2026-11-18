# Native foreground handoff review

Claude Code independently reviewed the design and test implementation with
Auto permissions.

| Review | Exact source | Model | Session | Verdict |
| --- | --- | --- | --- | --- |
| Design R1 | `ad30bb9` | Opus 5.5 | `2d86ddf9-b0cf-4e04-81cf-f4606e57f1fe` | Request changes |
| Design R2 | `ee43dd3` | Sonnet 5.5 | `42827cbc-6a42-4c45-8c99-43370548aff8` | Approve |
| Integrated test code | `7dec6df` | Sonnet 5.5 | `332ef2fe-8169-47ca-a7cb-c1fd8cb43a0a` | Approve |

Design R2 required a bounded observer input barrier, exact old-client exit
status and stderr, the whole cooked menu before sending the choice, one
shared reap deadline, and distinct geometry witnesses for the writer and
observer. The author clarified single reader ownership, exact wall-time
measurement and the existing relay limit in `2a95ff8` before implementing
the test.

The final reviewer inspected the complete diff and approved the real
foreground PTY, relay and tmux checks, close handling and cleanup. The
reviewer ran no live tests. Scope findings are reflected in the
[author's execution evidence](../validation/terminal-native-foreground-handoff.md).

- The test locates sibling executables. The build-stamp and byte checks
  were external steps performed by the author.
- The during Command starts within the handoff interval measured to
  observed raw Ready. Both measured turns finished before Ready.
  The test's two prepared Command owners share one fixture actor, so
  this supplies no installed daemon scheduling or load guarantee.
- A whole Command turn on that actor can stale the server's two-second
  admission snapshot. The fixture reschedules its Command turn after the
  client binding request is serviced to avoid starving its separate
  one-second binding lookup. The ready check remains fail-closed if
  the lookup or admission fails. Of the successful runs, none required
  widening the production deadline.
- The observer queue readings establish that no line-discipline input
  was pending at the sampled barrier. Full echo, distinct evaluated
  marker and final-capture checks establish that the test never saw
  the observer candidate reach the pane. The backend observer test
  separately covers rejected native writes.

Ripwire flagged a pair of cloned relay count functions. The integrated
version shares the exact predicate after selecting a kind or total.
Remaining flags include the ignored test entry point, separate bounded
stderr and PTY drains, and churn in the test harness. The product source
and regular CI selection are unchanged.
