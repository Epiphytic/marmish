# Installed Actions coordinator: local checks

The coordinator is CI-only Node code. Installed Marmish and its acceptance driver
remain Rust; neither the installer nor product calls Node, npm, Python or uv.

Local Node v22.22.0 ran 24 tests successfully on 2026-11-03. They cover immutable
records, exact ZIP members, size ceilings, redirects and credential isolation,
artifact run/attempt names, budget persistence, narrow retries, local publication
during API backoff, cancellation of an in-flight request and upload, delayed
child exit/IPC delivery, actual child ownership and forced cleanup, successful
three-record exchange, and transport errors racing the final success path.
The critical success-race regression also failed with its fix removed; Claude
independently repeated both mutants and loaded runs. Actual code review r4
approved this component. These dependency-injected checks do not qualify the hosted artifact service.
The action uses the runner's Node 24; hosted execution is a separate gate.

The lockfile pins @actions/artifact 6.3.1 and yauzl 3.4.0. The online npm audit
reported seven moderate transitive advisories, zero high and zero critical.
The affected glob/minimatch paths are reached through archiver directory/glob
walks; this integration uploads one explicit frozen file using SDK zip.file,
never a pattern or directory walk. Upload runs in an owned bounded child.
Claude independently inspected and accepted this reachability analysis in code
review r2. npm's suggested major downgrade to SDK 2.1.4 was not applied. Recheck
when upstream updates these transitive dependencies. An offline audit reporting
zero vulnerabilities is not evidence that the advisories disappeared.

The action is specific to github.com. It requires actions:read, a token input
(default github.token), and a shared private budget workspace for exchange/run.
The parent of inbox/identity directories is supervisor-owned, on the same
filesystem, and outside the driver's exact directory-membership checks.
Public record staging is private and cannot overwrite a published record.
Cancellation can cause the host runner to kill the job before cleanup finishes;
no missing join, ambiguous state or retained process is reported as success.
