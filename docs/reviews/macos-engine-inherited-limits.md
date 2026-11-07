# macOS inherited-limit diagnostic reviews

Scope: [diagnostic design](../phases/macos-engine-inherited-limits.md). This
standalone, manual, no-relay observation compares shell and GitHub Node 24
inherited limits. It does not qualify streaming or authorize a product correction.

Actual Claude Code `claude-sonnet-5-5`, `--permission-mode auto`, design session
`99fc3557-fb6b-49c2-9eee-eec22d0fd727` returned **APPROVE**, no blocking findings,
70.379 seconds / 8 turns. Private result SHA256
`7ae7052beb37d0ba5d1c9072d0b813acf482283c6255dc840ea116c3169063a1`. The reviewer inspected the actual
macOS engine gate and both launch chains, and made no edits, tests, network
requests or workflow dispatches.

Useful refinements were incorporated before implementation: pass shell observations
through step outputs into the action, removing temporary measurement files and
a separate comparison/cleanup step; retain exact decimal/infinity parsing and a
bounded synchronous child; describe the inheritance seam accurately rather than
claiming an identical spawn implementation; include constrained runner/image and
Node version provenance; distinguish the soft-limit gate from high descriptors
and other engine bootstrap failures. A single current sample cannot establish
historical runner state or the original engine error.

The coordinator recorded this approval before authorizing the bounded agent
implementation. Integrated code approval and hosted observations are recorded
below when completed; no approval or successful diagnostic run is inferred here.

## Integrated code review

Actual Claude Code AUTO session `b7506a24-f70f-40b5-ac6d-fd8eb15db3bc` returned **APPROVE**,
no blockers, 101.938 seconds / 9 turns. Private result
SHA256 `90bccea0b134e7a916ad3e30644e5c91bd68bd36daaf9faa21a7ad6deea8c6e8`. It inspected the complete
new workflow/action/tests, phase design, review record and specification diff;
the four separately approved evidence documents were checked for consistency,
not re-audited against raw inputs.

It independently passed six focused tests on local Node 22.22.0, JavaScript
syntax, YAML parsing and diff checks, and exercised fixed failure categories.
It reproduced a useful symlink-entry defect: the combined library/entry guard
could exit zero without a summary. A dedicated unconditional action entry point
was added and received corrective review below. Hosted Node 24, Darwin shell
behavior, actual limits and image variables remain unobserved until dispatch.

Author and coordinator focused tests passed six cases, syntax/YAML/diff checks.
Ripwire quality exited 2 on JSON evidence-field verbosity: a new artifact-metadata
field was conflated with a prior evidence document, and the new fixed projections
are necessarily verbose. No product code changed and no green static-quality
result is claimed. The staged change-context pass covered all eleven files;
its graph did not resolve the new JavaScript test imports, so the actual Node
test execution remains the relevant test evidence. Existing ignore rules cover
identities, keys, databases/sidecars, logs, temporary state and build artifacts;
representative ignore checks passed.

### Corrective integrated review

The same actual Claude AUTO session returned **APPROVE**, no blockers,
20.196 seconds / 3 turns. Private result SHA256
`efb9ca9ecb2ac5b09f4bba602b576613a336ecfbe21281630829db23e3517702`. It verified the unconditional
entry point, removal of the library guard, static policy update and actual
symlink subprocess regression. Independently passed seven tests, three syntax
checks, both YAML parses and staged/unstaged diff checks. It did not repeat the
unchanged workflow diff or previous evidence audits. Author seven tests passed
in 68.47 ms; the coordinator independently passed them in 64.17 ms. These are
local Node 22 results, not hosted Node 24 or macOS observations. All first-run
unknowns remain. The new entry file is explicitly staged for publication.

## Hosted evidence review

The [actual hosted observation](../validation/macos-inherited-limits-fa71302.md)
is recorded separately from code approval and is explicitly nonqualifying.
Actual Claude Code AUTO `claude-sonnet-5-5` session
`439e85d0-d336-46cb-9ab4-ac8e508b859b` approved the retained evidence in two
rounds: 61.867 seconds / 9 turns and 29.110 seconds / 5 turns. Private result
SHA256 values are
`3338c4dc43b2556a3da3fbd0d425d11b465e07c17aa611af6b845236a76b558f` and
`ee52b65546cbe3e96313550d5504e25f47b8bb80979e4b59be220336b3ff7154`.
The evidence records precise verification scope and remaining limits. Useful
wording clarifies that both readings use child shells, the infinity normalization,
the conditional guard inference and matching recorded image fields. Neither
round establishes the original discarded engine error or product correction.
