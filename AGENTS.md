# Working on Marmish

Read [the agent specification](docs/agent-spec.md) before planning or changing
behavior. It records what works, intended capabilities, design decisions and
known limits. Consult [the build plan](docs/build-plan.md) for phase boundaries,
[the README](README.md) for usage, and Beads for current task status.

## Phased build workflow

The user has authorized phased implementation using agents, independent Claude
reviews, and committing and pushing each completed, reviewed phase or increment.
Continue through implementation and validation; a plan alone does not complete
an implementation request. Existing authorization carries across turns unless
the user changes it.

1. **Track the work.** Use Beads (`bd`) for tasks, dependencies, blockers, review
   dispositions and handoffs. Claim the task before implementation. Use
   `bd prime` when context is missing. Keep durable project memory in Beads;
   do not create parallel Markdown task lists or ad hoc memory files.
2. **Review the overall plan.** For a new initiative or material change in scope,
   write or update the high-level phased build plan and review it with Claude.
   Define phase deliverables, dependencies, boundaries and exit criteria.
3. **Design each phase before coding.** At the start of each phase, write its
   detailed plan under `docs/phases/`. Specify interfaces, authenticated trust
   boundaries, persistence/recovery behavior, limits, test cases and acceptance
   criteria. Inspect the actual code and dependency contracts rather than
   assuming support. Obtain Claude's independent design review, incorporate
   useful findings and repeat until blocking design findings are resolved.
   Record the approval before implementation starts.
4. **Implement with agents.** Delegate bounded components to agents with explicit
   interfaces and disjoint file ownership. Coordinate integration centrally;
   avoid overwriting another agent's work. Use isolated worktrees when branches
   or task isolation require them. Preserve unrelated user changes and sibling
   repository work.
5. **Validate the result.** Run checks appropriate to the change and the phase's
   acceptance criteria. For transport changes, use actual client/server command
   execution through Marmot, not just mocks or successful relay publication.
   Include relevant negative, byte-preservation, replay, retry and recovery
   checks. Use disposable keys, explicit relay selection and bounded workloads;
   clean up owned test processes and private state. Keep sanitized evidence
   under `docs/validation/` and report failures and limitations honestly.
6. **Review the completed code with Claude.** Use Claude Code in
   `--permission-mode auto`, permitting repository inspection and local tests.
   The review must inspect the integrated result, including new files and tests,
   against the approved design. Give the reviewer precise scope and prevent
   conflicting live tests or unrelated edits. Store review evidence under
   `docs/reviews/`, including reviewer/model, reviewed scope, verdict, findings
   and validation. Incorporate useful changes, run affected checks and obtain
   another review after substantive fixes. No phase closes with unresolved
   blocking findings. If Claude or a required dependency is unavailable, record
   the specific blocker rather than inventing approval or silently substituting
   another reviewer.
7. **Publish each completed phase or increment.** Update specifications and usage
   to match the implementation. Audit `.gitignore` and the exact staged diff;
   exclude identities, keys, runtime databases, command journals, output,
   temporary files and logs. Stage intended source and sanitized documentation
   explicitly. Commit and push to the configured project remote, verify the
   push, and record the commit, branch, review and test evidence in Beads before
   closing the task. Do not rewrite history to fabricate earlier phase commits.

Scale the plan and review to the change. Small documentation corrections do not
require inventing a new implementation phase. Deployment, release publication
and third-party messaging require their own authorization; commit/push authority
does not imply those actions.

## Keeping the specification accurate

Update `docs/agent-spec.md` when a phase changes supported behavior, an important
decision or a known limit. Distinguish implemented and tested behavior from
intent. Link concrete evidence instead of inferring support from an API name or
fresh encryption keys. Beads remains the task tracker; the specification is the
behavioral and architectural handoff, not a second queue.
