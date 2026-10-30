# Owned synthetic macOS crash context

Hosted run 41129112655 reproduces SIGABRT for confined read, TCP and system true;
both unconfined controls pass, and every stderr is empty. No product permission
change is justified yet. This diagnostic-only continuation retains the previous
production-profile/recipe/command-builder contracts and all runtime gates.

Add a separate test-only driver importing the existing diagnostic fixture/build/
redaction/report helpers. Compile the exact existing probe with the reviewed
recipe into an owned canonical private /private/tmp fixture, using a unique
basename `mp-` plus eight hex characters derived from its random fixture name
(11 ASCII characters, avoiding process-name truncation). Use sibling HOME/control layout,
unchanged PROFILE, and the production command builder. Run an unconfined read
control then one confined read /dev/null, sequentially, with the same fixed PATH,
closed descriptors, Python3.11+/Darwin/non-root gates, DEVNULL stdin/stdout and private bounded stderr capture.
Use Popen to retain the actual owned child PID, wait with a five-second timeout,
and kill/reap on timeout or exceptional cleanup. No Keychain, identity, MDK,
external network, user profile, or production changes. An owned loopback listener
supplies the required BROKER parameter only; no traffic is needed for read.

If and only if that child aborts with SIGABRT, inspect the ordinary user's
Library/Logs/DiagnosticReports for filenames beginning with this exact unique
probe basename or the fixed `sandbox-exec-` fallback, and ending .ips. Snapshot
candidate names immediately before the confined execution and accept only new
files. Open the reports directory with O_DIRECTORY|O_NOFOLLOW, verify UID
ownership, enumerate only in memory, and open candidates relative to its dir_fd.
If absent, record reports_dir_absent and re-check after abort; never create it.
Never emit directory listings. Bound candidates to eight new matching files. Poll at most 20 seconds with
one-second intervals, no log-show or system report directory. Open candidates
with O_NOFOLLOW, require regular files owned by the current UID, bound read to
2 MiB, and parse only the normal two-object macOS .ips JSON form (metadata first
line and payload remainder). Require payload PID equal to owned Popen PID and
procPath equal to canonical owned probe path or exactly /usr/bin/sandbox-exec.
Record matched executable as fixed probe/sandbox_exec category, not its path. Reject mismatches with fixed
categories and never include their contents in a report.

Extract only fixed allowlisted facts: exception type/signal, termination namespace,
code/indicator and at most 1024 characters of termination details; `asi` string
values (or lists of strings) only for dyld, libsystem_secinit.dylib,
libsystem_c.dylib, libsystem_kernel.dylib, libc++abi.dylib and
libsystem_sandbox.dylib, at most six values of 1024 characters each; and at most 24
frames from the faulting thread. Frames contain at most 256 characters of symbol
and the basename of its referenced usedImages entry (no addresses, UUIDs, offsets,
source paths or other threads). Bound/sanitize/redact extracted strings with the
existing helper and owned root/home/checkout/temp/username tokens. Do not change
its existing behavior: a thin wrapper redacts each full input (input cap 64KiB)
before applying smaller character limits. Ignore all unlisted fields, including
byProc, coalitionName, bundle, identity, addresses and environment. Include fixed
category when context is unavailable, invalid, oversized, PID/path rejected,
parse failed or timed out. Record only the count of new prefix-matching files. Never
upload raw .ips, full JSON payloads, directory listings, host identifiers or
unrelated crash reports; do not delete OS-generated reports. Keep this feature
out of production diagnostics and release file lists.

Write a finite exclusive 0600 JSON report outside fixture, before cleanup, with
source/profile hashes, both case statuses and bounded extracted context. Preserve
nonzero status when confinement aborts. Extend the manual hosted workflow to run
this separate driver after the first diagnostic even when it fails, in a separate
private output directory, and always upload only its report. Both prep and run
steps have independent ids and if: always(), umask077; upload uses a distinct
artifact name and if-no-files-found:error. Existing workflow
permissions/timeouts/pins remain unchanged.

