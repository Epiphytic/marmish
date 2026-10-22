#!/usr/bin/env python3
"""Explicitly opted-in synthetic MDK compatibility probe, never a shell transport.

Only disposable identities and harness-generated public test payloads are used.
No public network operation runs on import, in tests, or without --live.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import selectors
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit

CAP = 1024 * 1024
KIND = 30100
HEX = re.compile(r"[0-9a-f]{64}\Z")


class SmokeError(Exception):
    """A fixed diagnostic category safe to include in the public report."""


def require(condition, category="schema_mismatch"):
    if not condition:
        raise SmokeError(category)


def relay_url(value):
    require(isinstance(value, str) and value.isascii(), "invalid_relay")
    require(not any(c.isspace() or ord(c) < 32 for c in value), "invalid_relay")
    require(not any(c in value for c in ",?#"), "invalid_relay")
    try:
        p = urlsplit(value)
        port = p.port
    except ValueError:
        raise SmokeError("invalid_relay") from None
    host = p.hostname or ""
    require(p.scheme == "wss" and not p.username and not p.password
            and "@" not in p.netloc and p.path in ("", "/"), "invalid_relay")
    require(len(host) <= 253 and all(
        re.fullmatch(r"[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?", label)
        for label in host.split(".")), "invalid_relay")
    require(not p.netloc.endswith(":"), "invalid_relay")
    require(port is None or 1 <= port <= 65535, "invalid_relay")
    return "wss://" + host.lower() + (f":{port}" if port not in (None, 443) else "") + "/"


def public_key(value):
    """Decode checksum-verified NIP-19 npub, or a 32-byte lowercase hex key."""
    require(isinstance(value, str), "invalid_public_identity")
    if HEX.fullmatch(value):
        return value
    alphabet = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    require(value.startswith("npub1") and len(value) == 63, "invalid_public_identity")
    try:
        words = [alphabet.index(c) for c in value[5:]]
    except ValueError:
        raise SmokeError("invalid_public_identity") from None
    check = 1
    expanded = [ord(c) >> 5 for c in "npub"] + [0] + [ord(c) & 31 for c in "npub"]
    for word in expanded + words:
        top = check >> 25
        check = ((check & 0x1ffffff) << 5) ^ word
        for i, generator in enumerate((0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3)):
            if (top >> i) & 1:
                check ^= generator
    require(check == 1, "invalid_public_identity")
    acc = bits = 0
    out = bytearray()
    for word in words[:-6]:
        acc = (acc << 5) | word
        bits += 5
        if bits >= 8:
            bits -= 8
            out.append((acc >> bits) & 255)
    require(len(out) == 32 and bits < 5 and (acc & ((1 << bits) - 1)) == 0,
            "invalid_public_identity")
    return out.hex()


def identifier(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{2,256}", value)
            and len(value) % 2 == 0, "invalid_identifier")
    return value


def response(data):
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError, RecursionError):
        raise SmokeError("invalid_json") from None
    require(isinstance(value, dict) and value.get("ok") is True
            and isinstance(value.get("result"), dict))
    return value["result"]


def verify_identity(value, relays):
    account = public_key(value.get("account_id"))
    require(public_key(value.get("npub")) == account and value.get("local_signing") is True)
    package = value.get("key_package")
    require(isinstance(package, dict) and package.get("published") is True
            and type(package.get("bytes")) is int and package["bytes"] > 0,
            "key_package_not_published")
    lists = value.get("relay_lists")
    require(isinstance(lists, dict) and lists.get("complete") is True and lists.get("missing") == [],
            "relay_lists_incomplete")
    for name in ("default_relays", "bootstrap_relays"):
        require(isinstance(lists.get(name), list))
        require({relay_url(r) for r in lists[name]} == set(relays), "relay_set_mismatch")
    for name in ("nip65", "inbox"):
        record = lists.get(name)
        require(isinstance(record, dict) and isinstance(record.get("relays"), list))
        require({relay_url(r) for r in record["relays"]} == set(relays), "relay_set_mismatch")
        require(record.get("kind") == (10002 if name == "nip65" else 10050))
        if name == "nip65":
            for direction in ("read_relays", "write_relays"):
                require(isinstance(record.get(direction), list))
                require({relay_url(r) for r in record[direction]} == set(relays), "relay_set_mismatch")
    return account


def matching_invite(value, group, inviter):
    invites = value.get("invites")
    require(isinstance(invites, list) and len(invites) <= 200)
    matches = [r for r in invites if isinstance(r, dict) and r.get("group_id") == group]
    if not matches:
        return None
    require(len(matches) == 1 and matches[0].get("pending_confirmation") is True,
            "invite_mismatch")
    require(public_key(matches[0].get("welcomer_account_id")) == inviter, "invite_mismatch")
    return matches[0]


def verify_members(value, accounts, local):
    members = value.get("members")
    require(isinstance(members, list) and len(members) == 2, "membership_mismatch")
    found = []
    for member in members:
        require(isinstance(member, dict), "membership_mismatch")
        key = public_key(member.get("member_id"))
        require(public_key(member.get("npub")) == key
                and member.get("local") is (key == local), "membership_mismatch")
        found.append(key)
    require(set(found) == set(accounts) and len(set(found)) == 2, "membership_mismatch")


def sent_ids(value):
    ids = value.get("message_ids")
    require(type(value.get("published")) is int and value["published"] > 0
            and isinstance(ids, list) and 1 <= len(ids) <= 16,
            "message_not_published")
    return [identifier(x) for x in ids]


def received_message(value, group, sender, payload, ids):
    messages = value.get("messages")
    require(isinstance(messages, list) and len(messages) <= 20)
    matches = [r for r in messages if isinstance(r, dict) and r.get("message_id") in ids]
    if not matches:
        return None
    require(len(matches) == 1, "message_mismatch")
    record = matches[0]
    require(record.get("group_id") == group and record.get("direction") == "received"
            and public_key(record.get("from")) == sender
            and type(record.get("kind")) is int and record["kind"] == KIND
            and record.get("plaintext") == payload and isinstance(record.get("tags"), list),
            "message_mismatch")
    return identifier(record["message_id"])


def copy_binary(source, destination, digest):
    require(Path(source).is_absolute() and re.fullmatch(r"[0-9a-f]{64}", digest), "invalid_binary")
    with os.fdopen(os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as src:
        require(stat.S_ISREG(os.fstat(src.fileno()).st_mode), "invalid_binary")
        with open(destination, "xb") as dst:
            shutil.copyfileobj(src, dst)
    os.chmod(destination, 0o700)
    with open(destination, "rb") as handle:
        actual = hashlib.file_digest(handle, "sha256").hexdigest()
    require(actual == digest, "binary_hash_mismatch")
    return str(destination)


class Processes:
    """Single-threaded bounded pipe servicing and ownership-preserving cleanup."""

    def __init__(self, deadline):
        self.deadline = deadline
        self.selector = selectors.DefaultSelector()
        self.children = []
        self.daemons = []
        self.logs = []

    @staticmethod
    def exited(child):
        return os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None

    def spawn(self, args, env, cwd, daemon=False):
        require(time.monotonic() < self.deadline, "global_timeout")
        child = subprocess.Popen(args, env=env, cwd=cwd, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=True)
        self.children.append(child)
        if daemon:
            self.daemons.append(child)
        buffers = [bytearray(), bytearray()]
        for pipe, buffer in zip((child.stdout, child.stderr), buffers):
            os.set_blocking(pipe.fileno(), False)
            self.selector.register(pipe, selectors.EVENT_READ, (buffer, daemon, [0]))
        return child, buffers

    def tick(self, delay=0.05):
        require(time.monotonic() < self.deadline, "global_timeout")
        for child in self.daemons:
            require(not self.exited(child), "daemon_exited")
        for directory in self.logs:
            for path in directory.iterdir():
                require(not path.is_symlink() and path.is_file(), "unexpected_log_file")
                require(path.stat().st_size <= CAP, "daemon_log_limit")
        for key, _ in self.selector.select(min(delay, max(0, self.deadline - time.monotonic()))):
            data = os.read(key.fd, 65536)
            if not data:
                self.selector.unregister(key.fileobj)
                key.fileobj.close()
                continue
            buffer, discard, count = key.data
            count[0] += len(data)
            require(count[0] <= CAP, "output_limit")
            if not discard:
                buffer.extend(data)

    def pause(self, seconds):
        until = min(self.deadline, time.monotonic() + seconds)
        while time.monotonic() < until:
            self.tick(min(0.05, until - time.monotonic()))

    def finish(self, child):
        # Do not reap the leader before the final group signal: its reserved PID
        # prevents cleanup from targeting a reused process group.
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        end = time.monotonic() + 0.2
        while not self.exited(child) and time.monotonic() < end:
            time.sleep(0.01)
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        result = child.wait()
        self.children.remove(child)
        for pipe in (child.stdout, child.stderr):
            if not pipe.closed:
                try:
                    self.selector.unregister(pipe)
                except KeyError:
                    pass
                pipe.close()
        return result

    def run(self, args, env, cwd, timeout=30):
        child, buffers = self.spawn(args, env, cwd)
        end = min(self.deadline, time.monotonic() + timeout)
        while not self.exited(child):
            require(time.monotonic() < end, "call_timeout")
            self.tick()
        # Drain through EOF, including trailing bytes after leader exit.
        while any(not p.closed for p in (child.stdout, child.stderr)):
            require(time.monotonic() < end, "call_timeout")
            self.tick()
        code = self.finish(child)
        require(code == 0, "subprocess_failure")
        return bytes(buffers[0])

    def close(self):
        self.daemons.clear()
        failure = None
        for child in list(reversed(self.children)):
            try:
                self.finish(child)
            except OSError as error:
                failure = error
        self.selector.close()
        if failure:
            raise failure


class Peer:
    def __init__(self, root, name, wn, wnd, relays, processes):
        self.home = root / name
        self.home.mkdir(mode=0o700)
        self.socket = self.home / "s"
        self.logs = self.home / "logs"
        self.logs.mkdir(mode=0o700)
        self.env = {"HOME": str(self.home), "LANG": "C", "RUST_LOG": "warn"}
        for key, dirname in (("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"),
                             ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")):
            path = self.home / dirname
            path.mkdir(mode=0o700)
            self.env[key] = str(path)
        self.prefix = [wn, "--home", str(self.home), "--socket", str(self.socket),
                       "--secret-store", "file", "--json"]
        self.account = None
        self.processes = processes
        processes.logs.append(self.logs)
        csv = ",".join(relays)
        self.daemon, _ = processes.spawn(
            [wnd, "--home", str(self.home), "--socket", str(self.socket), "--logs-dir",
             str(self.logs), "--secret-store", "file", "--discovery-relays", csv,
             "--default-account-relays", csv], self.env, self.home, daemon=True)
        end = min(processes.deadline, time.monotonic() + 20)
        while not self.socket.exists():
            require(time.monotonic() < end, "socket_timeout")
            processes.tick()
        require(stat.S_ISSOCK(self.socket.lstat().st_mode), "invalid_socket")
        def ready(value):
            require(type(value.get("running")) is bool)
            return True if value["running"] else None
        await_read(processes, lambda: self.call("daemon", "status"), ready)

    def call(self, *args):
        prefix = self.prefix + (["--account", self.account] if self.account else [])
        return response(self.processes.run(prefix + list(args), self.env, self.home))


def await_read(processes, read, validate):
    while True:
        result = validate(read())
        if result is not None:
            return result
        processes.pause(2)


def exchange(args, root, processes, report):
    report["stage"] = "binary_verification"
    wn = copy_binary(args.wn, root / "wn", args.wn_sha256)
    wnd = copy_binary(args.wnd, root / "wnd", args.wnd_sha256)
    report["binary_sha256"] = {"wn": args.wn_sha256, "wnd": args.wnd_sha256}
    report["stage"] = "version"
    version = processes.run([wn, "--version"], {"HOME": str(root), "LANG": "C", "RUST_LOG": "warn"}, root, 5)
    require(re.fullmatch(rb"wn [0-9]+\.[0-9]+\.[0-9]+[^\x00-\x1f]{0,80}\n?", version), "version_schema")
    report["wn_version"] = version.decode().strip()
    report["stage"] = "daemon_start"
    alice = Peer(root, "a", wn, wnd, args.relay, processes)
    bob = Peer(root, "b", wn, wnd, args.relay, processes)
    csv = ",".join(args.relay)
    report["public_identities"] = {}
    for name, peer in (("alice", alice), ("bob", bob)):
        report["stage"] = "identity_" + name
        value = peer.call("create-identity")
        peer.account = verify_identity(value, args.relay)
        report["public_identities"][name] = {"hex": peer.account, "npub": value["npub"]}
    require(alice.account != bob.account, "identities_equal")
    report["assertions"].append("distinct_signing_identities_and_explicit_relay_lists")
    report["stage"] = "key_package_fetch"
    def fetched(value):
        require(public_key(value.get("account_id")) == bob.account)
        require(type(value.get("key_package_bytes")) is int and value["key_package_bytes"] > 0)
        return True
    await_read(processes, lambda: alice.call("keys", "fetch", bob.account, "--bootstrap-relays", csv), fetched)
    report["stage"] = "group_create"
    created = alice.call("groups", "create", "marmish-smoke-" + secrets.token_hex(8),
                         bob.account, "--retention", "1d")
    group = identifier(created.get("group_id"))
    report["group_id"] = group
    report["stage"] = "invite"
    await_read(processes, lambda: bob.call("groups", "invites"),
               lambda v: matching_invite(v, group, alice.account))
    report["stage"] = "accept"
    accepted = bob.call("groups", "accept", group)
    require(accepted.get("accepted") is True and isinstance(accepted.get("group"), dict)
            and accepted["group"].get("group_id") == group)
    report["stage"] = "membership_and_retention"
    for peer in (alice, bob):
        verify_members(peer.call("groups", "members", group), (alice.account, bob.account), peer.account)
        retention = peer.call("groups", "retention", group)
        require(retention.get("group_id") == group and retention.get("enabled") is True
                and type(retention.get("disappearing_message_secs")) is int
                and retention["disappearing_message_secs"] == 86400, "retention_mismatch")
    report["assertions"].append("exact_two_member_room_and_one_day_retention")
    report["message_ids"] = []
    nonce = secrets.token_hex(16)
    for name, sender, recipient in (("alice_to_bob", alice, bob), ("bob_to_alice", bob, alice)):
        report["stage"] = "send_" + name
        payload = json.dumps({"type": "marmish-synthetic-smoke-v1", "nonce": nonce,
                              "direction": name}, separators=(",", ":"), sort_keys=True)
        ids = sent_ids(sender.call("messages", "send-event", group, str(KIND), payload))
        report["stage"] = "receive_" + name
        received = await_read(processes, lambda: recipient.call("messages", "list", group,
                              "--kind", str(KIND), "--limit", "20"),
                              lambda v: received_message(v, group, sender.account, payload, ids))
        report["message_ids"].append(received)
        report["assertions"].append("authenticated_incoming_" + name)
    report["outcome"] = "passed"
    report["stage"] = "complete"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live", action="store_true", required=True)
    for name in ("wn", "wnd", "wn-sha256", "wnd-sha256"):
        parser.add_argument("--" + name, required=True)
    parser.add_argument("--relay", action="append", required=True)
    parser.add_argument("--timeout", type=int, default=180)
    args = parser.parse_args(argv)
    report = {"outcome": "failed", "stage": "configuration", "assertions": [],
              "limitations": ["relay_lists_are_not_egress_enforcement",
                              "sender_authentication_trusts_selected_mdk_binary",
                              "binary_source_build_revision_unverified",
                              "public_keypackage_and_relay_list_residue_may_remain",
                              "retention_setting_is_not_deletion_certification",
                              "sigkill_and_escaped_descendants_are_not_contained"]}
    old_umask = os.umask(0o077)
    handlers = {}
    processes = None
    root = None
    try:
        require(sys.platform == "linux" and hasattr(os, "WNOWAIT"), "unsupported_platform")
        require(1 <= args.timeout <= 300, "invalid_timeout")
        args.relay = sorted({relay_url(r) for r in args.relay})
        require(1 <= len(args.relay) <= 8, "relay_count_limit")
        report["relays"] = args.relay
        root = Path(tempfile.mkdtemp(prefix="msh-", dir="/tmp"))
        processes = Processes(time.monotonic() + args.timeout)
        def interrupted(_signum, _frame):
            raise SmokeError("interrupted")
        for sig in (signal.SIGINT, signal.SIGTERM):
            handlers[sig] = signal.signal(sig, interrupted)
        exchange(args, root, processes, report)
    except SmokeError as error:
        report["error"] = str(error)
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        report["error"] = "local_io_or_schema_failure"
    finally:
        # Ignore repeated termination signals while cleaning owned groups/state.
        for sig in handlers:
            signal.signal(sig, signal.SIG_IGN)
        cleanup_failed = False
        if processes:
            try:
                processes.close()
            except OSError:
                cleanup_failed = True
        if root:
            try:
                shutil.rmtree(root)
            except OSError:
                cleanup_failed = True
        report["local_cleanup"] = "complete"
        if cleanup_failed:
            report.update(outcome="failed", local_cleanup="failed", error="cleanup_failure")
        for sig, handler in handlers.items():
            signal.signal(sig, handler)
        os.umask(old_umask)
    print(json.dumps(report, sort_keys=True))
    return 0 if report["outcome"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
