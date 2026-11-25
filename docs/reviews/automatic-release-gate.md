# Automatic GitHub release gate review

Claude Sonnet reviewed the phase design in auto mode. The review required
exact candidate binding, a bounded artifact inventory, mandatory matrix and
benchmark results, and distinct prepublication and public HTTPS checks.
The follow-up approved the revised design. The initial design session was
`4f48ec5c-ec3e-424c-bb4c-4beeaddca639`.

Claude Sonnet 5.5 approved the integrated implementation in auto mode
in session `cadb625d-16dc-4bcb-af50-e67bb9a3bc74`. The reviewed tree was
`afb729f2378ffaac1a371076a19e32dd73ef4052`. The review covered candidate
source and attempt binding, publication recovery, read and write permissions,
matrix aggregation, artifact names and pagination, and release metadata.

Claude ran 263 Node tests, 24 focused workflow policy tests, and actionlint on
the three modified workflows. Additional local checks passed 46 workflow
tests, 65 release tests, 12 ontology tests, and the staged ontology freshness
check. Ripwire identified growth in the policy tests and pagination and
publisher code, alongside an unchanged Rust function. The Node suite covers
all coordinator callers identified by its change analysis.

The reviewer noted that later cohorts use more API calls because their run
lists more artifacts. The existing rate budget remains binding. Recovery
requires a whole publisher attempt. GitHub's rerun of only failed jobs can
reuse earlier attempt jobs, and the handoff checks reject their evidence.
