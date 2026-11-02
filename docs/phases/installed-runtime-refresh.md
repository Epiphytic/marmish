# Experimental runtime refresh for installed acceptance

Beads: `marmish-2t0`, dependent on the approved production macOS loader repair
and the native installation/compiler prerequisites. This is a patch distribution
under the existing experimental-release authorization, not a claim that the
nine-pair matrix has passed. It uses the unchanged
[distribution procedure](distribution.md), including its separately attributed
local exact-archive acceptance route when hosted namespaces are denied.

## Scope and sequence

1. Complete independent design/code reviews and focused tests for the production
   loader repair and any compiler/probe prerequisites. Publish those sources and
   this reviewed plan to both source mirrors. Run the native macOS diagnostic;
   require the production profile's positive probes and its forbidden-file,
   forbidden-write and unselected-network negative probes to pass. Synthetic
   probes do not establish actual MDK or identity acceptance.
2. Freeze one exact, clean master commit and build `0.1.0-experimental.3` using
   the existing manual `release.yml`, with publication enabled. Its unchanged
   `needs: build` gate allows publication only if all hosted gates pass. In that
   case, use that CI publication and do not run a second local publisher. If a
   hosted build fails, the CI publish job remains skipped.
   Coordinate and record the source freeze with other active agents in Beads
   before dispatch. On either publication route, hold all later source,
   documentation, review and tracked Beads-export commits until visibility.
   If master advances, abandon the candidate. A leftover draft is not permission
   to delete it or bypass freshness checks; a new reviewed build may need a new
   version. Never use "Re-run failed jobs" to mix attempts. A non-namespace
   build failure or any CI publication failure blocks publication and requires
   a fresh reviewed build; it does not authorize a local publisher retry.
   Inspect runner assignment within five minutes. All three target archives must
   originate from that single run/attempt and commit, with exact manifests,
   verified licenses/provenance and native installation checks. Preserve any
   failed hosted execution report without changing its outcome.
3. If the hosted AMD64 execution check fails solely because its host denies the
   existing product namespace boundary, follow the already reviewed local
   operator route in `distribution.md`. Use the original downloaded archive,
   unchanged acceptance CLI at the frozen clean source, disposable keys, a new
   private prefix, the explicit EU/US test relays and no concurrent live test.
   Require all normal installation/help/idempotence/uninstall checks, all 15 live
   assertions, exact source/archive/wnd digests and complete cleanup. Missing
   artifacts, different failure causes or local acceptance failure block
   publication. Activate this exception only when the owned unshare probe
   reports EPERM/EACCES (`permission_denied=true`) and that same run's live
   acceptance fails at namespace startup. A sysctl value, generic failure or
   unavailable probe is insufficient. Record the 40-character source SHA, run
   and attempt in Beads and validation evidence before using the exception.
   Do not relax confinement or modify the publisher's gates.
   This independently reviewed plan covers at most one additional use of the
   exception, bound to the recorded experimental.3 source/run/attempt.
4. For the local exception, audit original versus aggregate reports. Only the prescribed
   `operator_provenance` object may be added to an acceptance report in the clean
   aggregate inputs; retain the untouched original separately. Only when the
   namespace exception is active, invoke the existing publisher CLI, preserving
   its exact-archive aggregation, draft verification, current-master and tag
   checks. A fully passing hosted route uses those automated publisher checks
   before visibility; operator inspection then follows publication.
   Preserve experimental limitations and explicitly
   qualify any local acceptance in the published release body.
5. Verify all public assets/checksums and source/tag bindings. Verify that the
   GitHub tag created by the publisher resolves to the frozen source, then
   push that same tag to Radicle without forcing conflicting refs. Announce to
   Iris and verify external source
   availability. Record exact evidence and any partial publication in Beads.
   Only then change the installed harness's fixed version to experimental.3,
   review that change and run the real-install nine-pair workflow. Test failures
   remain failures; the macOS and ARM64 runtime capability claims change only
   after actual installed identity/command evidence supports them.

## Acceptance and performance evidence

The refresh succeeds only when all three exact native archives are published
through the existing verified gate and accessible with their checksums. It does
not close `marmish-0po`. That task still requires all nine client/server cells,
independent authenticated receipts, exact byte/exit preservation and complete
cleanup in one attempt.

After the later matrix run, publish sanitized JSON and Markdown containing
source/run/archive identities,
explicit relays, installed interpreter, storage mode, outcomes, cleanup and
timing definitions. Publish measured command wall time, first-output latency,
cold-status time, exact stdin/stdout/stderr bytes and the two goodput
rates defined in `docs/agent-spec.md` only for verified cells. Keep failed cells
null with zero samples;
successful setup durations remain a separate table. Do not derive one-way
latency, percentiles or cross-platform rankings from one observation.

No identity, SQLCipher key/database, raw private stderr, runner home path or
temporary executable is added to source or release evidence. Remove disposable
identity state, private prefixes and temporary inputs after capturing evidence,
using the existing owned-process cleanup checks. Publication credentials must
not enter build/test children or evidence. Every completed
increment receives an actual Claude auto-mode code/evidence review, focused
checks, an explicit staged audit and dual-mirror publication.
