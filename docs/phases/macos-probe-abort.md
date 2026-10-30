# Diagnose the macOS confined probe abort

The user's experimental.2 output establishes that the unconfined broker probe
passes and the first confined broker probe terminates with SIGABRT (-6). It does
not establish whether the C helper reaches main or performs a TCP connect. Its
ordinary socket failures return 2. The production sandbox profile is balanced;
there is no justified syntax correction. Missing loader/runtime access remains
a hypothesis until native evidence distinguishes it.

This increment adds a manual, hosted `macos-15` diagnostic workflow and a small
test-only Python driver. It changes no production profile, helper, daemon,
installer, key storage or confinement gate. Root obtains Claude design approval
before agents implement; Claude reviews the integrated code before dual push
and dispatch. The existing native validation workflow remains unchanged.

The driver requires Darwin and an ordinary user, creates one owned private
canonical directory under `/private/tmp`, and compiles the existing exact
`tools/native/marmish-probe.c` using exactly `clang -Wall -Wextra -Werror -O2
-mmacosx-version-min=15.0 SOURCE -o EXEC`, then `/usr/bin/codesign --force
--sign - EXEC` and `/usr/bin/codesign --verify --strict EXEC`. Record the fixed
compiler flags in the report. Copy the current production PROFILE into the owned
directory. Construct commands through DarwinConfinedDaemon._command using only
owned fixture paths and a loopback listener, without starting MDK or its broker.
Use a construction-only `DarwinConfinedDaemon.__new__` fixture with `_directory`,
`home`, `socket` and a stub `_broker.address = ('127.0.0.1', listener_port)`.
Never call start or _prepare. HOME and the executable/profile directory are
separate siblings; CONFIG and ACK are outside HOME as in production. Assert
written profile bytes equal imported PROFILE before hashing.
The listener holds a bounded backlog for the two TCP connects; no external
network, DNS, relays, Nostr keys, user profiles or Keychain operations are used.

Run five subprocesses sequentially, continuing after failures: unconfined and confined `read /dev/null`,
then unconfined and confined TCP to the same owned 127.0.0.1 listener port,
then confined `/usr/bin/true` using the same command builder and profile. Mark
probe confined results control_invalid if their matching control fails. Each receives
stdin/stdout DEVNULL, exactly `env={'PATH': '/usr/bin:/bin'}`, `close_fds=True`, and a five
second timeout. Capture stderr privately to a temporary file and read at most
4097 bytes. Decode with replacement, remove control characters except newline/tab, and
redact owned root, runner home, checkout/workspace, system temporary root and
username before truncating the sanitized output to 4096 bytes. Also redact
trailing partial sensitive tokens at the capture boundary. Mark truncation. This stderr is from
trusted source-built synthetic probes with no secret input; never enable raw
stderr reporting for real identities or production daemon processes. Do not
query or upload host crash reports, environment dumps or system-wide logs.

The finite JSON report records schema, platform/architecture, source PROFILE and
probe-source SHA256, and each fixed case's exit code, signal, timeout and bounded
sanitized stderr. Build/sign failures are fixed categories with numeric status,
not raw compiler commands or exception text. Exit nonzero when a case fails;
Python 3.11 or newer is required; missing/old Python is a fixed failure category.
The report is written outside the fixture before cleanup, exclusively with
mode 0600 and no symlink following. The workflow must not relabel the original sandbox abort as success. Always
upload only the report, not the fixture tree or raw temporary captures. Use
read-only workflow permissions, pinned checkout/upload actions, no caches,
credentials or privilege, and a ten-minute job timeout. Trigger only on
workflow_dispatch, hosted macos-15, persist-credentials false, artifact upload
if always(). Keep the driver out of release file lists and DARWIN_NATIVE. Fixture cleanup closes
the listener, reaps each owned child and removes the owned temporary directory.

Offline tests cover CLI/platform refusal, report bounds and redaction, normal,
signal, timeout and compiler failure records, exact use of production command
construction, profile byte equality, report emission on signal/timeout and
cleanup on failures. Tests use mocked subprocesses and never
claim Darwin confinement. Native acceptance is the actual hosted run: preserve
both successful controls and failing/passing confined cases. Evidence may
justify a later separately reviewed minimal policy fix, which must still pass
the full existing allowed/denied native preflight and actual Marmot commands
before claiming macOS runtime support. No weakened fallback is authorized.

Empty stderr remains inconclusive: record it honestly and design a subsequent
bounded investigation rather than scraping host logs or guessing permissions.
