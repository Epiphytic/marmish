# Keyless unshare permission classifier

Actual native ARM run41400361937, published source
d004b191de597d1598b5d8b6567270290909aaf7, job125130666393, ran the reviewed
keyless diagnostic. Authenticated Bubblewrap acquisition passed; first stage
`/usr/bin/unshare --user --map-root-user --net /usr/bin/true` returned1 with no
signal at2026-11-02 06:31:40UTC. No Bubblewrap stage ran; owned cleanup passed.
This identifies the first failing configuration but no permission cause yet.

Scope: tools/marmish_ci_bwrap.py and its unit tests; this design, sanitized
native evidence and actual reviews. No workflow or production transport changes.
Use the already published diagnostic workflow for subsequent native validation.

The existing bounded runner already reads at most its output limit into a
private local bytes object before checking returncode. At that exact point,
classify only equality with either complete byte line:

- `unshare: unshare failed: Operation not permitted\n`
- `unshare: unshare failed: Permission denied\n`

No trimming, substring, case folding, regex, decode, locale fallback or arbitrary
text extraction. Primary util-linux source confirms `unshare failed` is the
fixed err(EXIT_FAILURE, ...) call after a failing unshare syscall:
https://kernel.googlesource.com/pub/scm/utils/util-linux/util-linux/+/refs/tags/v2.41.1/sys-utils/unshare.c
These English whole lines are candidates to match future native evidence, not
a claim that previous output contained them. Other locale/output stays unknown.
The diagnostic environment remains the existing build_environment unchanged.

ToolFailure retains fixed args/str/repr `bwrap_tool_failed`, bounded returncode,
and one boolean permission_denied attribute. The constructor accepts only an
exact bool true for that attribute; never accepts/stores raw output. run passes
the derived boolean; no body/argv/path is attached to exception, repr or report.
Success bytes and existing probe/bootstrap behavior remain unchanged.

Only the diagnostic's first `unshare` stage maps reason to permission_denied
when returncode is exactly int1 and the exception's attribute is exactly True.
Other nonzero results remain nonzero_exit, including matching output from
Bubblewrap stages, other returncodes or mutated nonboolean attributes. Numeric
rc/signal and all bounds/cleanup/privacy behavior remain unchanged. This reports
the utility's permission diagnostic for this exact command, not a particular
host policy, AppArmor rule, missing capability or namespace subtype. No host
changes are authorized or proposed by a match.

Offline tests exercise both exact lines via mocked bounded runner output and
inspect exception args/repr/attributes; reject missingnewline, extra lines,
prefix/suffix, NUL, invalidUTF8 and sentinel payloads; validate firststage and
exactstatus/bool restrictions; preserve existing probe contract. Run affected
helper/workflow/frozen releasevocabulary checks; no native work during reviews.
Actual Claude design approval precedes changes; actual auto code review follows.
Parent owns publication and native dispatch. Unknown output remains private.

Design review approved. Adopted notes: permission_denied extends the prior
plain-data reason vocabulary by exactly one value; no frozen require/SandboxError
category changes. Constructor default isFalse and stores value isTrue. Tests
assert environment unchanged, previous probe category unchanged, matching output
with rc2/negative and laterstage still nonzero_exit.

Code review approved; adopted optional privacy hardening: delete the bounded
body local before raising ToolFailure so that its traceback frame does not
retain those captured bytes. The private temporary output file still closes
under the existing context manager.

Final validation after hardening:60tests and96subtests passed across
ci_bwrap, namespaceworkflow, managedworkflow and release suites. py_compile and
scoped diff --check passed. Actual Claude design/code/final followup all approved;
no nativeclassifier result exists yet. Parent coordinates next native dispatch.
