"""Tests for the code graph's database and interchange contracts."""

import json
import os
import runpy
import sqlite3
import subprocess
import sys
import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

import yaml
from model import TABLES, materialize, native_model, semantic_model


class ModelTest(unittest.TestCase):
    def setUp(self):
        source_path = "crates/core/src/main.rs"
        self.rows = {
            "source_files": [
                dict(
                    zip(
                        TABLES["source_files"],
                        [source_path, "rust", "100644", "a" * 64, "parsed"],
                        strict=True,
                    )
                )
            ],
            "symbols": [
                dict(
                    zip(
                        TABLES["symbols"],
                        [
                            "function",
                            source_path,
                            "module",
                            "function",
                            "connect",
                            8,
                            10,
                        ],
                        strict=True,
                    )
                ),
                dict(
                    zip(
                        TABLES["symbols"],
                        ["module", source_path, None, "module", "transport", 1, 11],
                        strict=True,
                    )
                ),
            ],
            "code_references": [
                dict(
                    zip(
                        TABLES["code_references"],
                        ["import", source_path, None, "import", "mdk", 1],
                        strict=True,
                    )
                ),
                dict(
                    zip(
                        TABLES["code_references"],
                        ["call", source_path, "function", "call", "Client::connect", 9],
                        strict=True,
                    )
                ),
            ],
        }

    def test_sqlite_joins_and_foreign_key_validation(self):
        with TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "marmish.sqlite"
            materialize(path, self.rows)
            database = sqlite3.connect(path)
            try:
                actual = database.execute(
                    """SELECT source_files.language, module.name, symbols.name,
                              code_references.target, code_references.line
                         FROM code_references
                         JOIN source_files USING (path)
                         LEFT JOIN symbols ON symbols.id = code_references.scope_id
                         LEFT JOIN symbols module ON module.id = symbols.parent_id
                        ORDER BY code_references.line"""
                ).fetchall()
                self.assertEqual(
                    actual,
                    [
                        ("rust", None, None, "mdk", 1),
                        ("rust", "transport", "connect", "Client::connect", 9),
                    ],
                )
                self.assertEqual(
                    database.execute("PRAGMA foreign_key_check").fetchall(), []
                )
            finally:
                database.close()

            snapshot = path.read_bytes()
            self.rows["code_references"][1]["scope_id"] = "missing"
            with self.assertRaises(sqlite3.IntegrityError):
                materialize(path, self.rows)
            self.assertEqual(path.read_bytes(), snapshot)
            self.assertEqual(list(path.parent.iterdir()), [path])

    def test_interchange_has_same_tables_and_unique_join_pairs(self):
        semantic = json.loads(semantic_model())
        self.assertEqual(
            {
                dataset["name"]: tuple(field["name"] for field in dataset["fields"])
                for dataset in semantic["datasets"]
            },
            TABLES,
        )
        native = yaml.safe_load(native_model())
        (database,) = native["data_layer"]["databases"]
        (schema,) = database["schemas"]
        self.assertEqual((schema["database_name"], schema["name"]), ("marmish", "main"))
        self.assertEqual({table["name"] for table in schema["tables"]}, set(TABLES))
        self.assertEqual(
            {term["name"] for term in native["semantic_layer"]["terms"]}, set(TABLES)
        )
        pairs = [
            (join["source_table_id"], join["target_table_id"])
            for join in native["data_layer"]["joins"]
        ]
        self.assertEqual(len(pairs), 4)
        self.assertEqual(len(pairs), len(set(pairs)))

    def test_native_conversion_determinism_across_interpreters(self):
        snapshots = [
            subprocess.check_output(
                [
                    sys.executable,
                    "-c",
                    "import sys, model; sys.stdout.buffer.write(model.native_model())",
                ],
                cwd=Path(__file__).parent,
                env={**os.environ, "PYTHONHASHSEED": seed},
            )
            for seed in ("17", "89")
        ]
        self.assertEqual(snapshots, [native_model(), native_model()])

    @unittest.skipUnless(
        os.environ.get("MARMISH_AUTO_ONTOLOGY_SCHEMA"), "upstream schema path required"
    )
    def test_nvidia_native_schema(self):
        schema = runpy.run_path(os.environ["MARMISH_AUTO_ONTOLOGY_SCHEMA"])
        document = schema["AutoOntologyModelDocument"].model_validate(
            yaml.safe_load(native_model())
        )
        self.assertEqual(len(document.semantic_layer.terms), len(TABLES))


if __name__ == "__main__":
    unittest.main()