Offline tests cover new-name/PID/path selection, regular/owned/nofollow and size
checks, malformed/two-object parsing, bounded extraction/redaction, missing and
late reports, child timeout cleanup, and report emission on abort. Claude reviews
the design before agents implement and the integrated code before dual push.
Native evidence must identify the failing runtime operation before designing a
minimal production fix; any fix still requires the complete native allowed/denied
matrix and real Marmot command acceptance. If this remains inconclusive, record
that honestly and design a further bounded investigation without guessing grants.

## Native follow-up: rejected path attribution

Run 41130088901 passes the unconfined control and reproduces SIGABRT. One new
candidate matches the owned PID but fails the exact executable-path check, so
no facts were extracted. OS path redaction or aliasing is a hypothesis, not an
established cause. Retain strict failure until additional attribution is proven.

After build_probe succeeds and before either child, read the owned thin Mach-O
header directly with O_NOFOLLOW, require regular current-UID ownership, and read
at most 4096 bytes. Parse a 64-bit little-endian header for current platform.machine()
(arm64 or x86_64), bounded load-command count and lengths wholly inside that read,
and exactly one correctly sized LC_UUID command with a non-nil UUID. Invalid or
unsupported formats fail with a fixed category; no external tool or raw UUID is
exported, and UUID failure never downgrades attribution.

After exact new-candidate PID matching, classify procPath using fixed facts only:
prefix enum private_tmp/tmp/users/other; component count capped at 8; literal-star
component count capped at 8; exact unique basename match; procName match; alias match;
and UUID-image match. UUID binds the build, not the run: the run binding remains
the exact Popen PID, pre-spawn new-file snapshot, unique random basename and the
20-second collection window. Do not call UUID alone proof of process ownership.

Exact paths remain accepted. An alias may match only for an absolute bounded path
with strict resolution equal to the owned probe. For potential OS-redacted paths,
require a string <=4096 chars, absolute, no NUL/empty/dot/dotdot components and
exactly one component equal to `*` (never embedded stars). Only the exact literal
forms `/private/tmp/*/BASENAME`, `/tmp/*/BASENAME`,
`/private/tmp/*/control/BASENAME`, `/tmp/*/control/BASENAME` are candidates: these
represent either a collapsed middle or a redacted fixture component in our known
topology, not an assertion that either representation has yet been observed.
Before accepting one, also require procName equal to the unique probe basename,
and exactly one usedImages entry whose name equals that basename. If its path is
present it must end in /BASENAME; its arch must equal our parsed architecture;
its UUID must be a non-nil UUID string equal to the owned Mach-O UUID. These
conditions authorize diagnostic extraction only and never alter product policy.
The next native report records fixed path-shape facts and whether this validated
candidate was accepted, so actual OS representation is established from evidence.
Other patterns stay rejected. Record attribution probe_exact/probe_alias/
probe_uuid_redacted/sandbox_exec_exact, never path or UUID text.

Offline tests cover exact/alias/redacted UUID success, wrong/missing/nil UUID,
wrong procName/basename/PID/arch, duplicate usedImages names, unexpected patterns,
multiple/embedded stars, dotdot/oversized paths, malformed/oversized/duplicate UUID
load commands and unsupported Mach-O formats. Assert no raw path/UUID in reports.
Re-review this extension with Claude before implementation, then integrated code
and rerun native diagnosis. No permission change may pre-empt the evidence.

UUID/header parse failure is fatal before either child: exact and alias branches
never run without a valid UUID. Reject fat/universal and non-little-endian64 magic;
compare CPU type only, not subtype. Incomplete load commands within the 4096-byte
read are a fixed failure. Compare UUID objects parsed from raw LC_UUID bytes and
the crash-report string, case-insensitively. Alias checks use read-only strict
realpath resolution; no exception text is exported.
