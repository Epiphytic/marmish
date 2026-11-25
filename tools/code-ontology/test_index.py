"""Exercise the ontology checker against real, temporary Git trees."""

import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from index import INSTALLER_BINDINGS, installer_values

TOOL = Path(__file__).with_name("index.py").resolve()


class Freshness(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name)
        self.git("init", "-q")
        self.write("crates/example.rs", "pub fn send() { relay::publish(); }\n")
        self.write("scripts/setup.sh", "main() { printf hello; }\n")
        self.write("tools/peer.py", "from network import connect\n\ndef open_peer():\n    connect()\n")
        self.write("tools/candidate.mjs", "import fs from 'node:fs';\nfunction verify() { fs.statSync(); }\n")
        self.write("tests/probe.c", '#include <stdio.h>\nint probe(void) { return puts("data"); }\n')
        self.write("Cargo.toml", '[package]\nversion = "0.3.0"\n')
        self.git("add", ".")

    def git(self, *args):
        return subprocess.check_output(["git", *args], cwd=self.repo, text=True).strip()

    def write(self, path, data):
        path = self.repo / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(data, encoding="utf-8")

    def call(self, *args, expected=0, seed="17", cwd=None):
        result = subprocess.run(
            [sys.executable, TOOL, *args], check=False, cwd=cwd or self.repo, text=True,
            capture_output=True, env={**os.environ, "PYTHONHASHSEED": seed},
        )
        self.assertEqual(result.returncode, expected, result.stderr)
        return result

    def update(self):
        self.call("update")
        self.git("add", "docs/ontology")
        self.call("check")

    def test_stage_code_release_and_delete_changes(self):
        self.assertIn("missing or stale graph files", self.call("check", expected=1).stderr)
        self.update()
        previous_tree = self.git("write-tree")
        self.write("crates/example.rs", "pub fn send() { relay::enqueue(); }\n")
        self.call("check")  # The working-tree edit does not change the staged snapshot.
        self.git("add", "crates/example.rs")
        self.assertIn("code_references.jsonl", self.call("check", expected=1).stderr)
        self.call("check", "--ref", previous_tree)
        self.update()
        self.write("Cargo.toml", '[package]\nversion = "0.3.1"\n')
        self.git("add", "Cargo.toml")
        self.assertIn("source_files.jsonl", self.call("check", expected=1).stderr)
        self.update()
        self.write("tools/added.py", "def added(): pass\n")
        self.git("add", "tools/added.py")
        self.assertIn("symbols.jsonl", self.call("check", expected=1).stderr)
        self.update()
        (self.repo / "crates/example.rs").unlink()
        self.git("add", "-u", "crates/example.rs")
        self.assertIn("symbols.jsonl", self.call("check", expected=1).stderr)
        self.update()
        self.write("tools/unclassified.xyz", "unknown\n")
        self.git("add", "tools/unclassified.xyz")
        self.assertIn("unclassified tracked file", self.call("check", expected=1).stderr)

    def test_repeatable_output_and_reference_coverage(self):
        self.update()
        self.call("check", "--ref", self.git("write-tree"), seed="89")
        graph = self.repo / "docs/ontology"
        references = [
            item
            for line in (graph / "code_references.jsonl").read_text().splitlines()
            for item in json.loads(line)["references"]
        ]
        self.assertTrue({
            ("call", "relay::publish"), ("call", "printf"), ("import", "network"),
            ("call", "connect"), ("import", "node:fs"), ("call", "fs.statSync"),
            ("import", "stdio.h"), ("call", "puts"),
        }.issubset({(kind, target) for _, kind, target, _ in references}))
        self.assertTrue(all(line > 0 for _, _, _, line in references))
        self.assertNotIn("data", {target for _, _, target, _ in references})

    def test_sqlite_export_from_an_actual_index_and_subdirectory(self):
        self.call("update", cwd=self.repo / "tools")
        self.git("add", "docs/ontology")
        database_path = self.repo / "marmish.sqlite"
        self.call("sqlite", database_path)
        with sqlite3.connect(database_path) as database:
            self.assertEqual(database.execute("PRAGMA foreign_key_check").fetchall(), [])
            self.assertEqual(database.execute("""
                SELECT source_files.language, symbols.name, code_references.target
                FROM code_references
                JOIN symbols ON symbols.id = code_references.scope_id
                JOIN source_files USING (path)
                WHERE code_references.target = 'relay::publish'
            """).fetchall(), [("rust", "send", "relay::publish")])

    def test_literal_receivers_are_never_reference_targets(self):
        self.write("tools/secret.py", "def run():\n    'private label'.join([])\n    0xdeadbeef.bit_length()\n    handlers[509]()\n")
        self.write("tools/secret.mjs", "function run() { return 'private label'.trim(); }\n")
        self.write("crates/secret.rs", "pub fn run() { 'q'.is_ascii(); 5u8.pow(2); }\n")
        self.write("scripts/secret.sh", "run() { 'private label' --flag; }\n")
        self.git("add", "tools", "crates", "scripts")
        self.update()
        projection = (self.repo / "docs/ontology/code_references.jsonl").read_text()
        self.assertNotIn("private label", projection)
        self.assertNotIn("is_ascii", projection)
        self.assertNotIn("bit_length", projection)
        self.assertNotIn("handlers[509]", projection)
        self.assertNotIn("5u8", projection)

    def test_inventory_audits_parser_recovery_and_omissions(self):
        self.write("crates/recovered.rs", "pub fn run() {}\nfn {\n")
        self.write("tools/omitted.mjs", "const {label} = options;\n")
        self.git("add", "crates", "tools")
        self.update()
        inventory = self.repo / "docs/ontology/source_files.jsonl"
        statuses = {
            row["path"]: row["parse_status"]
            for line in inventory.read_text().splitlines()
            if (row := json.loads(line))
        }
        self.assertEqual(statuses["crates/recovered.rs"], "recovered")
        self.assertEqual(statuses["tools/omitted.mjs"], "parsed_with_omissions")

    def test_installer_rolls_forward_from_the_tagged_template(self):
        install_template = "scripts/install-native.sh"
        public_installer = "scripts/install.sh"
        body = "    printf new\n"
        template = "main() {\n" + "".join(
            f"    {key.decode()}='{token.decode()}'\n"
            for key, (_, token) in INSTALLER_BINDINGS.items()
        )
        self.write(install_template, template + "    printf old\n}\n")
        self.git("add", install_template)
        self.git("-c", "user.email=test@example.test", "-c", "user.name=Test", "commit", "-qm", "old release")
        self.git("tag", "v0.3.0")

        def installer(version, command):
            result = template + command + "}\n"
            values = [version, "a" * 64, "b" * 64, "c" * 64]
            for (_, token), value in zip(INSTALLER_BINDINGS.values(), values, strict=True):
                result = result.replace(token.decode(), value)
            self.write(public_installer, result)
            self.git("add", "--chmod=+x", public_installer)

        installer("0.3.0", "    printf old\n")
        self.write(install_template, template + body + "}\n")
        self.git("add", install_template)
        self.update()
        self.git("-c", "user.email=test@example.test", "-c", "user.name=Test", "commit", "-qm", "new release")
        self.git("tag", "v0.3.1-rc.1")
        installer("0.3.1-rc.1", body)
        self.call("check")
        installer("0.3.1-rc.1", "    printf changed\n")
        self.assertIn("does not match its released template", self.call("check", expected=1).stderr)
        installer("0.3.2", body)
        self.assertIn("fetch Marmish release tags", self.call("check", expected=1).stderr)

    def test_output_tampering_fails_check(self):
        self.update()
        self.write("docs/ontology/symbols.jsonl", "")
        self.git("add", "docs/ontology/symbols.jsonl")
        self.assertIn("symbols.jsonl", self.call("check", expected=1).stderr)


class InstallerBindings(unittest.TestCase):
    def test_exact_metadata_only(self):
        source = b"    version='0.3.0'\n" + b"".join(
            b"    " + key + b"='" + b"a" * 64 + b"'\n"
            for key in INSTALLER_BINDINGS if key != b"version"
        )
        next_release = source.replace(b"0.3.0", b"0.3.1").replace(b"a" * 64, b"c" * 64)
        self.assertEqual(installer_values(source)[b"version"], b"0.3.0")
        self.assertEqual(installer_values(next_release)[b"version"], b"0.3.1")
        with self.assertRaisesRegex(ValueError, "version"):
            installer_values(source.replace(b"version", b"version_typo", 1))
        with self.assertRaisesRegex(ValueError, "version"):
            installer_values(source + b"version='0.3.2'\n")


if __name__ == "__main__":
    unittest.main()
