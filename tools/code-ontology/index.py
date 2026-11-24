"""Extract a deterministic, source-grounded graph from a Marmish Git tree."""

import argparse
import hashlib
import io
import json
import os
import re
import subprocess
from collections import Counter, defaultdict
from importlib import import_module
from pathlib import Path, PurePosixPath
from urllib.parse import quote

from model import TABLES, materialize, native_model, semantic_model
from tree_sitter import Language, Parser

OUTPUT = PurePosixPath("docs/ontology")
PUBLIC_INSTALLER = PurePosixPath("scripts/install.sh")
INSTALLER_TEMPLATE = "scripts/install-native.sh"
GRAPH_FILES = {
    **{f"{name}.jsonl": name for name in TABLES},
    "semantic-model.yaml": None,
    "auto-ontology.yaml": None,
}
LANGUAGES = {
    ".rs": "rust", ".py": "python", ".sh": "bash", ".c": "c", ".h": "c",
    ".js": "javascript", ".mjs": "javascript", ".cjs": "javascript",
}
INVENTORY_TYPES = {
    ".toml", ".json", ".yml", ".yaml", ".lock", ".patch", ".md", ".jq",
}
SOURCE_ROOTS = {"crates", "tools", "scripts", "tests", "patches", ".github"}
ROOT_INPUTS = {
    "AGENTS.md", "README.md", "CHANGELOG.md", "LICENSE", ".gitattributes",
    ".gitignore", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml",
}
SYMBOLS = {
    "rust": {
        "function_item", "function_signature_item", "struct_item", "enum_item",
        "impl_item", "trait_item", "mod_item", "const_item", "type_item",
        "associated_type", "static_item", "union_item", "macro_definition",
        "enum_variant", "field_declaration",
    },
    "python": {"function_definition", "class_definition"},
    "javascript": {
        "function_declaration", "generator_function_declaration",
        "class_declaration", "method_definition", "variable_declarator",
    },
    "bash": {"function_definition"},
    "c": {
        "function_definition", "type_definition", "struct_specifier",
        "enum_specifier", "union_specifier", "enumerator", "preproc_def",
        "preproc_function_def",
    },
}
IMPORTS = {
    "rust": {"use_declaration": "argument", "extern_crate_declaration": "name"},
    "python": {"import_statement": "name", "import_from_statement": "module_name"},
    "javascript": {"import_statement": "source", "export_statement": "source"},
    "bash": {},
    "c": {"preproc_include": "path"},
}
CALLS = {
    "rust": {"call_expression": "function", "macro_invocation": "macro"},
    "python": {"call": "function"},
    "javascript": {"call_expression": "function", "new_expression": "constructor"},
    "bash": {"command": "name"},
    "c": {"call_expression": "function"},
}
# Dotted names and statically spelled paths are useful targets. Dynamic
# expressions are left unresolved and no argument or string value is stored.
NAME = re.compile(r"""[\w ./:'"<>@*{},+&!?\-$#\\\[\]=|~^%]+""")
CALLABLE = re.compile(r"(?![0-9])[\w .:<>?!&+,*\-]+")
INSTALLER_BINDINGS = {
    b"version": (rb"[0-9]+\.[0-9]+\.[0-9]+(?:-rc\.[1-9][0-9]*)?", b"@MARMISH_VERSION@"),
    b"sha_amd64": (rb"[0-9a-f]{64}", b"@MARMISH_SHA_AMD64@"),
    b"sha_arm64": (rb"[0-9a-f]{64}", b"@MARMISH_SHA_ARM64@"),
    b"sha_macos": (rb"[0-9a-f]{64}", b"@MARMISH_SHA_MACOS@"),
}


def git(*args, data=None):
    return subprocess.run(
        ["git", *args], input=data, stdout=subprocess.PIPE, check=True,
    ).stdout


def classify(path):
    """Classify each tracked path as source, inventory, generated, or excluded."""
    parts = path.parts
    if parts[0] == ".beads":
        return "excluded"
    if path == PUBLIC_INSTALLER:
        return "release_artifact"
    if path.parent == OUTPUT and path.name in GRAPH_FILES:
        return "generated"
    if path.suffix in LANGUAGES:
        return LANGUAGES[path.suffix]
    if path.name in ROOT_INPUTS and len(parts) == 1:
        return "inventory"
    if parts[0] in SOURCE_ROOTS and path.suffix in INVENTORY_TYPES:
        return "inventory"
    if parts[0] == "docs" and path.suffix in {".md", ".json"}:
        return "excluded"
    raise ValueError(f"unclassified tracked file {str(path)!r}")


