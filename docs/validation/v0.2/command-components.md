# Isolated native command components

This increment implements only the independently approved
[test-only scheduling exception](../../reviews/v0.2-scheduling-design-r2.md) and
[configuration bounds](../../reviews/v0.2-config-bounds-design-r1.md). It does not
complete Phase 1 or Phase 2. Production binaries still provide the native
foundation interface, not remote command execution.

`command_wire.rs` and `command_config.rs` are declared only under `cfg(test)`.
There are no production call sites, new dependencies, engine/store/service
integration, policy compiler or command activation. Existing private profile
helpers have only their visibility widened to `pub(crate)`; their checks and
existing callers are unchanged.

## Validation

On the combined worktree based on `766e68cc44f68bc2e5a2488a1463ba211493ed29`:

| Check | Result |
| --- | --- |
| Full native library suite before review fixes | 137 passed, 0 failed; 207.73 seconds |
| `cargo test --offline --locked -p marmish-native --lib command_` after review fixes | 24 passed, 0 failed; 117 foundation tests filtered out |
| Focused command wire tests after review fixes | 11 passed |
| Focused command configuration tests after review fixes | 13 passed |
| `cargo clippy -p marmish-native --all-targets --locked -- -D warnings` | Passed |
| `cargo fmt --all --check` | Passed |
| `git diff --check` | Passed |

The codec tests include independent canonical JSON and the normative 174-byte
digest vector, duplicate/unknown fields, explicit nullable fields, all byte values
in stdin/output, non-NUL argument bytes, NUL refusal, envelope and poll limits,
and the closed operation/error variants. Configuration tests cover required
fields, duplicate aliases, normalized identities, exact relay ceilings, raw
additive policy layers, defaults and bounds, cwd behavior, and private regular
files versus unsafe ownership/mode, links and nonregular files. These are inert
local checks; no DNS, relay, Arti or native keychain operations are used by the
new tests.

`ripwire crates/marmish-native --quality-delta` exited 2. Its reachability model
reports test-only and Serde-dispatched symbols as unreachable, and conflates the
new wire `id_timestamp` with a same-named foundation helper. It also flags a small
private test-fixture overlap with identity tests and repeated finite operation
dispatch. No baseline, suppression or semantic change was used to hide these
findings. This is not a passing quality-tool result. The independent
[round-2 code review](../../reviews/v0.2-command-components-code-r2.md) assessed
the concrete findings and found no necessary code change or blocking quality issue.
The final run also flags verbosity for a shared `tests` module name as the
literal corpus grows; this is test data, not a production function. The final
review explicitly inspected that corpus addition. No suppression was added.

[Claude's first integrated review](../../reviews/v0.2-command-components-code-r1.md)
found four blocking issues. The fixes reject explicit null `max_commands`, remove
three unapproved structural limit relationships pending engine validation, require
absolute non-NUL executable-path details, and add independent literal response
fixtures and closed enum tables. Explicit null `allow_exec` is also refused,
preserving omission versus an explicit empty allow list. Absolute paths containing
valid Unix filename control bytes remain representable in canonical JSON; the
future CLI must escape remote diagnostic text on display. No foundation or
production execution behavior changed in these fixes. The second review approved
the integrated test-only scope and independently passed all 24 focused tests,
strict all-target Clippy, formatting and diff checks.
The useful follow-up nonempty poll fixture pins gap fields, signal outcome,
binary chunks and cursors to literal bytes. The
[final review](../../reviews/v0.2-command-components-code-r3.md) independently
checked that fixture, confirmed unchanged production logic and repeated all
24 focused tests, strict Clippy, formatting and diff checks successfully.

Hosted direct/Arti acceptance, authenticated command execution, durable command
recovery, installed interoperability and command latency/goodput remain separate
requirements. Passing codec tests does not establish any of them.
