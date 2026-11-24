# Marmish code ontology

The graph records tracked files, declarations, lexical containment, and
syntactically identifiable imports and calls. It covers Rust, Python,
JavaScript, shell, and C in the runtime, developer tooling, and tests.
Build and dependency manifests, workflow definitions, fixtures, and patches
are tracked by content hash. The file inventory reports parser recovery.
A reference records the spelling at a source location. Resolving the
callee, expanding a macro, and evaluating conditional compilation require
separate analysis.

## Update after editing tracked inputs

Stage the intended code, configuration, or release changes first. The tool
reads the staged Git tree, including file modes and exact Git blob bytes.
After every further edit to those inputs, stage and repeat the update.

```sh
uv run --locked --project tools/code-ontology python tools/code-ontology/index.py update
git add docs/ontology/*.jsonl docs/ontology/*-model.yaml docs/ontology/auto-ontology.yaml
uv run --locked --project tools/code-ontology python tools/code-ontology/index.py check
```

Use `check --ref HEAD` to verify a committed snapshot. The CI check compares
generated outputs with the same type of immutable Git tree. Release
workflows check the exact candidate revision, including manual recovery.
Candidates created before the graph was introduced must be rebuilt from a
fresh, reviewed revision before publication.

The public installer is generated from its tracked shell template.
The graph indexes `scripts/install-native.sh` and the versioned Cargo
manifests. The generated `scripts/install.sh` is checked separately. Its
full content must match the native template from the selected Git release
tag with exactly four substitutions for the version and platform digests.
This lets publication advance the generated installer while still rejecting
unaccounted-for installer edits.

Run `sh tools/code-ontology/test.sh` to test generation, installer
verification, and validation against NVIDIA's pinned model schema.

## Query the graph

The JSON Lines files can be queried directly. Each reference file row groups
references by path. Tuples contain the scope ordinal, kind, target spelling,
and source line. Ordinal zero denotes file scope. Positive ordinals are
one-based positions among the symbol rows for that file.

The SQLite export builds the
same graph with primary and foreign keys, including symbol ownership and
reference scope.

```sh
uv run --locked --project tools/code-ontology python tools/code-ontology/index.py sqlite --ref HEAD /tmp/marmish.sqlite
sqlite3 /tmp/marmish.sqlite \
  "SELECT name, path, start_line FROM symbols WHERE name LIKE '%relay%' LIMIT 20;"
```

Generate the database with the basename `marmish.sqlite`. The NVIDIA SQLite
connector derives the catalog name from that filename and uses the `main`
schema. In an Auto Ontology instance you operate, add this SQLite file as a
dedicated source and import [the native semantic model](auto-ontology.yaml)
with `replace=false` and `embed=false`. Mount the file where the server can
read it if it runs in a container. Use a fresh, dedicated source for changed
snapshots. This command-line tool only creates local files. The model and
graph need no network credentials to generate once the locked dependencies
are installed.

The native model is generated from [the Apache Ossie semantic model](semantic-model.yaml)
using the converter pinned by [NVIDIA Auto Ontology](https://github.com/NVIDIA/auto-ontology/blob/a12fec75226a54ffc91752cca9538f1f01f760eb/pyproject.toml).
The [phase design](../phases/code-ontology.md) records exact pins and coverage.
