#!/bin/sh
set -eu

schema_dir=$(mktemp -d)
schema="$schema_dir/schemas.py"
trap 'rm -f "$schema"; rmdir "$schema_dir"' EXIT
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
    --max-time 30 \
    'https://raw.githubusercontent.com/NVIDIA/auto-ontology/a12fec75226a54ffc91752cca9538f1f01f760eb/auto_ontology/server/model_interchange/schemas.py' \
    --output "$schema"
expected=54e3d56e3ffb1178d5b4bfc2872fce761ede8461c3d8ea0a753f7a0fa7ecfc39
actual=$(shasum -a 256 "$schema")
test "${actual%% *}" = "$expected"
MARMISH_AUTO_ONTOLOGY_SCHEMA="$schema" \
    uv run --locked --python 3.12 --project tools/code-ontology python -m unittest discover -s tools/code-ontology
