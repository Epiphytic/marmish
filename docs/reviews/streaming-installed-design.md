# Installed streaming and transfer qualification design review

Scope: [Phase4 design](../phases/streaming-installed.md), Beads `marmish-8ka.5`.
Design evidence only: no installed platform pass, implementation or release approval.

## Round1

Reviewer: Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session `ac814264-10a3-4f79-a350-491956f31a46`;96.369s,17turns.
Verdict: **REQUEST_CHANGES**. Read-only inspection covered the proposed design,
v02 installed/release workflows, Node supervisor/provenance/transport contracts,
Rust cleanup/record schemas, installer, Cargo dependencies and prior stream/rsync/
copy designs. Reviewer performed no tests, builds, binaries or network operations.
No platform results were produced.

Blocking findings and draft revision dispositions:

- Existing supervisor2220s/+900s policy cannot accept3300s under62-minute watchdog.
  Revision pins a separate3300s cohort policy: expiry+420<=watchdog, cleanup expiry+300,
  preserving old matrix arithmetic and testing both.
- Two-stream ceiling contradicted default peer4 saturation. Revision configures
  peer2 for the bounded saturation test and explicitly owns/cancels both jobs.
- Small copies lacked observable deterministic interruption. Revision uses1MiB
  same-platform files, observes an intermediate durable receiver checkpoint locally,
  signals the still-owned initiating process, requires explicit helper cancel/finish,
  then resumes through a new stream. No checkpoint means honest case failure.
- Stream physical cleanup proof was unnamed. Revision specifies additive same-UID
  CommandCleanupStatus stream counts/barriers, a separate strict stream-aware decoder,
  and serializing/requiring the existing internal final stream_stop_complete flag.
  These are explicit future implementation/review requirements, not existing support.

Useful nonblocking findings incorporated: transitive local Cargo dependency fingerprint
coverage including core/vault; exact two-path candidate workflow allow-list; Linux
rsync verification without sudo, macOS Homebrew absolute path, minimum3.2.7 and shared
preparation-budget acquisition; explicit1200s edge family budget table.

## Round2

Reviewer: Claude Code `claude-sonnet-5-5`, `--permission-mode auto`.
Session `d7426346-c4ab-4dca-b7bd-ef7d7a248932`;213.274s,26turns.
Verdict: **REQUEST_CHANGES**. Read-only inspection covered the revised design,
workflows, supervision/provenance/installer tooling, cleanup schemas, copy
checkpoint constants/helper and workspace dependencies. No tests, builds, binaries,
network operations or edits were performed. No platform results were produced.

Blocking findings and draft revision dispositions:

- The1MiB file equals the byte checkpoint interval, so an intermediate checkpoint
  was timer-dependent. Revision uses4MiB and the actual checksum decoder on a
  read-only receiver checkpoint. Put/get interruption become separate1200s waves;
  slow transport still fails honestly rather than assuming a throughput result.
- SIGINT requests Cancel rather than detach. Revision pins owned-client SIGKILL,
  charged/live helper plus lock/Busy proof, explicit cancel and physical finish
  before fresh-stream resume. INT remains a separate cancellation case.
- Policy is immutable until daemon restart. Revision sets peer2 for the whole
  edge wave during setup and removes in-place restoration/restart assumptions.
- An1800s traffic budget exceeded server uptime once barrier/cleanup were counted.
  Revision bounds earliest-start to ready180s, traffic/reconciliation1200s, owned
  stop180s:1560s total plus240s uptime margin. Context1980s remaining is explicitly
  derived from1560s work plus420s slack, with timing regressions measured from
  server start.

Useful nonblocking findings incorporated: pre-transfer absent/empty inventory and
stream/launch-digest receipt binding; optional bounded additive status field for
old finite consumers with required stream-aware validation; rust-toolchain input
fingerprint; role-relative public exchange labels; prerequisite-only Linux rsync
verification and Homebrew acquisition provenance; plain installer versus separate
setup distinction; honest owned-child versus process-group authority; sanitized
bounded inherited driver diagnostics; nominal watchdog/provisioning margin.

## Round3

Reviewer: Claude Code `claude-sonnet-5-5`, auto; session
`6691a3da-d504-40f7-8082-37e33c6ac0b1`;112.166s,17turns.
Verdict: **REQUEST_CHANGES**, read-only source inspection and arithmetic;
no tests/builds/binaries/network and no platform results.

- Get receiver flock dies with the killed client, so Busy proof cannot work.
  Revision separates directions: put retains Busy; get proves OLD_ID live/charged,
  does not concurrently resume, then explicitly cancels before fresh resume.
- Wrong-owner installed case lacked a second principal. Revision explicitly places
  wrong-owner in hermetic multi-principal coverage, preserving honest distinctions.

Useful small clarifications: bounded checkpoint-directory discovery/atomic decode,
owned stderr ID capture before sanitization, completion-before-observation failure,
inventory/receipt inclusion in cell budget, new65-minute rendezvous job, server
start<=Context+720s to reach ready before900s. Follow-up approval remains pending.

## Round4 scoped follow-up

Reviewer: Claude Code `claude-sonnet-5-5`, auto; session
`dc823239-a7a1-46f6-a14e-6c046c71b270`;20.014s,6turns.
Verdict: **APPROVE** for both remaining round3 blockers, with no concrete
blockers remaining in that scope. Reviewer checked the revised direction-specific
interruption/wrong-owner requirements against copy_files, copy_protocol and
command_store. Read-only inspection only; no tests/builds/binaries/network or
platform results. Earlier rounds cover the remaining design and resolved findings;
this approval is design-only, not completed implementation or installed acceptance.

