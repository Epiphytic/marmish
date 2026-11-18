# Upstream MDK master code review

Claude Code with `claude-sonnet-5-5` reviewed the isolated native MDK
refresh in `--permission-mode auto` against base
`e74f621face7171ad0705809cbf0184ac417cb57`.

| Round | Session | Result |
| --- | --- | --- |
| R1 | `c0038052-4021-4eea-9669-a7a41d37d0c5` | Static approval conditional on first test run, superseded |
| R2 | `c015d55a-884e-4ad5-8f43-a0b23eff9d19` | Approved code, publication conditional on clean author suite |
| Read-only preflight follow-up | `882a7046-cafd-426b-8f44-fb08ce42e006` | Approved migration ceiling correction |

R2 inspected the exact source against upstream MDK `0ad3c610`. It confirmed
that missing Welcome provenance stays closed, group recovery leaves a scoped
Terminal fence unfinished, and a locally paused publication reports an actual
pre-send refusal. It checked that direct Nostr dependencies match the commit
used by MDK and that Command, Copy and Stream keep their existing paths.
Claude independently ran the immutable test executable. Seven Welcome cases
and one paused-publication case passed, in 115 and 15.6 seconds respectively.

The exact Cargo.lock revision preserves reproducibility with `--locked`.
Cargo.toml follows upstream `master`, as requested. A scratch review receipt
and Beads export were outside the delivery scope. Prior proof and design
references to MDK `8abc51b` record the revisions in use when those checks were
run. GroupRecovery is an authenticated diagnostic, with no new recovery
workflow or effect driven by it in this change.

The follow-up checked MDK's actual default migration registry and its
future-format rejection against Marmish's read-only preflight. Both checks
reject versions above 212 for the locked revision. The author reran the
encrypted WAL restart and packaging cases on an immutable build with valid
source metadata.

[Author validation](../validation/upstream-mdk-master.md)
