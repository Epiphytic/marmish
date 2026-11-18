# Terminal bootstrap restart review

Claude Code 2.1.301 with `claude-sonnet-5-5` independently reviewed both
rounds in `--permission-mode auto`.

| Round | Session | Design SHA-256 | Verdict |
| --- | --- | --- | --- |
| R1 | `21cf4a26-dc47-4256-bd91-96de6d3860eb` | `7355b81a3e3f114df5ae2172d7a258e654c275c081a85005ce9718b9ba81dca5` | REVISE |
| R2 | `5cbc7a65-7397-4faf-8d9a-6da3e4e6339f` | `b79608afce67c65ebdf3e9fb906ce541fc5db33ccb359e5564823ca0cdd72b63` | APPROVED |

R1 required exact authenticated EOSE, an explicit first-read seam, precise
preservation of the canonical Probe ID and original age, and regression tests
against a duplicate-suppressing relay. R2 checked the existing signed AUTH
readiness state and live versus staged paths. It approved discarding staged
alias events once, before the server's first Probe read. It approved deriving
retry envelopes from the original body and timestamp, with an exact ID check
and a new outer event ID. The relay still cannot turn an old Confirm into a
valid response to a fresh Challenge.

The current [design](../phases/terminal-bootstrap-restart.md) records that the
fixture restarts the server Terminal runtime, clarifies failure on an unverified
lane, and exercises pre-AUTH EOSE in the deterministic readiness test.