def entries(tree):
    objects = {}
    for entry in git("ls-tree", "-r", "--full-tree", "-z", tree).split(b"\0"):
        if not entry:
            continue
        metadata, filename = entry.split(b"\t", 1)
        mode, kind, oid = metadata.decode("ascii").split()
        path = PurePosixPath(filename.decode("utf-8"))
        category = classify(path)
        if category == "excluded":
            continue
        if kind != "blob" or mode not in {"100644", "100755"}:
            raise ValueError(f"unsupported Git entry {str(path)!r} ({mode})")
        objects[path] = (mode, category, oid)
    return objects


def blobs(objects):
    ids = sorted({oid for _, _, oid in objects.values()})
    if not ids:
        return {}
    contents = io.BytesIO(git("cat-file", "--batch", data=("\n".join(ids) + "\n").encode("ascii")))
    found = {}
    for oid in ids:
        reported, kind, size = contents.readline().decode("ascii").split()
        if reported != oid or kind != "blob":
            raise ValueError(f"expected Git blob {oid}")
        found[oid] = contents.read(int(size))
        if contents.read(1) != b"\n":
            raise ValueError(f"incomplete Git blob {oid}")
    return found


def installer_values(source):
    values = {}
    for key, (pattern, _) in INSTALLER_BINDINGS.items():
        assignment = re.compile(rb"^[ \t]*" + key + rb"[ \t]*=[^\n]*$", re.MULTILINE)
        found = assignment.findall(source)
        match = re.fullmatch(rb" {4}" + key + rb"='(" + pattern + rb")'", found[0]) if len(found) == 1 else None
        if not match:
            raise ValueError(f"unexpected generated installer binding {key.decode('ascii')}")
        values[key] = match[1]
    return values


def verify_installer(objects, contents):
    if PUBLIC_INSTALLER not in objects:
        return
    mode, _, oid = objects[PUBLIC_INSTALLER]
    if mode != "100755":
        raise ValueError("public installer must be executable")
    source = contents[oid]
    values = installer_values(source)
    tag = f"refs/tags/v{values[b'version'].decode('ascii')}"
    try:
        template = git("show", f"{tag}^{{commit}}:{INSTALLER_TEMPLATE}")
    except subprocess.CalledProcessError as error:
        raise ValueError(f"cannot read '{tag}'; fetch Marmish release tags") from error
    for key, (_, placeholder) in INSTALLER_BINDINGS.items():
        if template.count(placeholder) != 1:
            raise ValueError(f"unexpected renderer placeholder in {tag}: {placeholder.decode('ascii')}")
        template = template.replace(placeholder, values[key])
    if b"@MARMISH_" in template or template != source:
        raise ValueError(f"public installer does not match its released template at {tag}")


def spelling(node, allowed=NAME):
    if node is None or node.end_byte - node.start_byte > 400:
        return None
    value = node.text.decode("utf-8", errors="replace").strip()
    value = " ".join(value.split())
    return value if allowed.fullmatch(value) else None


def declaration(node):
    name = node.child_by_field_name("name")
    if node.type == "impl_item":
        trait = spelling(node.child_by_field_name("trait"))
        subject = spelling(node.child_by_field_name("type"))
        return f"{trait} for {subject}" if trait and subject else subject
    if node.type == "variable_declarator" and name and name.type != "identifier":
        return None
    if name is None:
        name = node.child_by_field_name("declarator")
        while name is not None and name.child_by_field_name("declarator") is not None:
            name = name.child_by_field_name("declarator")
    return spelling(name)


def references(node, language):
    fields = CALLS[language]
    if field := fields.get(node.type):
        raw_target = node.child_by_field_name(field)
        if (raw_target and raw_target.type not in {"string", "template_string"}
                and (target := spelling(raw_target, CALLABLE))):
            yield "macro" if node.type == "macro_invocation" else "call", target
    if field := IMPORTS[language].get(node.type):
        for raw_target in node.children_by_field_name(field):
            if target := spelling(raw_target):
                yield "import", target.strip("'\x22<>")


def key(prefix, kind, name, repeats):
    base = f"{prefix}/{kind}:{quote(name, safe='')}"
    repeats[base] += 1
    return f"{base}#{repeats[base]}"


