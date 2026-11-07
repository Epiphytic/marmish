# Phase 3 native copy design review

Scope: [the native copy design](../phases/streaming-copy.md). This is advance
design evidence; it authorizes no implementation and proves no transfer support.
No product source, Cargo job, running daemon, live relay or network test was used.

## Round 1

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Independent bounded reads inspected current stream
CLI/wire, native layout, live execution/launcher, executable authorization and
Linux/macOS spawn and durability contracts. The reviewer did not inspect the
complete command preparation flow or executable-image internals.

Blocking findings and revisions:

- Specify producer/order/encoding for copy frames, Get manifest generation,
  bounded reassembly above transport blocks, durable byte credit and final/error
  handshake. Added the frame grammar and explicit sender/receiver state machine.
- Separate Create from Resume; existing fresh IDs fail, missing checkpoints cannot
  become new transfers, and operands/immutable manifests must match on resume.
- Define the closed wire extension and digest rather than modifying old schemas.
  Added CopyHello/OpenCopy, tagged journal/preparation contracts, canonical digest
  and explicit old-peer timeout without argv fallback.
- Define reusable session ownership of sequences, retry bytes, raw offsets, credit,
  final output acknowledgment and an independent control handle. Preserve the
  current stdio adapter and avoid process-global redirection.
- Specify checkpoint schema, phases, creation/truncation synchronization, checksum,
  portable mode recovery and platform durability limits. macOS file F_FULLFSYNC
  and directory fsync do not promise unconditional hardware-loss durability.
- Specify the lock file and UnknownAttempt race, indefinite disconnected ownership,
  cancellation/commit interaction and local administrative recovery. Authority
  withdrawal preserves running work in accordance with the approved runtime.
- Pre-inspect regular file types before no-follow/nonblocking opens, compare opened
  identity, and state the same-UID adversarial device-substitution limitation.
- Make the private FD3 handoff compatible with syscall-only pre_exec: reserve a
  CLOEXEC source strictly above FD3, capture only its integer, mark all ordinary
  descriptors CLOEXEC first, then dup2. Typed server preparation alone attaches it.

Useful additional revisions include exact macOS leading-alias normalization,
relative path bases, existing no-replace rename primitives without link fallback,
explicit overwrite race semantics, conservative preserved owner read/write modes,
receipt retention lower bounds, admission scan limits and targeted negative tests.
Executable authorization of the native image permits its other argv; this design
makes no per-principal filesystem isolation claim against already-authorized
same-UID executable code.

## Round 2

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Read-only inspection verified spawn hook signatures,
exec-image ordering, platform rename/sync primitives, live preparation ownership,
layout resolution and stream limits. It did not inspect complete journal recovery
or preparation dispatch. It ran no builds, tests or binaries.

Blocking findings and revisions:

- State the exact ResolvedExec request, verify executable policy really ignores
  trailing argv, and choose an honest copy-only authorization position. Actual
  policy `executable_names` and the additive matching loop inspect executable and
  alias only. The design preserves normal authorization and now requires an
  explicit operator warning that allowing the native image grants its other
  administrative/configuration subcommands under the server UID; no copy-only
  security claim is made.
- Long hashing/prefix verification/sync must not look like transport failure.
  Added bounded/coalesced Progress frames, independent I/O/keepalive work and no
  default file-work timeout. Put's extra complete source pass is explicit.
- Avoid cadence/window deadlock. Fixed the durable file window at 2 MiB with a
  1 MiB checkpoint interval, distinct from 256 KiB transport buffering; specified
  reverse-control draining and the invariant for any future configurability.
- Define Get's client-owned receiver lifecycle and the admission lock protocol.
  Added concrete nonblocking lock creation/validation, mkdir-to-lock Busy handling,
  local crash/cancel semantics, and durable server transfer-attempt fencing so a
  dropped client lock cannot launch a competing Get helper.

Useful revisions add FIFO/read-only/overflow validation of the trusted context
pipe, bounded pre-Start ownership, Put resume-ID recovery from server listings,
replacement metadata semantics, two-directory rename test scope, and finite wire
error propagation tests. The proposal to drop receipts after Finish is declined:
retaining them for the replay window preserves lost-ACK/retry evidence. Quota
exhaustion remains an explicit error until eligible expiry; active locks and
uncertain state are not silently reclaimed. A shared private container and space
preallocation remain optional future improvements, not promised by this phase.

## Round 3

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Read-only review inspected live preparation, installed
layout, launcher/pre_exec and platform sync/rename primitives, CLI cancellation
shape and wire encoding. It did not inspect complete journal/runtime recovery or
Rust std's internal descriptor allocator; no tests/builds/mutations were run.

