# Native direct Marmot command validation — 2026-11-02

Working tree: v0.2/rust-native, based on 5a37bdc886aaa1d3e48f776ca9a25749882e1ebb; integrated Phase 2 changes were uncommitted during this run.

Fresh client/server Nostr identities, independent encrypted MDK and command stores, dedicated Marmish command room, explicitly selected `wss://relay.us.whitenoise.chat`, direct transport (`--tor off`). Invoked the actual Rust `marmish command-check` and sibling Rust worker built with `cargo build --locked --offline -p marmish-native --bins`. The fixture uses the production account owner, authenticated native message ingestion, durable command service/engine, response digest and cursor consumption. No Python runtime, namespace startup or mock transport.

| Check | Result | Completion latency | Delivered stdout goodput |
| --- | --- | --- | --- |
| Literal argv with quotes, dollar/backtick syntax, backslash, newline and non-UTF8 byte | Exact 53 bytes; exit 0 | 1736 ms | 30 B/s |
| Binary stdin and EOF through cat | Exact 16384 bytes; exit 0 | 2221 ms | 7374 B/s |
| Independent stderr and nonzero exit | Exact 12 stderr bytes; exit 7 | 1676 ms | Not applicable |

Total fixture time, including setup and cleanup: 13816 ms. These are single local Linux observations over an external relay, not RTT measurements or sustained transport benchmarks. Goodput includes command admission, execution, polling and response delivery. [Machine-readable sanitized result](local-command-direct-2026-11-02.json) includes content hashes and all cleanup flags. Actual engine quiescence, engine joins, MDK stops, runtime cleanup and private state cleanup all passed.

Earlier attempts exposed a driver directory-permission bug (fixed by creating its root with mode0700) and one finite validation failure before diagnostic details were added. The latter attempt correctly reported retained state and is not counted as a passing cleanup. Its cause was not established; this successful retry does not erase that limitation.

Not covered: daemon restart while a command is running, remote conflict/replay, public command CLI/IPC, Arti command execution, installed artifacts or cross-platform command execution. The earlier hosted Phase1 foundation evidence does not substitute for these command checks. Phase2 remains open.
