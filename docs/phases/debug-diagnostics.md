# Debug diagnostics increment

## Scope and acceptance

Add opt-in `--debug` to the installer and both frontends. Temporarily use
`sh marmish-install.sh --quickstart --debug` in the README quickstart.
This increment diagnoses failures; it does not resolve or bypass the macOS
confinement blocker and does not certify native execution.

Installer diagnostics report fixed installation/setup stages, platform,
architecture, Python version and numeric failure details. Frontend diagnostics
report fixed profile/key-session/transport/RPC stages and the existing native
preflight callback's stage, bounded errno/exit status and signal number/name.
Emit bounded JSON records to stderr; preserve application stdout, error category,
exit status, key storage behavior, sandbox policy, and cleanup semantics.
Never print raw exceptions, tracebacks, subprocess stderr, paths, user input,
identity material, credentials, command contents, RPC payloads or environment.
Use an explicit field/value allowlist; syntactically safe arbitrary strings are
not sufficient redaction. Debug logging must not replace an original failure.

## Interfaces and compatibility

Implement diagnostics inside existing packaged modules, without adding runtime
archive files. Each frontend guarded invocation owns/reset its diagnostic state.
The shared parser recognizes --debug with an argparse action so --doctor's early
exit can also report context. Do not scan remote argv or option values for flags.
Instrument the profile flow with fixed stages. Assign a diagnostic callback to
the transport instance and use the existing Darwin _preflight callback protocol;
production refusal and cleanup rules must remain unchanged. The existing spike
callback suppresses secondary cleanup errors after a primary failure; production
must explicitly preserve its previous raise behavior when attaching diagnostics
(e.g. a keyword-only preserve_cleanup_errors switch with the existing default
for spike callers). Add a regression for this distinction.

Installer --debug sets MARMISH_DEBUG=1 only in owned child environments. Updated
frontends treat that exact value as debug opt-in, also supporting their own flag.
This avoids passing an unknown CLI option to the existing experimental.1 archive.
Older archives ignore the variable and retain their current category-only output;
the installer must clearly report this compatibility limit (without claiming the
new runtime diagnostics are present). Updated binaries require a subsequent
built/reviewed archive; changing the Iris installer does not modify an immutable
installed runtime. Do not patch downloaded package contents.

## Execution and validation

After independent Claude design review, delegate installer and frontend edits to
agents with disjoint ownership. Root integrates README/specification changes.
Test installer plain/debug help and quickstart child inheritance, stage/failure
records, legacy compatibility, and signal cleanup. Test both frontend flags,
remote literal --debug not enabling diagnostics, doctor order, invocation reset,
stdout separation, and secret canaries in errors/argv/env. Mock Darwin preflight
control=0/confined=-6 and timeout/OSError to prove safe failure-stage reporting.
Run existing relevant Python suites and syntax checks. These hermetic checks are
not native macOS acceptance; preserve the outstanding native blocker separately.
Obtain Claude auto-mode integrated code review, incorporate useful findings and
retest. Audit explicit staged paths, commit, push GitHub and Radicle, verify both,
and record sanitized validation/review evidence in Beads before closing.

## Design review resolutions

- Production preflight uses explicit cleanup-error preservation, independently
  of observer presence. The spike default remains unchanged.
- Emitters are best-effort and catch Exception around record generation/writes,
  including closed/broken stderr. Records are small and bounded; do not claim
  wall-clock nonblocking guarantees for an arbitrary stderr device. A blocked
  user-provided stderr has the same output-flow limitation as normal errors.
  Preserve KeyboardInterrupt/SystemExit and normal signal handling. Only fixed
  error-type categories, bounded errno/HTTP status/exit integers, enumerated
  platform/architecture, numeric Python version and signal.Signals names pass.
- Existing DoctorAction exits early: document `--debug --doctor` flag order;
  MARMISH_DEBUG=1 works independently of order. No raw argv pre-scan. Full option
  names are documented; adding --debug may make old --de abbreviations ambiguous.
- Child env means dict(os.environ) with exact MARMISH_DEBUG=1 when installer debug
  is enabled. Ambient MARMISH_DEBUG=1 still enables updated frontend diagnostics
  without enabling installer diagnostics. --debug works with uninstall/version.
  Always explain that runtime details require an archive built after this change;
  do not pretend capability detection. Installer debug failure records exclude
  raw OSError text (normal pre-existing error messages remain otherwise intact).
- Linux emits profile/transport stages; Darwin additionally emits preflight probe
  details. No import of the unpackaged spike module. Prefix records with fixed
  `marmish debug: ` / `marmish installer debug: ` and schema version. Stderr may
  contain remote output and is not authenticated diagnostic evidence; test a
  record-shaped remote line does not change program behavior.
- Keep existing keychain helper environment allowlist, no diagnostic setting in
  helper/daemon environments. Track removal of the temporary README flag in Beads.