Blocking findings and revisions:

- A fixed dup2-to-FD3 could overwrite std's exec-error pipe. Replaced it with a
  retained dynamically allocated source FD >3, cleared CLOEXEC only on that owned
  FD after blanket marking, and a server-only argv integer locator. No dup2 or
  process-global FD reservation is needed; startup failure propagation tests are
  mandatory. Private consistency context and normal executable authority remain.
- Admission refusal predates helper frames. Added CopyRefused with finite codes
  and prior binding, plus an encrypted attempt-index schema with atomic Open /
  launch / cleanup updates, revision fences, replay retention and conservative
  recovery. Extra Get senders are prevented as an ownership/quota invariant, not
  falsely described as a data corruption defense.
- Added exact numbered byte/enum layouts, metadata encoding, Resume phase and
  final-phase re-entry table; verified receipts use Prefix/Done/Commit/Finish
  without Data or another rename.
- Grounded remote cancellation in actual cancel_selected: a new CLI on the same
  preserved profile loads durable client binding and sends sequence-0 Cancel.
  Busy prints the command and does not silently retry/cancel. Loss of that local
  profile/binding requires server-local administration; IDs are not capabilities.

Additional revisions clarify one canonical tagged launch digest including codec,
upgrade-sensitive executable allow paths, source identity across attempts,
checkpoint offsets beyond file length, symlink-hostile layouts, capability probe
deadlines, helper registration scope and measured-rate qualification deadlines.

## Round 4

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Bounded static reads included exec-image internals,
command preparation and runtime projection in addition to prior spawn/wire/policy
contracts. No builds/tests/mutations were run; complete persistence schemas and
client preparation durability were not inspected.

Blocking findings and revisions:

- Removed stale FD3 parsing text; selected inherited FD >3 is validated as a
  read-only FIFO, bounded-read to EOF and closed, with pipe stdin/stdout required.
  Named it server-supplied consistency context and explicitly rejected an
  authentication claim against same-UID processes or allowed shell executables.
- Split synchronous CopyRefused admission from Opened plus asynchronous engine
  failure. Defined finite terminal Failed mapping and fresh stream IDs for new
  attempts; refused replay never replaces an active index owner.
- Pinned session state, owned Arc context/control lifetime, pending dropped-future
  retries, mismatched InputPending refusal, ACK bounds and final EOF/terminal ACK.
- Strict recorded dev/inode comparison now fails IdentityChanged after remount
  without adopting/truncating partials; uncertain trees require explicit local
  inspection/cleanup rather than misleading HashMismatch or automatic deletion.
- Receiver namespace now hashes actual account pair, transfer ID and direction,
  preventing another wire-authorized principal selecting the same checkpoint by
  copying a public transfer ID. Same-UID executable authority remains unchanged.

Useful additions specify JSON hex/base64 limits, file PermissionDenied distinct
from authorization denial, no-clobber commit conflict retention, exact walk/base
flags, bounded cancellation versus blocking file work, and new-Open age versus
existing journaled stream control. The last distinction follows actual wire
validation rather than treating seven-day Open admission age as forced runtime.

## Round 5

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. The reviewer found the security/spawn architecture
sound and feasible; remaining blockers were protocol specification gaps. Reads
were bounded CLI/preparation/layout/live/launcher/platform/policy/wire inspection;
no journal/runtime/exec-image deep review, builds or tests were performed.

Revisions specify the copy driver's wake/backoff/keepalive and immutable Pending
deadline rules; Put/Get hashing after Start and deliberately unbounded pre-manifest
wait ownership under asynchronous semantics; exact final checkpoint/phase/rename/
Commit/Finish ordering; codecs<=2 versus versions<=4; exact digest versus separate
Binding identifiers; and computed checkpoint maximum 12656 bytes below 16 KiB,
with no duplicated paths/basename. Added early helper dispatch, explicit direct-
parent scan and unqualified network-filesystem limits, and bounded refusal rows
with a conservative unknown-Open age floor rather than evicting replay fences.

