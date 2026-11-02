# Owned macOS loader permission matrix

Hosted [run 41396357778](https://github.com/Epiphytic/marmish/actions/runs/41396357778)
now completes the owned lifecycle, but the unmodified production profile aborts
all confined binaries before main. Controls succeed. The attributed crash has
CacheFinder → ignite → boot_boot → ignition_halt, namespace 0x23, code 2;
there is no attributed denial. Apple's [XNU reason header](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/reason.h)
identifies 35 as LIBIGNITION. Apple's [dyld CacheFinder](https://github.com/apple-oss-distributions/dyld/blob/fd8d0c4d52320ebf64db34f3cb280310d905c5ae/dyld/DyldProcessConfig.cpp)
invokes ignition while discovering its shared-cache directory. This localizes
startup failure but does not identify a denied syscall or prove a profile fix.
WebKit's [Apple-maintained sandbox](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/WebProcess/com.apple.WebProcess.sb.in)
separately grants file-read* and file-map-executable for runtime libraries.
Marmish currently has reads but no mapping grant, and no root-directory read.

Add only an opt-in --loader-matrix mode to the existing synthetic probe diagnostic
and focused tests. Keep the default diagnostic, production PROFILE, native probe
source, installed state machinery, MDK, identity and Keychain behavior unchanged.
The parent wires a separate macOS job with its own ten-minute timeout into the
existing manual hosted diagnostic workflow. No production change follows without native evidence
and its own scoped review.

Test all eight combinations of three fixed independent permission additions,
including the unchanged baseline. No arbitrary flags, paths or profile input:

* `root`: file-read-data on literal `/` only (directory handle/enumeration, no
  descendant read or write).
* `map`: file-map-executable on the exact EXEC literal and fixed system runtime
  subpaths `/usr/lib`, `/System/Library/Frameworks`,
  `/System/Library/PrivateFrameworks`, `/System/Library/dyld`.
* `cryptex`: file-read* and file-map-executable on the fixed shared-cache-only
  subpaths `/System/Cryptexes/OS/System/Library/dyld`,
  `/System/Volumes/Preboot/Cryptexes/OS/System/Library/dyld`, and
  `/private/preboot/Cryptexes/OS/System/Library/dyld`.

The cryptex factor deliberately combines reads and maps, some overlapping the
existing /System read grant; its aliases cover resolved system cache locations.
A failed matrix rules out only these additions, not missing fsctl/ioctl/Mach
permissions.

Append only the selected fixed rules to a byte-exact base PROFILE copy inside
the owned private fixture. Before each variant rewrite exactly
<control>/profile.sb, read it back and require exact expected bytes and SHA256
before any case. Record each SHA256 and factor names. Generate every confined
argv through the unchanged daemon._command. EXEC remains the signed disposable C probe. Network rules, writable HOME,
ACK exceptions, default deny, and exec restrictions remain exactly the baseline.
No map-all, read-root-subpath, extra Mach lookup, wildcard network or other broad
permission. Even the full combination is diagnostic-only, not a production mode.

For each variant run the same five capabilities with the existing five-second
bounded run_case: positive read `/dev/null`, positive TCP to the owned selected
loopback listener, negative read and write to the fixed owned file <fixture>/outside/synthetic
(0600, fixed synthetic bytes), outside HOME and control and distinct from
EXEC/CONFIG/ACK/ACKTMP/SOCKET, and negative TCP to a second owned unselected loopback listener. Run each
corresponding unconfined control once first; all five must exit exactly zero
before any variant can be eligible. Invalid controls do not stop the matrix;
mark every variant case control_invalid and force overall failure. Each variant passes only with exact expected
codes [0,0,2,2,2], category completed, no timeout and no signal; crashes cannot
count as denied capabilities. Keep going after failures to reveal interactions.
Both listeners have backlog 32, enough for every control/variant even if a denied
TCP unexpectedly succeeds; no DNS, public network or external targets. No payload
or identity enters these fixtures; file bytes stay synthetic and private.

Use existing build/sign/verify recipe once; existing ordinary-user macOS/Python
requirements and private /private/tmp fixture ownership. A separate schema-2
report has fixed mode, platform/architecture/compiler flags/source digest/base
profile digest, five controls, eight variants (id/factors/profile digest/five
cases/ok), finite failure and overall ok. Each case reuses the redacted schema with matrix stderr capped to 512 UTF-8
bytes after existing redaction, setting stderr_truncated on any further truncation
(the default diagnostic retains its 4096-byte cap). Serialize UTF-8 JSON bytes with ensure_ascii=False before the exclusive write,
enforce <=64KiB, and on overflow replace it with a fixed report_too_large failure
report. No raw captures or paths. Add a maximum non-ASCII stderr test for all
45 cases. Always write a report, including fixture/build failure, and return
exit 0 iff overall ok, otherwise 1. Record baseline_reproduced as true only when
both baseline positive cases reproduce the observed completed SIGABRT/exit -6
without timeout. Overall ok requires valid controls, baseline_reproduced and at
least one exact passing variant; it never means production readiness. A passing
baseline or all-variants-fail produces exit 1 and no production inference.
Each variant records finite classification passed/loader_abort/other using exact
return/signal facts; other includes profile rejection and needs further evidence. Fixture cleanup is mandatory. At most 45 probe
children, each <=5 seconds; build stages <=30 seconds each; hosted 10-minute
job bounds the complete operation. Owned C probe cannot fork; no daemon starts.

Offline tests assert exact fixed rule text/eight unique combinations/base
unchanged, correct commands and separate listeners/outside file, exact return
codes rather than crash-as-denial, invalid controls invalidate all variants,
continuation after failures, bounded report/privacy/exclusive ownership, build
failure and cleanup, opt-in CLI compatibility. Obtain actual Claude auto design
approval before implementation and actual integrated code review after tests.
Parent publishes the reviewed increment, runs the hosted matrix, preserves its
sanitized report, then selects the smallest empirically passing factor set (cardinality, then fixed
root/map/cryptex order for deterministic ties) for
an independently reviewed production-profile correction with the full existing
native confinement positive/negative preflight gate.
