# macOS startup abort: confirmed boundary and proposed next investigation

Hosted [run 41132685757](https://github.com/Epiphytic/marmish/actions/runs/41132685757)
executed source `e6ba0b471c856453b46e24b2586131f48e9639db` on ARM64. It started
within the user's five-minute runner threshold and completed as **failure**, not
as a passing confinement gate. The unchanged probe's unconfined read exited 0;
its confined read exited -6 / SIGABRT, with empty stderr. The report is preserved
in [sanitized validation evidence](../validation/macos-crash-run41132685757.json).

Exactly one new candidate matched the owned child PID and the reviewed redacted
path rule. Its unique process name, image name, architecture and non-nil Mach-O
UUID also matched. The exported path-shape facts are `private_tmp`, four
components and one whole-component star. This establishes that the OS uses one
of the explicitly validated collapsed-middle forms; no raw path, PID or UUID is
published.

The crash facts show `EXC_CRASH`, `SIGABRT`, termination namespace `<0x23>`, code 2,
and an entirely dyld startup stack: `ignition_halt`, `boot_boot`, `ignite`,
`dyld4::CacheFinder::CacheFinder`, `DyldCache`, `ProcessConfig`, `start`. No ASI or
termination-details string is available. The read probe has not reached its
ordinary file operation, and the earlier TCP diagnosis does not establish a
broker connection failure.

Apple's [reason namespace definitions](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/sys/reason.h#L127-L148)
identify decimal 35 (0x23) as LIBIGNITION, separately from SANDBOX (25) and SECINIT
(47). Apple's [dyld cache-discovery implementation](https://github.com/apple-oss-distributions/dyld/blob/fd8d0c4d52320ebf64db34f3cb280310d905c5ae/dyld/DyldProcessConfig.cpp#L1031-L1118)
calls `ignite` while finding the shared cache and cryptex. That is consistent
with the observed stack, but the public source has not been matched to this
runner's exact dyld build. No implementation of `ignition_halt` or `boot_boot`
was found in that official source repository.

**Established:** failure is in loader/libignition startup under the sandbox,
before the synthetic probe's read or TCP logic. **Unknown:** what operation or
resource fails; what LIBIGNITION code 2 means on this OS build; whether the
missing permission concerns a file, executable mapping, Mach service or another
operation. This evidence does not justify any particular production permission
change, a Keychain workaround or bypassing confinement.

## Proposed bounded diagnostic increment, requiring design review

The next useful discriminator is an actual denied operation for this synthetic
child. A separate reviewed diagnostic may query the ordinary user's unified log
on the disposable hosted runner, restricted to a short measured child-execution
window and the exact unique probe basename plus owned Popen PID. This would be
an explicit extension to the prior no-log-query diagnostic design, not an
unreviewed change to its contracts.

The proposed query must run without sudo, preserve the unchanged production
profile, use an absolute `/usr/bin/log` executable, a fixed argument structure
and a timeout, and match both the unique probe name and PID in the same denial
record. Raw output stays in a private fixture file; cap capture size and record
count before parsing, and never upload it. Do not query an entire host log,
other processes, host crash directories, identity state or Keychain state.
Missing permissions, no matching record or unrecognized formats remain explicit
inconclusive results.

The finite report should contain only an allowlisted operation enum and enough
sanitized system-target information to design a narrow fix. Define the exact
system-target allowlist, control-character removal, host-identifier redaction,
length limits and rejection cases in the detailed design before implementation.
Never emit arbitrary log messages, paths to user data, UUIDs, host names or
unrelated records. Include negative tests proving nonmatching PID/name/time
records cannot enter the report. Keep the current native abort as a nonzero
result even if a useful denial is found.

After concrete evidence identifies the denied operation, review a separate
minimal production fix with Claude. It must pass the complete existing allowed
and denied macOS preflight, preserving filesystem/control-state and network
isolation, followed by real client/server command execution through Marmot.
Static doctor checks, successful compilation, source research or a diagnostic
query alone do not establish working macOS support. No production change or
native rerun is authorized by this analysis document itself.
