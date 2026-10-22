# Phase 2b command prototype validation

This increment connects the same-user command engine to Marmot through the
hash-pinned MDK daemon. The [approved phase plan](../phases/phase-2b-prototype.md)
and [Claude plan review](../reviews/phase-2b-plan-final.json) describe the intended
boundary. This report separates implemented behavior, automated checks and the
remaining limits. The [live end-to-end report](phase-2b-e2e.json) passed all
12 assertions in 168.471 seconds on 2026-10-22 using explicitly selected
`wss://relay.eu.whitenoise.chat/` and `wss://relay.us.whitenoise.chat/`. Fresh
disposable server, client and unapproved-client identities were used; the
report publishes only the server/client public identities and session group IDs.

The full offline run passed 82 Python tests and 96 Rust tests. Workspace clippy
with warnings denied, formatting and whitespace checks passed. Namespace checks
ran on this Linux host; they were not skipped. Claude Sonnet 5.5 reviewed the
plan and code in auto mode. The [initial code review](../reviews/phase-2b-code.json)
required changes; the [second review](../reviews/phase-2b-code-final.json) approved
the corrected code. The [final focused review](../reviews/phase-2b-code-followup.json) also approved
subsequent recovery fixes and the harness/documentation. This is prototype evidence, not production or
multi-device certification.

Earlier attempts exposed harness issues: the child PATH omitted `/usr/sbin`,
an eight-second sleep was too short to establish a disconnected running job,
the cumulative 180-second client deadline was too short for the full suite, and
the unapproved client's daemon closed before confirmed invitation delivery.
Those were corrected with an explicit trusted PATH, bounded sentinel-controlled
jobs, a 900-second total budget and keeping the daemon alive until observation.
The successful run includes the actual CLI subprocess check; no result is
inferred from a mock or from relay publication alone.

## Reproduce the checks

Use the pinned Rust 1.97.1 toolchain, a C compiler, make and Perl for the vendored
SQLCipher/OpenSSL build, and Python 3.11 or newer. The worker and engine bridge
must be built without the engine fault-injection feature for normal use.

```sh
cargo test --offline --locked --workspace
cargo clippy --offline --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --offline --locked --workspace
python3 -m unittest discover -s tools -p 'test_*.py'
```

The Python suite uses fake local RPC peers and controlled namespace test
endpoints. It does not contact public relays. Namespace tests require the Linux
tools and kernel permissions described in the README; unsupported hosts cannot
establish the confinement evidence merely by passing unrelated unit tests.

The live harness requires explicit opt-in, a selected relay set, the pinned
daemon and built worker/bridge. Replace the example origin with the intended
test relay; repeat `--relay` when using multiple origins.

```sh
python3 tools/marmish_e2e.py --live \
  --relay wss://your-relay.example/ \
  --wnd /absolute/path/to/wnd \
  --wnd-sha256 1324c70d389fb49f37ae8f7f6ab2a16840b138a0c438d7aef10bb6f9eee20a03 \
  --bridge "$PWD/target/debug/marmish-engine-bridge" \
  --worker "$PWD/target/debug/marmish-worker" --timeout 900
```

It creates disposable identities under a private temporary root and tests real
Marmot traffic. The maximum overall duration is 900 seconds. Its report contains
public identity/group identifiers, assertion names, elapsed time and fixed
failure categories. The harness stops frontends, releases its bounded test jobs
and removes disposable local state on exit. Published encrypted relay events are
not remotely erased by that cleanup.

The live assertions cover literal arguments including spaces, Unicode, quotes,
newlines, dollar signs, backticks, leading dashes and non-UTF-8 bytes; separate
binary stdin/stdout/stderr; nonzero exit status; cancellation; exact duplicate
submission without duplicate execution; conflicting invocation rejection;
stale challenges, wrong groups and ordinary chat payloads; unallowlisted and
ordinary-chat invitations; detached-worker completion while both frontends are
stopped; restored sessions/output after restart; independent new MLS groups;
previous-boot valid control-frame rejection after restart; configurable retention; and actual client/server CLI processes.

## What the adapter verifies

The installed daemon's observed SHA-256 is
`1324c70d389fb49f37ae8f7f6ab2a16840b138a0c438d7aef10bb6f9eee20a03`.
The adapter's request and result contracts were inspected in committed MDK
source `37c248a` and compared with synthetic requests captured from the installed
`wn` CLI. A matching hash identifies the observed artifact; exact binary-to-source
build provenance remains unverified. The private daemon RPC is version-specific.

RPC payloads use a Unix socket, never subprocess argv or environment variables.
Each request has one newline frame, a write half-close, a single absolute socket
deadline, Linux peer-UID checking, a 256 KiB request cap and a 4 MiB response cap.
Both JSON layers reject duplicate object keys and nonfinite numbers. Diagnostics
omit raw daemon stdout/stderr and application payloads. Uncertain sends retain
their invocation identity rather than becoming new commands automatically.

