"""SQLite and Auto Ontology representations of the Marmish code graph."""

import json
import os
import sqlite3
from dataclasses import dataclass
from pathlib import Path
from tempfile import NamedTemporaryFile

from ossie_nvidia_auto_ontology import convert_ossie_to_auto_ontology


@dataclass(frozen=True)
class Column:
    name: str
    description: str
    storage_type: str = "TEXT"
    nullable: bool = False


SCHEMA = {
    "source_files": (
        Column("path", "Repository-relative path in the indexed Git tree."),
        Column("language", "Parser language or inventory category."),
        Column("mode", "Git file mode in octal."),
        Column("sha256", "SHA-256 fingerprint of the indexed file content."),
        Column("parse_status", "Structural parser result or inventory-only status."),
    ),
    "symbols": (
        Column("id", "Stable lexical declaration identifier."),
        Column("path", "File containing the declaration."),
        Column("parent_id", "Enclosing declaration, when present.", nullable=True),
        Column("kind", "Declaration kind reported by the parser."),
        Column("name", "Declaration name as written in the source."),
        Column("start_line", "First source line, starting at one.", "INTEGER"),
        Column("end_line", "Last source line, starting at one.", "INTEGER"),
    ),
    "code_references": (
        Column("id", "Stable source reference identifier."),
        Column("path", "File containing the reference."),
        Column("scope_id", "Enclosing declaration, when present.", nullable=True),
        Column("kind", "Syntactic reference kind, such as import or call."),
        Column("target", "Reference spelling recorded by the parser."),
        Column("line", "Source line, starting at one.", "INTEGER"),
    ),
}

TABLES = {
    name: tuple(column.name for column in columns) for name, columns in SCHEMA.items()
}

DESCRIPTIONS = {
    "source_files": "Tracked code, build inputs, tests and release configuration.",
    "symbols": "Named declarations and their lexical containment.",
    "code_references": "Syntactic imports and calls with their source locations.",
}

# SQLite and NVIDIA's native catalog can each represent these as distinct joins.
RELATIONSHIPS = (
    ("symbol_file", "symbols", "path", "source_files"),
    ("symbol_parent", "symbols", "parent_id", "symbols"),
    ("reference_file", "code_references", "path", "source_files"),
    ("reference_scope", "code_references", "scope_id", "symbols"),
)


def semantic_model() -> bytes:
    """Return an Apache Ossie semantic model in JSON-compatible YAML."""
    datasets = []
    for table, columns in SCHEMA.items():
        fields = [
            {
                "name": column.name,
                "description": column.description,
                "datatype": "String" if column.storage_type == "TEXT" else "Integer",
                "expression": {
                    "dialects": [{"dialect": "ANSI_SQL", "expression": column.name}]
                },
            }
            for column in columns
        ]
        datasets.append(
            {
                "name": table,
                "source": f"marmish.main.{table}",
                "description": DESCRIPTIONS[table],
                "primary_key": [TABLES[table][0]],
                "fields": fields,
            }
        )
    relationships = [
        {
            "name": name,
            "from": source,
            "to": target,
            "from_columns": [column],
            "to_columns": [TABLES[target][0]],
        }
        for name, source, column, target in RELATIONSHIPS
    ]
    document = {
        "version": "0.2.0.dev0",
        "name": "marmish",
        "description": "Source files, lexical declarations and syntactic references in Marmish.",
        "datasets": datasets,
        "relationships": relationships,
    }
    return (json.dumps(document, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def native_model() -> bytes:
    """Convert Marmish's Ossie model to NVIDIA's native import document."""
    return convert_ossie_to_auto_ontology(semantic_model().decode("utf-8")).encode(
        "utf-8"
    )


def materialize(path: Path, rows: dict[str, list[dict[str, str | int | None]]]) -> None:
    """Write a complete queryable SQLite snapshot and replace the previous one."""
    with NamedTemporaryFile(
        dir=path.parent, prefix=f".{path.name}.", delete=False
    ) as handle:
        temporary_path = Path(handle.name)
    try:
        database = sqlite3.connect(temporary_path)
        try:
            database.execute("PRAGMA foreign_keys = ON")
            with database:
                for table, columns in SCHEMA.items():
                    _create_table(database, table, columns)
                    values = ", ".join(f":{column.name}" for column in columns)
                    database.executemany(
                        f"INSERT INTO {table} ({', '.join(TABLES[table])}) VALUES ({values})",
                        rows[table],
                    )
        finally:
            database.close()
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def _create_table(
    database: sqlite3.Connection, table: str, columns: tuple[Column, ...]
) -> None:
    fields = [
        f"{column.name} {column.storage_type}"
        + ("" if column.nullable else " NOT NULL")
        for column in columns
    ]
    fields.append(f"PRIMARY KEY ({columns[0].name})")
    fields.extend(
        f"FOREIGN KEY ({column}) REFERENCES {target} ({TABLES[target][0]}) "
        "DEFERRABLE INITIALLY DEFERRED"
        for _, source, column, target in RELATIONSHIPS
        if source == table
    )
    database.execute(f"CREATE TABLE {table} ({', '.join(fields)})")
