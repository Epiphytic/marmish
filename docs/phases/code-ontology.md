# Code ontology and freshness enforcement

Beads issue `marmish-am1`.

Marmish will maintain a complete indexed source inventory and a queryable code
graph using NVIDIA Auto Ontology's native model interchange. NVIDIA's framework
accepts structured data and semantic models. The repository indexer will
extract the source structure and provide those inputs without needing a model
API key or a deployed service for every commit.

## Phase 1 — Generate and query the graph

Build a deterministic, local index of Git-tracked first-party source, tests,
scripts, build and dependency manifests, patches, CI definitions, release
metadata and install tooling. Use pinned Tree-sitter grammars for Rust,
Python, JavaScript, shell and C. Index files, named declarations and their
lexical relationships, plus source import and call references with file and
line evidence. References are syntactic and are not claims of name or
runtime dispatch resolution. Keep file hashes for tracked code/config/release
inputs that have no parser, and fail on unknown extensions in code roots.
Exclude graph outputs, Beads data, local untracked files and historical prose
or validation artifacts. Do not copy source bodies or runtime data into the
graph. Use repository-relative IDs and deterministic serialization.

Check in the graph and native Auto Ontology model. Provide a local command to
materialize the graph as a SQLite source with real keys and foreign keys.
Generate or validate the semantic interchange through the NVIDIA-specified
Apache Ossie converter revision and check representative SQL joins against
the database. The Rust packages and installer gain no dependency. Import is
opt-in against a separately operated Auto Ontology instance.

## Phase 2 — Enforce updates

Provide one update command and one read-only check. The check regenerates and
compares the canonical graph and model, and fails on a changed, added or
removed tracked input or output drift. Document the same-commit requirement
for agents, including version and release metadata changes. Add an isolated
read-only PR/push check on the default branch and release tags. The release jobs enforce
this gate before packaging and publication. This phase leaves GitHub branch
settings alone because the existing publisher explicitly refuses protected master
and performs a direct fast-forward after validating an exact one-file installer
cutover. Direct ref writes by anyone with repository write access remain outside
Actions enforcement. Keep arbitrary public PR code off the
shared self-hosted fleet. Existing release jobs keep their runner and
provenance rules. The native candidate and publication workflows must check
the exact candidate Git tree, including manual recovery of candidate artifacts;
do not execute historical candidate scripts from a publication context. The
active public installer `scripts/install.sh` is a rendered release artifact.
It is excluded from the graph, file inventory and input hash set. It is
classified separately and covered by the tag-bound byte comparison. Index
its entire source template `scripts/install-native.sh`. Separately
verify the active artifact at every check by resolving its literal version
to the matching local `refs/tags/v<version>` source commit and comparing
the full bytes against that commit's template with the four renderer
substitutions. Accept only the exact generated assignment shapes.
The signed asset pins remain the publisher's separate digest contract.
This accounts for old active installer code during development and new
installer code after automatic cutover, without leaving stale generated
ontology files in the publisher's one-file fast-forward. Use Git tag
objects from the same checkout, and fail with a fetch hint when the
required tag is missing. Gate the legacy experimental publication path
as well. Tests must show failure on stale code and release metadata, and
success after regeneration. This change publishes source only, without
cutting an installer or binary release.

## Data and tooling contract

Run the development tool with Python 3.12 and a checked-in uv lock. Lock
Tree-sitter and the five grammar packages (Rust, Python, JavaScript, Bash, C)
to precise versions and registry artifact hashes. Pin NVIDIA Auto Ontology
at `a12fec75226a54ffc91752cca9538f1f01f760eb` for the interchange contract
and its Apache Ossie converter at
`cb4b46b97bcdb9f2685ef2f7023b9134a4426da2`. Lock converter Python
dependencies as well. CI installs these tools in an isolated environment via
a commit-pinned uv setup action. They do not enter product packages or builds.

Take both content and file modes from the Git index tree (developers stage
inputs first) or an immutable commit/tree supplied by CI. Read Git objects
without checking out or executing the inspected revision. Evaluate rules in
order: graph outputs and documentation under the dedicated ontology directory
are separate; Beads, historical prose and validation evidence are excluded;
all recognised source languages in any directory are parsed; ordinary
tracked files in `crates/`, `tools/`, `scripts/`, `tests/`,
`patches/` and `.github/` are inventory inputs if their extension is
declared. Explicit root build metadata, Cargo lockfiles, release notes,
agent instructions and Git settings are also inventory inputs. Reject an
unclassified path or an unsupported Git entry instead of silently skipping
it. Scriptless manifests, fixtures and patches are inventory-only. Report
known parser recovery in the inventory so coverage is auditable.

Model one SQLite file named `marmish.sqlite`, schema `main`. Each graph
table has one semantic Term with named attributes for its columns, a
documented primary key, and foreign keys for lexical scope and ownership.
Use distinct relationships only where one ordered physical table pair
exists. Preserve role-specific keys in data where native import cannot
represent multiple joins on the same pair. Stable symbol keys are
repository-relative file path, lexical scope, declaration kind and name,
plus a repeat ordinal within that exact group. Source lines are attributes.
Store deterministic UTF-8 LF JSONL with a fixed schema, sorted inputs and
file-grouped references to avoid repeating paths and long lexical keys. Use
stable row order. Write the small Ossie semantic model and round-trip it via
the pinned converter to the native model and through NVIDIA's native schema
in validation. Store metadata as names, locations, modes and hashes. Do not
store contents or arguments as reference literals.

Materialize the database locally, and only import into an approved separate
Auto Ontology connection using `replace=false&embed=false`. Public CI never
requests a model, exports secrets or modifies such a connection. Validate
coverage, referential integrity and representative SQL, test deterministic
freshness on staged additions/deletions/code and release inputs, compare two
different hash seeds and run native schema validation. Coverage on any
platform remains a separate claim until tested. Check GitHub workflow
permissions and trigger trust with fixture tests.

## Release enforcement details

Eligibility uses the workflow revision (`github.sha`) as the tool source
and checks the requested immutable candidate commit before making the
eligibility decision. The publisher reads the candidate commit from its
validated, run-bound inspection metadata and checks it with the trusted
publisher tree in a read-only job before write credentials are available.
A candidate with no ontology is rejected, including historical recoveries.
The legacy experimental workflow checks the single captured workflow SHA.
CI for public PRs uses a short isolated hosted job as existing public PR
workflows do. It runs on every PR and merge result, with read-only contents
and without credentials persisted in its checkout. The project leaves
shared self-hosted qualification scheduling unchanged.