## Round 6

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`.
Verdict: **REQUEST_CHANGES**. Bounded static inspection covered existing journal
format/migration, preparation and spawn types, layout recognition and session
contracts. No builds, tests, live operations or repository mutations were run.

Revisions add a unified refusal/error/retry table, distinguish unknown helper
lock owner from a known prior stream binding, and preserve terminal wire errors.
The shared session and synchronous file-worker receiver now have concrete module
ownership and method contracts. The typed live preparation extension owns its
context FD through spawn, borrows only the locator in ChildSpec, validates Darwin
FD limits after allocation and preserves parent/child close ownership on errors.
Journal format 6 migrates atomically to 7, preserving Exec digests and records;
the refusal replay floor is copy-only. Registered installed, Cargo-sibling and
named downloaded images use pinned identity checks across authorization/spawn,
with unmanaged-image provenance limits stated explicitly. Qualification adds
migration rollback, old-binary rejection, image-upgrade races, shared receiver,
FD error-path and error mapping checks. Follow-up review remains required.

## Round 7

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, resumed round-6
session for focused verification. Verdict: **REQUEST_CHANGES**. Four of five
prior blockers were resolved. Direct exec_image inspection found one incorrect
assumption: current ExecImage stores a CString path and calls execve; no opened
image FD is pinned. The design now explicitly retains ordinary Exec's path-based
trust boundary, admits the check/exec replacement race and hostile same-UID
replacement limitation, and only detects trusted-build/schema mismatch in helper
startup before transfer filesystem access. It claims neither portable fd-exec nor
a macOS self-path running-image pin. Also corrected Error Busy to carry no prior
stream ID, required migration tests from every accepted journal version and added
a fresh-stream-ID diagnostic for the copy replay floor. This was bounded static
review only; no tests, builds or mutations were performed by the reviewer.

## Round 8 — approved

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, focused
follow-up in session `70cd724e-1e64-4075-a761-8f51f06352bc`.
Verdict: **APPROVE**. The reviewer confirmed the final blocker resolved and found
no new trust or recovery contradiction. The design now matches actual pathname
execve, explicitly preserves the same-UID executable trust boundary and states
the remaining replacement race and limits of helper compatibility checks.

This last pass read the complete helper-image binding section, Error row and
pin-related claims, using earlier exec_image/launcher findings. It was not a
fresh full reread; earlier rounds cover the other contracts. No migration code
or tests were inspected in this pass, and no builds, tests, binaries, edits or
git/Beads mutations were performed by the reviewer. Nonblocking follow-ups are
image hashing cost, repeating the race caveat in future operator setup guidance,
and verifying implementation migration tests across accepted formats. These
remain implementation/review acceptance considerations, not supported behavior.

This approval completes the advance design review only. The phase remains
blocked on its rsync dependency and implementation authorization. No native file
copy implementation or end-to-end transfer support is claimed here.

## Scoped context clarification — round 1

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, independent
fresh session `2198dfae-8c92-4cf5-adc6-ec522231fea1`.
Verdict: **REQUEST_CHANGES**. Static inspection confirmed the closed context
codec, authenticated-context trust framing, actual compiled metadata sources,
Launch digest exclusions and no-shortened receipt deadlines. Blocking findings
were the unnamed receipt-retention source, Get sender/receiver policy ambiguity,
and unspecified quota recovery before actor-authorized GC. The design now names
`limits.copy_receipt_retention`, separates payload/chat retention, states Get's
context field is validated but ignored by its sender, and requires actor-GC for
phase acceptance while retaining unindexed or uncertain state conservatively.
Useful refinements specify build mismatch diagnostics, intentional cross-group
resume under fresh authorization, wall-clock limits, compatibility identifier
limits and a missing below-floor decode test. Follow-up review is required.

Scope: context grammar/validation/tests, receiver retention paths/tests, matching
design sections, compiled release metadata/build script and existing retention
configuration. No compilation, tests, binaries, live/network operations or product
mutations by the reviewer. Other filesystem paths and daemon/helper integration
were not reviewed; the new modules remain unlinked and uncompiled.

## Scoped context clarification — round 2 approved

Reviewer: Claude Code, `claude-sonnet-5-5`, `--permission-mode auto`, focused
follow-up in session `2198dfae-8c92-4cf5-adc6-ec522231fea1`.
Verdict: **APPROVE**. All three blockers and required small specifications were
resolved; no new contradiction was found. The reviewer inspected the revised
concrete-context section, below-floor decode regression, same-UID cleanup
reference and strict native config handling. No compilation, tests, binaries,
network operations or mutations were performed by the reviewer. Config, helper
diagnostic and actor-GC contracts are prospective, not verified implementations.

Useful nonblocking implementation guidance: keep config format 1 for the additive
defaulted key and state that older strict binaries reject configs explicitly
setting it; verify client config propagation; retain honest context-version-skew
errors and expand malformed-context decode coverage. The config compatibility
statement was added. Full integrated code review and qualification remain required;
unlinked pure modules and written tests have not yet been compiled or run.
