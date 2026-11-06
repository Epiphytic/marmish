# Phase 2b rsync design review

Scope: `docs/phases/streaming-rsync.md`, a local rsync adapter over the existing
native duplex stream. This is design evidence, not implemented support or
cross-platform end-to-end qualification.

## Round 1

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Independent review inspected current CLI and stream
contracts and performed bounded local Linux rsync 3.2.7 captures. It did not run
Cargo, use network tools, modify runtime source, or test macOS/other rsync versions.

Blocking findings and revision dispositions:

- Real local rsync forwards parent INT/TERM/HUP to its remote-shell child as
  SIGUSR1. Specify an explicit RsyncSignals policy with USR1/INT/TERM/HUP,
  early registration, bounded confirmation and repeated-signal handling.
- Secluded bootstrap can include trailing `--iconv=VALUE`. Permit one nonempty,
  bounded literal option and retain exact captured fixtures.
- Whole-vector root Clap parsing can consume global-looking remote arguments.
  Specify raw prefix extraction and a separate no-global trailing-vector parser,
  including option arity, remote `-l` versus global `-u`, and dispatch seams.

The review additionally proposed cancellation on stdout close. That suggestion is
**declined**: the user's explicit asynchronous execution contract keeps commands
running on disconnect. Local EPIPE/parent SIGKILL are detach/error, not explicit
remote cancel. The revised design requires recovery via streams/cancel and tests
that preserve this distinction.

## Independent local capture during revision

Observed Linux rsync: `rsync version 3.2.7 protocol version 31`.
A disposable capture executable recorded arguments then exited without a peer;
each rsync invocation returned expected exit 12. No remote process, relay or
network connection was used. Capture directory was removed afterward.

| Local options | Captured suffix after `-l npub1test relay.example` |
| --- | --- |
| `-as` | `rsync --server -slogDtpre.iLsfxCIvu` |
| `-as --iconv=utf8` | `rsync --server -slogDtpre.iLsfxCIvu --iconv=utf8` |
| `-as --iconv=UTF-8,ISO-8859-1` | `rsync --server -slogDtpre.iLsfxCIvu --iconv=ISO-8859-1` |

These are bootstrap observations only. They prove neither remote transfer nor
platform interoperability. The exact source/destination temporary paths and
private diagnostics are intentionally excluded.

## Round 2

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Root-global extraction and iconv findings were
resolved. Two further signal races blocked approval:

- Real process-group Ctrl-C delivered INT followed by rsync's forwarded USR1
  about 400 ms later. The naive second-signal escape would abandon cancellation.
  Revision: two-second coalescing grace; USR1 always coalesces during confirmation;
  only later INT/TERM/HUP can leave without confirmation. Deadline never resets.
- Explicit USR1 can race the EPIPE caused by rsync exiting immediately afterward.
  Revision: latched signals, biased polling and pending-notification recheck before
  I/O-error exit ensure the explicit cancel wins. EPIPE alone still detaches.

The revision also moves registration before command_socket/autostart/SRV awaits,
passes owned listeners into execute, specifies no-Open versus attempted-Open
cancellation, the existing min(idle,60s) confirmation budget, direct adapter exit
codes, global-option acceptance, parser-command assertion and bare separator rule.

Claude inspected the actual source seams and repeated Linux rsync 3.2.7 captures
including push/pull iconv, protocol 29, old-args and signal probes. It observed
`--iconv=.` emitted literally, `--iconv=-` omitted, and `RSYNC_OLD_ARGS=1` plus `-s`
unchanged. It ran no Cargo/network/macOS tests, edited no files, and removed its
owned probes and directories. These observations do not prove transfer support.

## Round 3

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **APPROVE**, with one required documentation correction: the phase header
incorrectly named round 2 as the approval round. Corrected to round 3 before
recording approval. Both round-2 signal blockers and all round-1 findings are
resolved; source implementation and actual parser/platform behavior remain untested.

Claude inspected the current CLI contracts without modifying source and used only
bounded local non-network rsync 3.2.7 probes. Four probes observed USR1 reaching the
adapter before EPIPE; a SIGKILL-parent probe observed no cancellation signal. It
also observed USR1 on local timeout/protocol errors, bracket removal for IPv6,
and additional option spellings. These useful observations are incorporated as
implementation/qualification requirements, not claims of portable guarantees.
No Cargo, relay/network, macOS, other-version rsync, or implementation tests ran.
Reviewer cleaned owned probe directories/processes.

Approval applies to the revised design in `docs/phases/streaming-rsync.md`, including
early owned listeners, coalescing, explicit-signal priority over I/O failure,
disconnect-preserving behavior, literal iconv grammar and two-stage raw parser.
No rsh implementation has started under this design record. Actual code and
qualification still require the normal independent review before phase closure.
