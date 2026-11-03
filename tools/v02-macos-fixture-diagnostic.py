#!/usr/bin/env python3
"""Bounded, build-only utility probes; never installed with Marmish."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def run(case, argv, root, env, data=b""):
    try:
        result = subprocess.run(argv, cwd=root, env=env, input=data,
                                capture_output=True, timeout=3, check=False)
        record = {"case": case, "returncode": result.returncode,
                  "stdout_hex": result.stdout[:512].hex(),
                  "stderr_hex": result.stderr[:512].hex(),
                  "stdout_bytes": len(result.stdout), "stderr_bytes": len(result.stderr)}
    except (OSError, subprocess.TimeoutExpired) as error:
        record = {"case": case, "error_type": type(error).__name__,
                  "errno": getattr(error, "errno", None)}
    print(json.dumps(record, sort_keys=True), flush=True)


def main():
    with tempfile.TemporaryDirectory(prefix="marmish-fixture-diagnostic-") as temporary:
        root = Path(temporary).resolve()
        for name in ("bin", "home", "tmp", "utilities"):
            (root / name).mkdir(mode=0o700)
        env = {"PATH": str(root / "utilities"), "HOME": str(root / "home"),
               "TMPDIR": str(root / "tmp"), "LANG": "C"}
        for utility in ("/bin/sh", "/bin/cat", "/bin/sleep", "/usr/bin/printf"):
            with Path(utility).open("rb") as source:
                header = source.read(512)
            print(json.dumps({"case": "utility_metadata", "utility": utility,
                              "script_header_hex": header.hex() if header.startswith(b"#!") else None}), flush=True)
            run("file:" + utility, ["/usr/bin/file", "-L", utility], root, env)
        run("printf_original", ["/usr/bin/printf", "binary:%s", "ok"], root, env)
        executable = root / "bin/marmish"
        shutil.copy("/usr/bin/printf", executable)
        run("printf_renamed_fresh", [str(executable), "binary:%s", "ok"], root, env)
        executable.unlink()
        shutil.copy("/bin/sleep", executable)
        run("copied_sleep_before_overwrite", [str(executable), "0"], root, env)
        shutil.copy("/usr/bin/printf", executable)
        run("printf_overwritten_executed_inode", [str(executable), "binary:%s", "ok"], root, env)
        executable.unlink()
        shutil.copy("/usr/bin/printf", executable)
        run("printf_recreated_inode", [str(executable), "binary:%s", "ok"], root, env)
        run("fractional_sleep", ["/bin/sleep", "0.03"], root, env)
        run("shell_binary_printf", ["/bin/sh", "-c", "printf '\\000\\377'; printf '\\376\\001' >&2"], root, env)
        executable.unlink()
        for index in range(2):
            executable.write_text("#!/bin/sh\n/bin/cat >/dev/null\nprintf '%s\\n' 'marmish handle: synthetic' >&2\n/bin/sleep 0.03\nexit 0\n")
            executable.chmod(0o700)
            run("script_rewrite:" + str(index), [str(executable), "submit"], root, env, b"\x00\xff")


if __name__ == "__main__":
    main()
