# Private recipient candidate build and local reads design review

Claude Code 2.1.301 reviewed the design in Auto mode against the installed
recipient daemon plan and composed source `2c18a7d`. Both reviews inspected
source only.

| Round | Reviewed design commit | Claude session | Verdict |
| --- | --- | --- | --- |
| R1 | `6508a0791a0eb48c98a07eb1ae525787d04d39bc` | `ffb0e8a3-f9d3-4632-801d-57b1b9afe0dd` | Request changes |
| R2 | `15db742` | `901df3fc-1de8-439d-9c24-b4c14516b91d` | Approve |

R2 accepted the closed read-only candidate wire, shared same-UID listener
and readiness/drain lifecycle, including ordinary accept retry and
Peer-only handling of registration failure. The candidate packaging
classification stays separate from public release and installer inputs.
Actual Pending remains part of the composed charged-owner milestone.

Apply these implementation checks from the approval:

- Verify the non-test candidate cannot use Approve or Decline request and
  reply variants, `State::Activating`, `Witness`, `Work::begin_manual` or
  `Manual::authorize`. Verify the projected entry excludes internal
  `qualified_at` and `activating_at` fields.
- Exercise the shared foreign-UID refusal predicate and the same-UID
  positive path on the actual listener. Report the predicate evidence as
  such if the harness does not spawn a foreign-UID process.
- Set the pair to Stopped on registration failure and verify stale intake
  cannot queue. Verify the one-shot Ready transition with and without a
  startup reporter, including restart.
- Check both binaries' command parsing around the unqualified token,
  explicit separators, `npub@relay` and rsync.

No runtime code or build was reviewed in these design passes.
