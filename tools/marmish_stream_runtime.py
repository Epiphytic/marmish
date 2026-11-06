#!/usr/bin/env python3
"""Qualify installed/native stream CLIs against an explicitly selected relay.

Uses disposable file-backed identities and a private HOME/cwd. No private logs,
command diagnostics, profile paths, or secret values enter the JSON report.
Failed private state is retained locally for inspection, never uploaded by this tool.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import selectors
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit


class QualificationError(Exception):
    """Only fixed finite categories are safe to report outside private state."""

    def __init__(self, stage: str, category: str):
        super().__init__(category)
        self.stage = stage
        self.category = category


def bounded_integer(minimum: int, maximum: int):
    def parse(value: str) -> int:
        try:
            number = int(value)
        except ValueError as error:
            raise argparse.ArgumentTypeError("expected an integer") from error
        if not minimum <= number <= maximum:
            raise argparse.ArgumentTypeError(f"expected {minimum}..{maximum}")
        return number

    return parse


def arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--marmish", required=True, type=Path)
    parser.add_argument("--relay", required=True)
    parser.add_argument("--source-revision", help="optional 40-character source commit SHA")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--bytes", type=bounded_integer(1, 64 * 1024 * 1024), default=393216)
    parser.add_argument("--samples", type=bounded_integer(1, 50), default=3)
    parser.add_argument("--timeout-seconds", type=bounded_integer(10, 86400), default=600)
    options = parser.parse_args()
    if options.source_revision is not None:
        if re.fullmatch(r"[0-9a-fA-F]{40}", options.source_revision) is None:
            parser.error("--source-revision must be a 40-character hexadecimal commit SHA")
        options.source_revision = options.source_revision.lower()
    options.marmish = options.marmish.resolve()
    options.output = options.output.resolve()
    if not options.marmish.is_file() or not os.access(options.marmish, os.X_OK):
        parser.error("--marmish must identify an executable file")
    if not options.output.parent.is_dir() or options.output.is_dir():
        parser.error("--output must identify a file in an existing directory")
    try:
        relay = urlsplit(options.relay)
        valid = relay.scheme in {"ws", "wss"} and bool(relay.hostname)
        valid = valid and not any((relay.username, relay.password, relay.query, relay.fragment))
        _ = relay.port  # Validate the port without resolving the relay hostname.
    except ValueError:
        valid = False
    if not valid:
        parser.error("--relay must be an explicit ws/wss URL without credentials, query, or fragment")
    return options


def public_npub(value: str) -> str:
    """Encode the public 32-byte identity returned by `init` as NIP-19 npub."""
    try:
        raw = bytes.fromhex(value)
    except (TypeError, ValueError) as error:
        raise QualificationError("identity", "public_identity_schema") from error
    if len(raw) != 32:
        raise QualificationError("identity", "public_identity_schema")
    values = []
    accumulator = bits = 0
    for byte in raw:
        accumulator = (accumulator << 8) | byte
        bits += 8
        while bits >= 5:
            bits -= 5
            values.append((accumulator >> bits) & 31)
    if bits:
        values.append((accumulator << (5 - bits)) & 31)
    prefix = "npub"
    expanded = [ord(char) >> 5 for char in prefix] + [0] + [ord(char) & 31 for char in prefix]
    checksum = 1
    generators = (0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3)
    for value in expanded + values + [0] * 6:
        high = checksum >> 25
        checksum = ((checksum & 0x1FFFFFF) << 5) ^ value
        for index, generator in enumerate(generators):
            if (high >> index) & 1:
                checksum ^= generator
    checksum ^= 1
    values.extend((checksum >> (5 * (5 - index))) & 31 for index in range(6))
    alphabet = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    return prefix + "1" + "".join(alphabet[value] for value in values)


class Qualification:
    def __init__(self, options: argparse.Namespace):
        self.options = options
        # HOME placement also satisfies native profile filesystem ownership rules.
        self.root = Path(tempfile.mkdtemp(prefix=".marmish-stream-check-", dir=Path.home()))
        self.root.chmod(0o700)
        for name in ("home", "work", "runtime", "tmp"):
            (self.root / name).mkdir(mode=0o700)
        # Qualification never forwards ambient API tokens, preload variables or
        # unrelated user configuration into daemon debug logs or fixtures.
        self.environment = {name: os.environ[name] for name in ("PATH", "LANG", "LC_ALL", "TERM")
                            if name in os.environ}
        self.environment.update(
            HOME=str(self.root / "home"),
            XDG_CONFIG_HOME=str(self.root / "home" / ".config"),
            XDG_DATA_HOME=str(self.root / "home" / ".local" / "share"),
            XDG_CACHE_HOME=str(self.root / "home" / ".cache"),
            XDG_RUNTIME_DIR=str(self.root / "runtime"),
            TMPDIR=str(self.root / "tmp"),
        )
        self.children: list[tuple[subprocess.Popen, object | None, str | None]] = []
        relay = urlsplit(options.relay)
        self.report = {
            "schema": 1,
            "passed": False,
            "relay": relay.scheme + "://" + relay.netloc,
            "relay_path_redacted": bool(relay.path and relay.path != "/"),
            "cleanup_scope": "owned_daemon_shutdown_and_protocol_cancel",
            "platform": os.uname().sysname,
            "architecture": os.uname().machine,
            "payload_bytes": options.bytes,
            "latency_samples": options.samples,
            "results": [],
        }

    def command(self, identity: str, *arguments: str) -> list[str]:
        arguments = list(arguments)
        if arguments and arguments[0] in {"status", "stream"}:
            arguments.insert(0, "--no-autostart")
        return [str(self.options.marmish), "--state-dir", str(self.root / "state"),
                "-i", identity, "--key-storage", "file", *arguments]

    def spawn(self, identity: str, *arguments: str, log=None, input_pipe=False, daemon_role=None):
        # Main-thread-only harness: defer raising termination handlers until the
        # Popen is registered. No blocked signal mask or pre_exec callback can
        # leak into the child. Caught Python handlers reset on exec as usual.
        pending = []
        previous = {}

        def defer(number, _frame):
            if number not in pending:
                pending.append(number)

        try:
            for number in (signal.SIGINT, signal.SIGTERM):
                previous[number] = signal.signal(number, defer)
            process = subprocess.Popen(
                self.command(identity, *arguments),
                cwd=self.root / "work", env=self.environment, start_new_session=True,
                stdin=subprocess.PIPE if input_pipe else subprocess.DEVNULL,
                stdout=log if log is not None else subprocess.PIPE,
                stderr=subprocess.STDOUT if log is not None else subprocess.PIPE,
            )
            self.children.append((process, log, daemon_role))
        finally:
            for number, handler in previous.items():
                signal.signal(number, handler)
            for number in pending:
                handler = previous[number]
                if callable(handler):
                    handler(number, None)
                elif handler != signal.SIG_IGN:
                    signal.raise_signal(number)
        return process

    def run(self, stage: str, identity: str, *arguments: str, data=b"", timeout=None):
        process = self.spawn(identity, *arguments, input_pipe=True)
        try:
            stdout, stderr = process.communicate(
                input=data, timeout=timeout or self.options.timeout_seconds
            )
        except subprocess.TimeoutExpired as error:
            raise QualificationError(stage, "process_timeout") from error
        # Diagnostics remain private even when the subprocess unexpectedly emits
        # arbitrary data. Report only fixed categories and validated metrics.
        if process.returncode not in {0, 7}:
            (self.root / f"{stage}.stderr").write_bytes(stderr)
        return subprocess.CompletedProcess(process.args, process.returncode, stdout, stderr)

    @staticmethod
    def require_success(result, stage: str):
        if result.returncode != 0:
            raise QualificationError(stage, "command_failed")
        return result

    def identify_binary(self):
        digest = hashlib.sha256()
        with self.options.marmish.open("rb") as binary:
            while chunk := binary.read(1024 * 1024):
                digest.update(chunk)
        result = self.require_success(
            self.run("version", "client", "--version", timeout=20), "version"
        )
        try:
            version_output = result.stdout.decode("ascii").strip()
        except UnicodeDecodeError as error:
            raise QualificationError("version", "version_schema") from error
        # Never place arbitrary executable stdout into a public report.
        match = re.fullmatch(
            r"(?:marmish(?:-native)? )?([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?)",
            version_output,
        )
        if match is None or len(version_output) > 128:
            raise QualificationError("version", "version_schema")
        self.report["binary"] = {"sha256": digest.hexdigest(), "version": match.group(1)}
        if self.options.source_revision is not None:
            self.report["source_revision"] = self.options.source_revision

    def initialize(self) -> str:
        identities = {}
        for identity in ("client", "server"):
            result = self.require_success(
                self.run("init_" + identity, identity, "--relay", self.options.relay, "init"), "identity"
            )
            try:
                document = json.loads(result.stdout)
                identities[identity] = public_npub(document["identity"])
            except (ValueError, KeyError, TypeError) as error:
                raise QualificationError("identity", "public_identity_schema") from error
        for identity, role, permission, peer in (
            ("client", "client", "--trust", "server"),
            ("server", "server", "--grant", "client"),
        ):
            self.require_success(self.run(
                "setup_" + identity, identity, "--relay", self.options.relay,
                "setup", "--role", role, permission, identities[peer],
            ), "setup")
        # Broaden only this disposable profile to exercise explicitly selected
        # fixtures. Real setup, identities, trust, and execution grants stay native.
        config_path = self.root / "state" / "server" / "config.json"
        config = json.loads(config_path.read_text())
        config["command_policy"] = {"version": 1, "defaults": {"allow_exec": ["/**"]}}
        config["cwd"] = str(self.root / "work")
        config_path.write_text(json.dumps(config))
        config_path.chmod(0o600)
        return identities["server"]

    @staticmethod
    def daemon_ready(status, identity: str) -> bool:
        if status.returncode != 0:
            return False
        try:
            if len(status.stdout) > 65536:
                raise ValueError
            envelope = json.loads(status.stdout)
            if (not isinstance(envelope, dict) or set(envelope) != {"ok", "result", "error"}
                    or envelope["ok"] is not True or envelope["error"] is not None):
                raise ValueError
            result = envelope["result"]
            if not isinstance(result, dict):
                raise ValueError
            if result == {"state": "activating"}:
                return False
            if (set(result) != {"state", "identity", "conversations", "received", "pending"}
                    or result["state"] not in {"active", "transport_degraded"}
                    or not isinstance(result["identity"], str) or not 1 <= len(result["identity"]) <= 256
                    or any(type(result[key]) is not int or not 0 <= result[key] <= 2**64 - 1
                           for key in ("conversations", "received", "pending"))):
                raise ValueError
            return result["state"] == "active"
        except (ValueError, TypeError, UnicodeDecodeError):
            raise QualificationError("daemon_" + identity, "status_schema") from None

    def start_daemons(self):
        for identity in ("server", "client"):
            log = (self.root / f"{identity}.log").open("wb")
            try:
                process = self.spawn(identity, "--foreground", "--debug", "daemon", log=log,
                                     daemon_role=identity)
            except BaseException:
                log.close()
                raise
            deadline = time.monotonic() + 90
            while True:
                if process.poll() is not None:
                    raise QualificationError("daemon_" + identity, "daemon_exited")
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise QualificationError("daemon_" + identity, "readiness_timeout")
                status = self.run("status_" + identity, identity, "status", timeout=min(20, remaining))
                if self.daemon_ready(status, identity):
                    break
                time.sleep(min(0.2, max(0, deadline - time.monotonic())))

    def binary_roundtrip(self, remote: str):
        payload = (bytes(range(256)) * ((self.options.bytes + 255) // 256))[:self.options.bytes]
        for compressed in (False, True):
            before = time.monotonic()
            result = self.run(
                "binary_zlib" if compressed else "binary_none", "client", "stream", remote,
                *(["--compress"] if compressed else []), "--", "/bin/sh", "-c",
                'cat; printf "stderr check\\n" >&2; exit 7', data=payload,
            )
            elapsed = time.monotonic() - before
            if result.returncode != 7 or result.stdout != payload or b"stderr check\n" not in result.stderr:
                raise QualificationError("binary_roundtrip", "byte_or_exit_mismatch")
            self.report["results"].append({
                "case": "binary_roundtrip", "compression": "zlib" if compressed else "none",
                "bytes": len(payload), "seconds": round(elapsed, 6),
                "bytes_per_second": round(len(payload) / elapsed, 1),
                "sha256": hashlib.sha256(result.stdout).hexdigest(), "exit": 7,
            })

    def early_stdin_closure(self, remote: str):
        # Exceed one complete stdin credit window even with --bytes=1. Neither
        # command consumes the whole payload; closure must preserve its natural
        # status and output rather than fail the client with a remote Closed.
        offered_bytes = max(self.options.bytes, 262144 + 8192)
        payload = (bytes(range(256)) * ((offered_bytes + 255) // 256))[:offered_bytes]
        for compressed in (False, True):
            for name, argv, expected in (
                ("true", ("/usr/bin/true",), b""),
                ("head", ("/usr/bin/head", "-c", "1"), payload[:1]),
            ):
                codec = "zlib" if compressed else "none"
                stage = "early_stdin_" + name + "_" + codec
                result = self.run(
                    stage, "client", "stream", remote,
                    *(["--compress"] if compressed else []), "--", *argv, data=payload,
                )
                if result.returncode != 0 or result.stdout != expected:
                    raise QualificationError(stage, "natural_exit_or_output_mismatch")
                self.report["results"].append({
                    "case": "early_stdin_closure", "command": name, "compression": codec,
                    "offered_bytes": len(payload), "stdout_bytes": len(result.stdout), "exit": 0,
                })

    def literal_arguments(self, remote: str):
        values = ["space value", "single'quote", 'double"quote', r"back\slash", "$(touch SHOULD_NOT_EXIST)",
                  "semi;colon", "line\nvalue", "", "-leading", "Unicode-🌲"]
        result = self.require_success(self.run(
            "literal_argv", "client", "stream", remote, "--", "/usr/bin/printf", "%s\\0", *values
        ), "literal_argv")
        if result.stdout != b"".join(value.encode() + b"\0" for value in values):
            raise QualificationError("literal_argv", "byte_mismatch")
        if (self.root / "work" / "SHOULD_NOT_EXIST").exists():
            raise QualificationError("literal_argv", "unexpected_shell_evaluation")
        self.report["results"].append({"case": "literal_argv", "passed": True, "arguments": len(values)})

    def latency(self, remote: str):
        elapsed_samples = []
        for index in range(self.options.samples):
            before = time.monotonic()
            result = self.require_success(self.run(
                f"latency_{index}", "client", "stream", remote, "--", "/usr/bin/printf", "marmish-latency"
            ), "latency")
            elapsed = time.monotonic() - before
            if result.stdout != b"marmish-latency":
                raise QualificationError("latency", "byte_mismatch")
            elapsed_samples.append(elapsed)
            self.report["results"].append({"case": "small_command_latency", "sample": index + 1,
                                           "seconds": round(elapsed, 6), "bytes": len(result.stdout)})
        self.report["latency_seconds"] = {
            "min": round(min(elapsed_samples), 6), "median": round(statistics.median(elapsed_samples), 6),
            "max": round(max(elapsed_samples), 6),
        }

    def cancel_backpressure(self, remote: str):
        process = self.spawn(
            "client", "stream", remote, "--", "/bin/sh", "-c",
            'trap "" TERM; printf "ready\\n"; while :; do printf "backpressure backpressure backpressure\\n"; done',
        )
        read_ready(process, timeout=min(180, self.options.timeout_seconds))
        time.sleep(2)
        before = time.monotonic()
        process.send_signal(signal.SIGINT)
        try:
            # Keep stdout blocked during cancellation, rather than accidentally
            # making cancellation work by draining the blocked data lane first.
            process.wait(timeout=min(90, self.options.timeout_seconds))
            _, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired as error:
            raise QualificationError("cancel_backpressure", "cancellation_timeout") from error
        if process.returncode != 130 or b"terminal state confirmed" not in stderr:
            (self.root / "cancel.stderr").write_bytes(stderr)
            raise QualificationError("cancel_backpressure", "cancellation_unconfirmed")
        self.report["results"].append({"case": "ctrl_c_cancel_backpressure", "exit": 130,
                                       "seconds": round(time.monotonic() - before, 6), "confirmed": True})

    def cleanup(self) -> bool:
        # This checks owned daemon shutdown plus the separately validated remote
        # cancellation protocol. It cannot prove arbitrary escaped descendants
        # absent. Never sweep cwd/environment or signal numeric groups after
        # their leader has been reaped: neither establishes process ownership.
        confirmed = True
        for process, log, daemon_role in reversed(self.children):
            try:
                alive = process.poll() is None
                # A daemon must survive until our shutdown request. Even an
                # unexpected zero exit is a failed lifetime qualification.
                if daemon_role is not None and not alive:
                    confirmed = False
                if alive:
                    try:
                        os.killpg(process.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass  # It exited between poll and signaling; still reap it.
                    except PermissionError:
                        confirmed = False
                        process.kill()  # Still an owned, unreaped Popen child.
                    try:
                        process.wait(timeout=45)
                    except subprocess.TimeoutExpired:
                        confirmed = False  # Forced daemon death can leave unowned remote children.
                        try:
                            os.killpg(process.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                        process.wait(timeout=10)
            except (OSError, subprocess.TimeoutExpired):
                confirmed = False
            finally:
                if daemon_role is not None:
                    if process.returncode != 0:
                        confirmed = False
                    self.report.setdefault("daemon_shutdown", []).append({
                        "role": daemon_role if daemon_role in {"server", "client"} else "daemon",
                        "exit": process.returncode,
                    })
                for pipe in (process.stdin, process.stdout, process.stderr):
                    if pipe is not None:
                        try:
                            pipe.close()
                        except OSError:
                            confirmed = False
                if log is not None:
                    try:
                        log.close()
                    except OSError:
                        confirmed = False
        return confirmed


def read_ready(process: subprocess.Popen, timeout: float):
    """Read exactly the marker without buffering any subsequent stdout bytes."""
    deadline = time.monotonic() + timeout
    marker = bytearray()
    with selectors.DefaultSelector() as selector:
        selector.register(process.stdout, selectors.EVENT_READ)
        while len(marker) < len(b"ready\n"):
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not selector.select(remaining):
                raise QualificationError("cancel_ready", "readiness_timeout")
            chunk = os.read(process.stdout.fileno(), len(b"ready\n") - len(marker))
            if not chunk:
                raise QualificationError("cancel_ready", "unexpected_eof")
            marker.extend(chunk)
    if marker != b"ready\n":
        raise QualificationError("cancel_ready", "marker_mismatch")


def write_report(path: Path, report: dict):
    descriptor, temporary = tempfile.mkstemp(prefix=".marmish-report-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w") as output:
            json.dump(report, output, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main(qualification_type=None, cases=None) -> int:
    options = arguments()
    os.umask(0o077)
    qualification = (qualification_type or Qualification)(options)
    started = time.monotonic()
    previous_handlers = {}

    def interrupted(_number, _frame):
        raise QualificationError("interrupted", "signal_received")

    for number in (signal.SIGINT, signal.SIGTERM):
        previous_handlers[number] = signal.signal(number, interrupted)
    try:
        qualification.identify_binary()
        remote = qualification.initialize()
        qualification.start_daemons()
        if cases is None:
            qualification.binary_roundtrip(remote)
            qualification.early_stdin_closure(remote)
            qualification.literal_arguments(remote)
            qualification.latency(remote)
            qualification.cancel_backpressure(remote)
        else:
            cases(qualification, remote)
        qualification.report["passed"] = True
    except QualificationError as error:
        qualification.report.update(passed=False, error={"stage": error.stage, "category": error.category})
    except Exception:
        qualification.report.update(passed=False, error={"stage": "qualification", "category": "unexpected_failure"})
    finally:
        # A repeated terminal interrupt cannot skip owned-process cleanup.
        for number in previous_handlers:
            signal.signal(number, signal.SIG_IGN)
        clean = qualification.cleanup()
        if not clean:
            qualification.report.update(passed=False, cleanup="unconfirmed")
        qualification.report["total_seconds"] = round(time.monotonic() - started, 6)
        qualification.report["private_state_retained"] = not qualification.report["passed"]
        try:
            write_report(options.output, qualification.report)
        except OSError:
            qualification.report.update(passed=False, private_state_retained=True,
                                        error={"stage": "report", "category": "report_write_failed"})
        if qualification.report["passed"]:
            try:
                shutil.rmtree(qualification.root)
            except OSError:
                qualification.report.update(passed=False, private_state_retained=True,
                                            error={"stage": "cleanup", "category": "state_removal_failed"})
                try:
                    write_report(options.output, qualification.report)
                except OSError:
                    # Do not leave a passing report we just wrote when cleanup
                    # failed. Filesystem failures can also refuse this best effort;
                    # consumers must still require successful process exit.
                    try:
                        options.output.unlink()
                    except OSError:
                        pass
        if qualification.report["private_state_retained"]:
            print("Private diagnostic directory retained:", qualification.root, file=sys.stderr)
        try:
            print(json.dumps(qualification.report, indent=2))
        finally:
            for number, handler in previous_handlers.items():
                signal.signal(number, handler)
    return 0 if qualification.report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