Application frames are at most 32 KiB and stream chunks are 4096 bytes before
base64 encoding. Authentication uses the actual MDK row's group, sender,
direction, kind 30100 and Marmish version tag. Payload-claimed identities and
display names do not establish authority. Dedicated rooms require the expected
profile purpose, an allowlisted inviter and exactly the two expected members.
Every new session creates an independent MDK MLS group. This uses MDK's existing
MLS behavior, not a custom encryption scheme or a certification of the future
normative multi-device protocol.

## Confinement and storage boundaries

The host process launches an outer user/network namespace, brings up loopback
and sets `ip_unprivileged_port_start=0` inside that network namespace. A nested
bubblewrap user/PID namespace drops all capabilities before its inner process
creates loopback listeners and starts MDK. Startup tests capability masks,
binding port 443, refusal to modify loopback, absence of default routes and the
empty resolver configuration. Host namespace sysctls are not modified.

Each approved DNS origin maps to a private loopback address/port and a Unix
bridge. A host broker connects only to that configured origin; TLS, SNI and
certificate verification remain end to end in MDK. This confines physical
origins, not URL paths inside TLS or the behavior of an approved relay. Only
root-path `wss://` origin URLs are accepted. The daemon has private writable state,
restricted mounts and no ambient host socket directories or external DNS.

The command worker runs outside the MDK sandbox as the server's local non-root
Unix account. It is not an operating-system sandbox for commands authorized by
that account. The server chooses cwd and explicitly permits shell execution if
desired. Frontend shutdown leaves detached command workers running; daemon
namespace teardown terminates processes inside that namespace.

SQLCipher encrypts engine state and retained stdout/stderr. Its colocated
private `engine.key` supports unattended detached-worker recovery; possession of
both key and data defeats that storage protection. Frontend session metadata and
retry journals are separate private 0600 JSON files, not SQLCipher databases.
An outstanding retry journal contains original command arguments and a token.
Completed waits and terminal status/cancel observations scrub those fields logically, without a forensic-erasure promise.
Supplied Nostr secret-key import is not implemented; initial identities are
generated and persisted privately by MDK.

## Delivery and retention limits

History scans request the newest 100 kind-30100 records and MDK returns that
window oldest first. The bounded seen-ID cache holds at most 2048 IDs per group.
After the initial stale baseline, a full window whose oldest ID is unknown fails
visibly as a history gap. No sender timestamp is treated as an arrival cursor.
A delayed event inside the current window is found by its ID; a newly received
event backdated entirely outside the newest 100-row window cannot be detected by
this interface. This is a bounded prototype delivery limit. Stop-and-wait
requests, challenge renewal and client deadlines expose unavailable responses
without inventing replacement invocations. A transient response-send failure can
leave the rest of that scan unanswered; the client's eight-second retransmission
and cached replies recover them. Semantic room faults are quarantined privately
and persistently; transient RPC failures retry. At most 32 active/pending rooms
and 256 quarantined rooms are retained. Exhausting quarantine blocks new
enrollment until operator intervention.

Server startup and existing-room enrollment baseline historical frames as stale.
Fresh hello nonces obtain boot-specific challenges with 60-second monotonic
lifetimes. Engine authorization and durable invocation deduplication provide an
additional launch boundary. The same authenticated principal can inspect its
jobs across independent sessions. There is no endpoint ownership/takeover or
interactive terminal policy yet.

New conversations default to a 24-hour application-message retention policy;
the CLI can select 1–604800 seconds. Local expiry sweeps report unread,
clock-skew and scan-bound deferrals explicitly. Relay storage and backups are
outside the client's deletion control. This is best-effort retention, not a
guaranteed deletion deadline. Engine output/replay retention is configured
separately. Initial and retry submission preserve argv, but interrupted client
stdin is not automatically persisted and replayed.

The prototype remains Linux-only and single-device. Interactive PTYs/tmux,
simultaneous read/takeover, aliases, SRV discovery, supplied-key import, privileged
user mapping, X11, forwarding and file transfer belong to later phases.

## Static quality check disposition

`ripwire . --quality-delta` was run against baseline `192408c` and exited 2;
this is not reported as a clean quality gate. Its five gating rows comprise an
unchanged `Engine::submit` reported as dead, normalized-token similarity between
the engine fault labels and bridge error-code mapping, two JSON encoders, and
similar independent test setup. Runtime bridge tests exercise `Engine::submit`;
the fault labels and error codes have distinct contracts. The JSON encoder
provides canonical protocol bytes and sanitized serialization failures. The
small test fixtures remain local to independent suites. These findings were
reviewed rather than suppressed or hidden by changing the baseline. New-symbol
rows include dynamic Python constructors/test methods, dispatch complexity and
explicit protocol parameter lists. The sandbox duplex pump was split into
focused read/write/half-close methods with a regression test for full buffers.
The remaining dispatch and validation code was independently reviewed by Claude;
this prototype makes no claim that the static report is empty.
