# Independent Claude automation design review, round3

Reviewer: Claude Code claude-sonnet-5-5, `--permission-mode auto`.
Session: `8a2ecc9d-489f-41cf-afbf-bc8aefe32a9e`.

**APPROVED** for `native-automatic-release.md`. B1 and B2 are both resolved. I made no edits and ran no tests, Cargo or live processes.

**B1: draft invisibility, resolved**
- Lines 43–48 say that read-only eligibility and inspect cannot see private drafts.
- The `contents: write` job is named the draft authority. Before its first tag write, it lists all releases, including drafts, with bounded complete pagination.
- It refuses any matching native draft at or above the selected version, unless that draft is the exact resumable SOURCE/version/candidate/inventory match.
- The test case is at lines 323–325. A read-token fixture hides a higher or equal draft, and the write-token authority must still refuse it before any tag write, except for an exact resumable match.
- GitHub's list-releases endpoint returns drafts only to callers with push access. That matches the r2 finding, and I did not re-check it independently.

**B2: anchored native tag filter, resolved**
- Lines 40–44 define matching native tags as exactly `^v0\.2\.[0-9]+(-rc\.[0-9]+)?$`.
- All non-matching tags and releases are ignored, and the two `v0.1.0-experimental.*` tags are named.
- Only inconsistent matching native state fails closed. Comparison is numeric, not lexical.
- The test case at lines 322–323 requires ignoring the existing 0.1 experimental and unrelated tags.
- The pattern's `-rc.N` form agrees with the stated `0.2.N` / `0.2.N-rc.N` version grammar.

**r2 non-blocking items**
- The `before` attestation caveat is stated, at lines 88–91.
- Candidate artifact expiry after 14 days is stated, at lines 254–256.
- The merge-from-master off-first-parent case is documented, at lines 65–67.
- Bootstrap construction by plumbing or Git Data REST, with no SOURCE checkout in the write job, is stated, at lines 260–266.

**`v0.2.3-publication.md`: approval stands**
- The "Operator coordination" section now covers the r2 wording items:
  - operator abstention during the publisher run (lines 97–105)
  - diagnostic ancestor limited to `DIAGNOSTIC_PATHS`, with the automation and publication docs entering master afterward
  - stop and escalate on a macOS failure that tooling cannot fix, with no bypass
  - stale status lines replaced with approval links before freeze
- Approval of this document is still conditional on the automation approval above, and on integrated code review before SOURCE is published.

The status line at line 3 of `native-automatic-release.md` and line 9 of `v0.2.3-publication.md` still reads "approval required". The new coordination section already requires updating both before freeze, so this is not a blocker. Record this review as `native-automatic-release-design-r3.md` when you write it up.
