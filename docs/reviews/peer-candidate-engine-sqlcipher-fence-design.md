# Candidate Command SQLCipher fence design review

Claude Code ran each independent review in Auto mode against the isolated
source beginning at `33c7338`. Review inspected checked-in Rust and the
locked SQLite/SQLCipher sources. It made no edits or builds and used no relay.

| Exact design | Claude session | Disposition |
| --- | --- | --- |
| `c9bec7f` | `5dde9a33-ea9b-4aca-87b8-0bc7fec8d705` | Request changes |
| `0b728fa` | `11325914-d2c1-41bc-8c00-13a9298f844b` | Request changes |
| `a1088e6` | `5db31060-2468-42ee-a170-5c7c5f0bad7f` | Approve |

R1 established that rusqlite can close partial handles before its constructor
returns. It required explicit close before returning spool output, sticky
refusal across error unwinding, handling of SQLite close failure, concrete
runtime mutex tests, and native/prototype error mappings. R2 checked these
mechanics and required the same park and poison rule after raw close failure,
a single latch pairing for the unsafe-close notification, and global
propagation of refusal during reconciliation. R3 verified the revised
ownership, notification pairing and error propagation against the source.
Cargo and native runtime tests remain implementation evidence.

The approved candidate raw path shares post-open hardening with stock.
Implementation updates old fixture constructors to the paired Trip API and
checks the notifier from an unwind destructor. The eventual registry mount
supplies its sealed same-lifetime observation and custody notification.
