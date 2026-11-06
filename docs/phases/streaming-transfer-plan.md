# Streaming commands and file transfer

Beads: `marmish-8ka`. This is a source implementation initiative; it does not
publish a new binary release. Existing finite commands and their recovery remain
supported. The user authorizes implementation, independent Claude reviews and
publication of each completed increment to both source mirrors.

## Goal and boundaries

Provide a binary-clean duplex remote shell for rsync, optional streaming
compression, and resumable regular-file upload/download with checksums and safe
atomic installation. Reuse native identities, configured/default relays, accepted
Marmish command rooms, same-UID IPC and additive executable restrictions. No PTY,
port forwarding, filesystem sandbox, automatic package installation, or generic
shell evaluation is introduced.

The existing command engine has sequential stdin but retains only a fixed output
prefix. Its loss-reporting behavior is appropriate for finite asynchronous jobs,
not a reliable rsync byte stream. Add an explicit stream protocol and bounded
backpressured execution path, leaving command/1 semantics unchanged. Network
loss does not cancel a streaming command; full buffers block its pipes. A stream
attachment cannot transparently resume after uncertain local pipe delivery or a
daemon/worker crash. Such loss fails explicitly and never automatically executes
the command again. Rsync can start a new invocation with its own partial-file
options; native copy has durable file-level resumption.

## Phases and gates

1. **Bounded stream primitives.** Independently reviewed closed wire schema,
   byte offsets, duplex windows, duplicate/ACK checks, EOF and terminal offsets,
   independent bounded compression frames, and deterministic adversarial tests.
   No live execution is enabled in this phase.
2. **Native duplex runtime (2a), then rsync CLI (2b).** Detailed design/review before coding.
   Authenticated stream messages in dedicated command rooms; durable launch fence
   and publication ownership; same-UID local IPC; bounded process pipes; explicit
   `marmish rsh` CLI, idle timeout and binary-clean streams. Reuse server PATH
   resolution and every executable restriction. Test real rsync push/pull and
   duplex data exceeding former caps through actual Marmot transport.
3. **Resumable regular-file copy.** Detailed design/review before coding.
   `marmish put REMOTE LOCAL REMOTE_PATH` and `get REMOTE REMOTE_PATH LOCAL`
   use a normally authorized native helper over the same stream. Verified prefix
   checkpoints bind both identities, direction, path and source size/hash.
   No-follow descriptor-relative receiving, private sibling temporary files,
   explicit overwrite, checksum before atomic commit, safe mode/mtime metadata.
4. **Installed interoperability and evidence.** Detailed design/review before
   changing CI. Exercise actual installer and installed binaries on Linux AMD64,
   Linux ARM64 and macOS ARM64. Test directional platform pairs, rsync and native
   copy, negative authorization, special filenames, compression, resumptions and
   slow consumers. Publish measured latency, raw goodput and compression ratios
   with source/platform and workload. Report runner/dependency failures honestly.

Every phase has independent Claude design and integrated code review in auto
mode, appropriate tests, documentation updates, audited staged diff, commit and
push to GitHub and Radicle, verified refs and Iris availability. Useful review
findings are incorporated before closing its Beads task. Phase details and test
results distinguish implementation from intended behavior.

## Cancellation

Foreground streaming commands and transfers handle Ctrl-C by sending an explicit
authenticated cancellation, independently of a blocked data exchange, and report
confirmed versus uncertain cancellation on stderr. A second interrupt may exit
locally without claiming the remote stopped. Provide stream listing and a stable
stream ID for cancellation from another terminal; existing finite-command cancel
remains supported. Cancellation is idempotent, owner-bound and signals the owned
process group with a bounded graceful period and escalation. Never signal a
recycled PID or equate client disconnection with cancellation. Tests cover blocked
stdin/output, children, duplicate cancellation and cancellation after exit.

## Provisional interoperability choices

The stream/1 schema remains provisional until phase 2a integration design is
approved. Use a separate `marmish-stream/1` application family, never ordinary chat messages
or an implicit extension of finite Submit. Negotiate protocol/compression before
launch, bind all exchanges to authenticated MLS room, identities and invocation.
Use independent zlib frames (`none` fallback) with offsets and checksums over raw
bytes; zlib already exists in the dependency graph and has a bounded decoder
window. Compression is optional and only used when beneficial. No cross-message
compressor state is needed for retry or file resumption.

Rsync examples use `rsync -a -s -e 'marmish rsh' ... NPUB:path`. Require rsync's
secluded-args support on both endpoints; stock older macOS rsync may need a modern
rsync installation. Accept rsync's `-l NPUB HOST` convention without treating it as
a Unix account. Never join remote argv into a shell command. `--rsync-path` selects
one executable, not a shell script. Allowing rsync or the transfer helper grants
its ordinary same-UID filesystem access. In particular rsync configuration/hooks can execute
arbitrary commands: authorizing the rsync executable effectively authorizes
arbitrary command execution as that UID. Executable restrictions are not an argv
or filesystem sandbox. Native transfers must not bypass executable authorization.

Compression defaults to none. Opt-in compression before encryption leaks data
size/compressibility; do not mix attacker-controlled data and secrets under an
assumption that ciphertext size is private.
