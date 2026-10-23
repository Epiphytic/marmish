# Additive command restrictions

Beads: `marmish-b33.1`. The user replaced override/last-entry-wins with
additive restrictions. This increment precedes native macOS support.

## Contract and boundaries

`marmishd --command-policy FILE` reads strict JSON before starting MDK and passes
it through the trusted bridge to the Rust engine. Identity admission still
requires `--allow`: a policy cannot grant identity access. No policy preserves
existing authenticated execution behavior.

Schema: `version: 1`, optional `defaults` restriction object, optional `keys`
list of `{principal, restrictions}` objects. Frontend principals accept npub or
hex and become canonical hex on the bridge. Duplicate key entries accumulate.
Reject unknown fields, duplicate JSON object keys, invalid keys/globs/versions,
files over 64 KiB, over 128 entries, over 256 total patterns or patterns over
1024 UTF-8 bytes. Repeat structural validation at the engine boundary.

Concrete interfaces: `EnginePolicy.command_policy: Option<CommandPolicy>` has
`#[serde(default)]`; bridge `Init.command_policy` is optional and defaulted.
`CommandPolicy {version: u32, defaults: Restrictions, keys: Vec<KeyRestrictions>}`,
`KeyRestrictions {principal: PrincipalId, restrictions: Restrictions}` and
`Restrictions {allow_exec: Option<Vec<String>>, deny_exec: Vec<String>}` are
strict serde types in the engine policy module, with defaults for optional
fields. The bridge converts canonical hex principal strings into typed IDs.
Engine validation rejects principals absent from allowed_principals. Bounds
constants live with these types. New accepted jobs persist
`canonical_exec_required: bool` (serde default false), copied into WorkerClaim
and ChildSpec. Persisted Config.version becomes 2; new binaries accept/migrate
version 1, while existing old binaries reject version 2 before claiming jobs.
SQL schema user_version stays 1: only versioned JSON config/job data changes.
New worker launch must fail closed against malformed persisted policy/markers.

A restriction object has optional `allow_exec` and `deny_exec` pattern lists.
An absent allow adds no constraint; an empty allow denies all. At least one
pattern must match in **every** applicable allow layer. A match in **any** deny
layer denies. Defaults and every entry for the authenticated principal apply
together, independent of order. Keys absent from the file still get defaults.

Patterns are absolute, anchored, case-sensitive byte matches. `*` matches zero
or more non-`/` bytes; `**` matches zero or more bytes including `/`. Backslash
escapes a following star or backslash; dangling/other escapes are invalid.
Other characters, including question marks, brackets and braces, are literal.
This implements the requested two wildcards, not every GitHub glob extension.
Use bounded non-exponential dynamic programming.

Require a literal leading slash. Reject NUL, control characters and invalid
Unicode in configuration. Runs of two or more stars mean one double star.
Slashes remain literal: `/usr/**/echo` does not match `/usr/echo`, nor does
`/**/sh` match `/sh`. Non-UTF-8 executable bytes can match wildcards but cannot
be named literally by JSON. The caps above count key-list entries, all patterns
across defaults and keys, and decoded UTF-8 bytes respectively. A shared fuel
budget of 1,000,000 state transitions per policy evaluation bounds denial cost;
exhaustion denies with the same category. Pure matching runs at most twice on
new admission, never at worker launch. Test adversarial budget exhaustion.

An effective allow list or nonempty deny list requires an absolute, already
canonical executable path: refuse PATH names, relative paths, dot/traversal,
duplicate separators and symlink aliases. Resolve and compare bytes before
matching; repeat canonical-path validation at launch. This requires canonical
spellings such as `/usr/bin/echo` where `/bin` is a symlink. Original argv and
digest stay unchanged, preserving retry identity and arbitrary argument bytes.
Empty ineffective layers add no restriction; unrestricted execution keeps its
existing semantics. Restricted argv[0] is at most 4095 bytes. The persisted job
marker enables the launch check only for accepted restricted jobs. The worker
checks only realpath equality, never current policy patterns. Check failure
returns Exited(NotStarted) through ChildSpawnError::NotStarted, with no retry.

Restrictive policies reject shell-string mode even with `--allow-shell`. Only
argv[0] is matched, not arguments or joined shell text. Allowing interpreters,
shells, env, writable scripts, compilers or programs with execution features
grants their capabilities. This is executable selection, not a behavior or
filesystem sandbox. Same-user mutation of executables is outside the existing
same-user trust boundary; do not claim inode/content pinning.

## Admission and recovery

Engine::submit checks new requests after existing() and before token consumption,
job directories or durable admission. Store::admit checks after existing() and
repeats only pure lexical/matcher/shell enforcement; no filesystem operations
occur inside its SQLite write transaction. Engine performs the filesystem
canonical check before entering the store; worker repeats it at launch. Exact
already-admitted retries retain deduplication semantics after policy tightening;
conflicts still fail. Status/wait/cancel are ownership operations. No policy
data comes from Marmot payloads; errors are fixed categories without command data.

Policy is immutable until restart. Accepted/running jobs keep their accepted
authority and survive disconnect or policy changes. Claiming old accepted jobs
must not reapply current shell permission: valid persisted requests carry their
accepted authority, with the job-specific canonical marker default false.
Never normalize the request or change its digest to prepare execution.

The frontend reads cap+1 bytes from a no-follow regular file owned by its user
without group/world write permissions. Reject invalid UTF-8/BOM, non-finite
numbers, booleans as version numbers, nested duplicate keys and excessive JSON
nesting without printing content. Policy denials (including not found or
noncanonical paths) map to one fixed `command_denied` engine/bridge category.
Executable spelling restrictions do not prevent copying/hardlinking binaries or
mutation by another principal controlling a writable executable/directory;
administrators must allow only trusted programs/paths. No sandbox claim.

## Implementation and acceptance

One agent owns Rust policy/matching/admission/store/launcher and Rust tests;
another owns Python configuration/CLI, bridge integration and frontend tests.
Agree the wire schema before editing; integration/docs remain central.

Required checks: defaults across principals; multiple additive entries; absent
versus empty allow; deny precedence/order independence; slash-sensitive star
versus double star; escapes and literal punctuation/non-UTF-8 bytes; strict
malformed/oversized input; PATH/relative/traversal/symlink refusal; shell refusal;
unchanged literal arguments; denials leave tokens/jobs untouched; duplicate,
conflict and restart behavior. Run workspace tests, clippy/format and Python
checks. Extend actual Marmot E2E with allowed execution and default/per-key
denials without side effects. Record sanitized results, obtain Claude auto-mode
code approval, incorporate useful findings, update usage/spec and commit/push.