Useful suggestions incorporated: explicit wrong-owner in the hermetic list and
get sender backpressure/list evidence distinguished from receiver-lock evidence.
Phase dependencies still gate implementation and actual hosted qualification.

## Public-CLI coverage boundary adjustment, round 1

Claude Code Sonnet5.5, auto, session
`7ea295b0-07a0-4809-8e82-ac194194074b`, 34.367 seconds and 7 turns:
**REQUEST_CHANGES**, two evidence-mapping clarifications. The reviewer verified
that setup acquires the daemon-held exclusive profile lease before grant/trust
updates, found no public live-reload/replayed-Open API, and agreed with the
hermetic-only classification while preserving all installed acceptance.

The initial paragraph overgrouped three existing tests: none sends a second
authenticated Open, and unavailable adapter policy is not live runner revocation.
The revision maps every cited test precisely, names the runner-only irreversible
input-suspension test, and records the authenticated repeated-Open and end-to-end
live-actor withdrawal coverage gaps explicitly. No build, tests, network or
platform qualification was performed by this review. This adjustment does not
close those gaps or approve Phase 4 completion.

### Public-CLI boundary scoped follow-up

Claude Code Sonnet5.5, auto, session
`ad29ea33-76d4-43bb-8e3a-42a41533e186`, 22.345 seconds and 6 turns:
**APPROVE**, narrow classification design only. The reviewer inspected every
cited test, confirmed the corrected claim mappings and retained explicit gaps,
and verified that actual installed/positive/cancellation/resume acceptance was
unchanged. Its useful nonblocking wording correction was incorporated: the
opening installed-case list no longer simultaneously labels the two hermetic-only
cases as required installed cells. No tests, builds or platform runs occurred.

## Bounded interruption payload and metrics refinement

Claude Code Sonnet5.5, auto, session
`2e12587b-8822-4078-be74-a2489f03dc8f`, 45.631 seconds and 9 turns:
**APPROVE**, scoped refinement only. The reviewer verified the exact constants:
1MiB CHECKPOINT_INTERVAL, 8192-byte MAX_BLOCK and 2MiB durable window. The payload
is now 1.25MiB (`CHECKPOINT_INTERVAL + 32 * MAX_BLOCK`), preserving an actual
intermediate byte checkpoint while reducing qualification cost after the parent
measured 2MiB in 384.67s on a relay. That observation is not a platform guarantee.
Execution uses 1140s and reserves 60s for reconciliation inside the original
1200s bound. The stable post-cancel pre-resume checkpoint supplies remaining
logical bytes; the initial live sample is not used to infer resume goodput.

The useful wording condition was incorporated: the canonical production
helper/client receiver loop must actually checkpoint at the byte interval.
This isolated baseline has only imported pure APIs, so a constant or helper test
does not prove the production loop. Missing intermediate state still fails.
GET is explicitly live/charged evidence without a backpressure claim; ordering
is driver-enforced rather than independently reconstructed by rollup. No builds,
tests or live runs were performed during this review. Phase completion and
platform qualification are not approved by this scoped verdict.

## Installed pre-Open process-control observation

Claude Code Sonnet5.5, auto, round1 session
`a1e696c0-dd01-4eae-bf15-49c05223e31e`, 124.685 seconds and22 turns:
**REQUEST_CHANGES**. The design now pins canonical additive journal fields,
requires a locally queued Hello before INT, retains the owned server Child mutex
through synchronous SIGCONT cleanup, forbids IPC reentrant locking while paused,
and requires present Hello-only server state before administrative cleanup.

Round2 session `a0f835ca-a37f-4dec-b82c-55010c414d25`,105.868 seconds:
**APPROVE conditional on one wording correction**, incorporated above. Status
can allocate a Hello-shaped record, so attribution relies on the isolated fresh-ID
production CLI sequence, not state shape alone. Qualified additive fields remain
mandatory. Actual before-Open exit255 and exact diagnostic were verified in
source; no platform, process-control or relay test was run by this review.
Pause timeout remains fail-closed; client abort/reap precedes server resumption
on failure, and final physical cleanup must still pass.

## Independent edge-wave budgets and complete membership

Claude Code Sonnet5.5 in auto reviewed the measured-latency amendment. Rounds1–4
requested precise membership, honest initiating/control call counts, independent
manual dispatches, retained positive awkward-argument coverage, receiver-side
nonregular destinations, and correction of the measured throughput units. All
were incorporated in the phase design. Actual review records:

| Round | Session | Seconds | Turns | Verdict |
| --- | --- | ---: | ---: | --- |
| 1 | `3b371f3c-492f-4d79-8325-69469d910391` | 57.553 | 8 | REQUEST_CHANGES |
| 2 | `6dd03902-9ee7-47fa-8a0b-74cbee33a620` | 70.946 | 9 | REQUEST_CHANGES |
| 3 | `976d1186-3ac2-45a5-95d4-2c9484544eb6` | 49.719 | 5 | REQUEST_CHANGES |
| 4 | `94f0af9f-efe7-4456-ac80-6bad1ce33192` | 22.531 | 7 | REQUEST_CHANGES |
| 5 | `6835939c-31af-42e2-a172-76d30432f4e4` | 7.771 | 3 | APPROVE |

The final verdict approves the scoped design, not implementation or platform
results. Each of eight sequential manual dispatches has its own original
1140-second execution budget and 60-second reconciliation reserve; deadlines
are never extended by successful cases. The edge-copy wave includes22 tiny
calls per platform (20 case observations plus two dedicated committed receipt
seeds). Installed observations do not replace the separately required hermetic
replay and exact signal/EPIPE-branch evidence. No workflow was dispatched during
this review.