def extract(rows, source_path, parser, code, language):
    root = parser.parse(code).root_node
    repeats = Counter()
    omitted = False
    file_key = quote(source_path, safe="/")
    pending = [(root, None)]
    while pending:
        node, parent_id = pending.pop()
        if node.type in SYMBOLS[language]:
            name = declaration(node)
            if name is None:
                omitted = True
            else:
                symbol_id = key(parent_id or file_key, node.type, name, repeats)
                rows["symbols"].append({
                    "id": symbol_id, "path": source_path, "parent_id": parent_id,
                    "kind": node.type, "name": name, "start_line": node.start_point.row + 1,
                    "end_line": node.end_point.row + int(node.end_point.column != 0),
                })
                parent_id = symbol_id
        for kind, target in references(node, language):
            if target:
                rows["code_references"].append({
                    "id": key(parent_id or file_key, kind, target, repeats),
                    "path": source_path, "scope_id": parent_id, "kind": kind,
                    "target": target, "line": node.start_point.row + 1,
                })
        pending.extend((child, parent_id) for child in reversed(node.named_children))
    status = "recovered" if root.has_error else "parsed"
    return status + "_with_omissions" if omitted else status


def generate(objects, contents):
    parsers = {
        language: Parser(Language(import_module(f"tree_sitter_{language}").language()))
        for language in sorted(set(LANGUAGES.values()))
    }
    rows = {table: [] for table in TABLES}
    for path in sorted(objects, key=str):
        mode, category, oid = objects[path]
        if category in {"generated", "release_artifact"}:
            continue
        data = contents[oid]
        source_path = str(path)
        status = "inventory"
        if category in parsers:
            status = extract(rows, source_path, parsers[category], data, category)
        rows["source_files"].append({
            "path": source_path, "language": category, "mode": mode,
            "sha256": hashlib.sha256(data).hexdigest(), "parse_status": status,
        })
    return rows


def json_lines(records):
    return "".join(
        json.dumps(row, ensure_ascii=False, separators=(",", ":")) + "\n"
        for row in records
    ).encode("utf-8")


def grouped_references(rows):
    scopes = defaultdict(dict)
    for declaration in rows["symbols"]:
        indexes = scopes[declaration["path"]]
        indexes[declaration["id"]] = len(indexes) + 1
    groups = defaultdict(list)
    for ref in rows["code_references"]:
        path = ref["path"]
        ordinal = scopes[path][ref["scope_id"]] if ref["scope_id"] is not None else 0
        groups[path].append([ordinal, ref["kind"], ref["target"], ref["line"]])
    return ({"path": path, "references": refs} for path, refs in groups.items())


def render(rows):
    return {
        "source_files.jsonl": json_lines(rows["source_files"]),
        "symbols.jsonl": json_lines(rows["symbols"]),
        "code_references.jsonl": json_lines(grouped_references(rows)),
        "semantic-model.yaml": semantic_model(),
        "auto-ontology.yaml": native_model(),
    }


def changes(objects, contents, expected):
    return [
        name for name, data in expected.items()
        if (entry := objects.get(OUTPUT / name)) is None
        or entry[0] != "100644" or contents[entry[2]] != data
    ]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("update", help="generate graph files from staged inputs")
    commands.add_parser("check", help="compare graph files and inputs in the same Git tree").add_argument("--ref")
    sqlite = commands.add_parser("sqlite", help="materialize a verified graph for Auto Ontology")
    sqlite.add_argument("--ref")
    sqlite.add_argument("path", type=Path, metavar="marmish.sqlite")
    args = parser.parse_args()
    try:
        export_path = args.path.absolute() if args.command == "sqlite" else None
        os.chdir(os.fsdecode(git("rev-parse", "--show-toplevel").removesuffix(b"\n")))
        ref = getattr(args, "ref", None)
        tree = git("rev-parse", "--verify", "--end-of-options", ref + "^{tree}").decode().strip() if ref else git("write-tree").decode().strip()
        objects = entries(tree)
        contents = blobs(objects)
        verify_installer(objects, contents)
        rows = generate(objects, contents)
        expected = render(rows)
        if args.command == "update":
            output = Path(OUTPUT)
            output.mkdir(parents=True, exist_ok=True)
            if output.resolve() != Path.cwd() / output or any((output / name).is_symlink() for name in expected):
                raise ValueError("ontology output must be ordinary repository files")
            for name, data in expected.items():
                (output / name).write_bytes(data)
                (output / name).chmod(0o644)
        else:
            stale = changes(objects, contents, expected)
            if stale:
                raise ValueError(
                    f"missing or stale graph files in {tree}: {', '.join(stale)}. "
                    "Stage the inputs, run the ontology update, then stage docs/ontology."
                )
            if args.command == "sqlite":
                materialize(export_path, rows)
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        parser.exit(1, f"code ontology: {exc}\n")
    counts = ", ".join(f"{len(records):,} {table}" for table, records in rows.items())
    print(f"code ontology {args.command}: {counts}")


if __name__ == "__main__":
    main()
