# Managed runtime packaging validation

Work tracked in Beads `marmish-x6z.1`. This validates the disposable harness,
not a production installer change or a release. Native Linux ARM64 and macOS
ARM64 results remain required before accepting the packaging phase.

## Local trials

The harness uses pinned uv 0.11.16 and CPython 3.13.13 with the exact published
Marmish experimental.2 archive. Early trials found harness assumptions about
uv's minor-version symlinks, the installed archive basename, and the unsupported
`wnd --version` option. These were corrected without changing product behavior.
Transient deletion of uv staging entries during size monitoring is tolerated
only for `FileNotFoundError`; other filesystem errors still fail the trial.

The full suite initially exposed test-environment assumptions: executable test
fixtures use shebangs that cannot contain spaces, confinement tests need sbin
tools, and Rust fixtures require an edition-2024 compiler. The suite now uses
the second, unspaced runtime slot, explicitly resolves the existing test-only
compiler, and includes system sbin. Separate launcher and argument-preservation
checks continue to use the spaced slot. Runtime subprocess environments remain
allowlisted. No additional test skip or product-guard relaxation was introduced to fix
these issues; the final report discloses the suite's existing skips.

Trial 10 passed 431 tools tests under managed CPython. It also passed artifact
digest/size checks, uv rejection cases, the exact embedded archive verifier,
two independent runtimes, both frontend help commands, static doctor, supported
native help, literal arguments and the runtime dependency audit. Cleanup
completed and the real uv directories' metadata snapshots were unchanged.
The runtime tree measured 98,220,404 bytes, 3,471 regular files and 1,048 links;
nine native objects were audited, with maximum referenced GLIBC version 2.17.
These interpreter measurements do not lower the bundled application's native
dependency requirements.

Trial 10 failed the retained launcher inside its independent offline namespace.
Although network isolation succeeded, the read-only host root had an unmapped
owner, so Marmish correctly rejected an installed path beneath that ancestor.
A separate disposable check with an owned tmpfs root, read-only system/scratch
mounts and the invoking non-root UID/GID passed the actual installed-file guard,
verified a different network namespace, and failed an external connection.
Trial 11 incorporated that correction and passed all implemented local gates:
435 tools tests, seven reported skips, independent network denial, both
retained launchers after removing uv/downloads/caches, and version two after
deleting version one. Cleanup completed and real uv-home metadata was unchanged.
Trial 12 repeated the proof after final review fixes and passed 436 tests,
with seven skips, the same offline gates and cleanup/home checks. The
[sanitized local report](managed-runtime-linux-amd64.json) records Trial 12
and includes the three unmet acceptance cases. This is local
AMD64 evidence only; the earlier failed trial remains part of the record.

## Evidence boundaries

The [freeze-time checksum crosscheck](managed-runtime-checksums.json) records
all nine artifact hashes matching freshly downloaded official checksum assets.
The checksum assets themselves matched their frozen hashes and sizes. This is
publisher consistency checking, not independent publisher authentication.

Reports export artifact provenance, fixed failure categories and unittest
identifiers. Raw unittest output is never uploaded; an explicitly requested
local diagnostic is exclusively created with mode 0600. Trials create no
identities, access no keychain and contact no Marmot relays.

The old Linux distribution still depends on `/usr/bin/python3` in its doctor
and confinement helpers. Fixed-interpreter launcher success does not establish
complete system-Python independence. macOS dyld/libignition confinement failure
remains separate and unresolved. The production managed bootstrap is gated on
the complete native packaging results and its conditionally approved design.

## Remaining acceptance gaps

Native CI and any outstanding
design cases must pass before phase acceptance. Current unit tests cover archive
headers, link/path refusal and environment construction; they do not establish
an end-to-end interrupted extraction, a substituted archive after the final
rehash, or all hostile ambient-configuration/current-directory combinations.
The expanded-size monitor polls, so its 1GiB limit is a best-effort detection
bound rather than a hard filesystem quota. These limits must not be represented
as completed security or confinement guarantees. uv installation itself uses
its pinned `--offline` flag; independent namespace denial currently verifies
retained application startup, not uv installation or its missing-local case.

## Static quality check

`ripwire . --quality-delta` reported exit 2, attributing changed short names
`run` and `artifacts` to the untouched Rust worker and an older JSON report.
A tools-scoped check similarly attributed `run` to untouched `build_mdk.py`.
`git diff HEAD --exit-code` confirmed all three reported existing files are
unchanged. These are symbol-identity collisions, not a passing quality gate.
The new harness's reported acceptance-function complexity and parameter counts
were inspected alongside the independent code review; unittest discovery
accounts for its reported dead-test symbols. No unrelated source was changed
to suppress those diagnostics.
