# Installed CI HTTP diagnostics review

Beads `marmish-et1.1`; base `cf2f948bfed63e848b7e79d7f7a292aee9f39dd9`.
[Approved phase](../phases/installed-artifact-http-diagnostics.md). Scope: exactly
`tools/v02-actions/transport.mjs`, `transport.test.mjs`,
`supervisor-diagnostic.mjs`, `supervisor-diagnostic.test.mjs`, plus this phase
and review evidence. No Rust, workflows, dependencies or retry/security policy
changes. The root retired N qualification after repeated edge-copy artifact
exchange failures; no third blind retry or release qualification is claimed.

Claude Code with `--permission-mode auto`, model `claude-opus-5-5`, performed
actual independent reviews:

- Design session `b3a5e304-cdfc-41ed-825c-903185f45c95`: initial
  **CHANGES_REQUIRED**, response SHA256
  `f4344bd2af0512b2854656e1b865efe4ed07cebbe9ca01a4db2f4b578b930d0a`.
  Required capturing metadata and its fields exactly once before validation
  and serialization; incorporated before code, with changing-getter tests.
  Follow-up **APPROVE** before implementation, response SHA256
  `dc5dfb2cf857a69923b972428f4a742e06c87c54c6a80a2ade4d779f253085f2`.
- Completed-code session `bbb0c57a-b559-41a3-af64-f99662cf6e97`: **APPROVE**,
  response SHA256
  `501c2e7f7e42776ce1f98e7f1440694eed6636c72d52cad07d6e1e42f36028a2`.
  Reviewer verified frozen file hashes and independently ran focused49 and
  full209 Node tests, all passed with zero failures/skips. Nonblocking stale
  phase approval/result wording corrected afterward; code bytes unchanged.

Terminal `artifact_http` errors now carry only numeric HTTP status and trusted
`api`/`blob` request class. Public diagnostics copy both validated optional fields
or neither. Error messages, retry/backoff/deadline/API budget, redirect admission,
body handling, failure precedence and owned-child cleanup ordering stay unchanged.
No URL, hostname, headers, response text, tokens or arbitrary error values are
serialized. Invalid or throwing metadata falls back to the prior seven-field
schema. Tests include all-field exact-key validation, hostile/changing getters,
noncategory no-read assertions and actual transport-through-supervisor HTTP
fixtures for API and blob failures before child termination.

Author validation: focused49/49, full209/209, zero failures/skips;
`git diff --check` passed. All HTTP and child operations in these checks are
local deterministic fixtures. No native/Cargo, real relay, hosted workflow or
GitHub mutation was executed. The temporary local dependency symlink is excluded
from source and removed for handoff; package-lock bytes are unchanged.

Quality-delta exited2 with14 findings/8 gating, preserved without suppression.
The reviewer accepted the dispositions: unchanged Rust `request` location is a
name collision; getter `status`/`target` false dead-code reports are disproved by
read-count assertions; historical self/ambient churn does not justify refactoring;
minor class growth is four lines. No metric-driven code or acknowledgement file
was added. Change-context completed successfully; its name-based graph is
structural evidence, not runtime proof.

This increment makes future failures actionable; it does not recover the missing
status of runs42722743507/1 or42724923331/1, prove a provider/transient cause,
prove physical cleanup, or qualify a v0.3.1 release. Existing failed evidence is
retained. A new source/candidate and fresh required acceptance remain necessary.
