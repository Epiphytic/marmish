"""Offline RPC contract tests; all peers are temporary local fake Unix sockets."""

import copy
import json
import os
from pathlib import Path
import socket
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

import marmish_rpc as rpc
from test_mdk_smoke import identity

A, B, G, M = (digit * 64 for digit in "1234")
SID = "a" * 32
RELAYS = ["wss://relay.example/"]


def result(**fields):
    return {"account_id": A, "npub": A, **fields}


def wire(value):
    return rpc.encode({"code": 0, "stdout": rpc.encode({"ok": True, "result": value}).decode() + "\n",
                       "stderr": ""}) + b"\n"


class Peer:
    """Read through client half-close before sending; record exact request bytes."""

    def __init__(self, path, response, *, drip=False):
        self.request = b""
        self.response = response
        self.drip = drip
        self.server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.server.bind(str(path))
        self.server.listen(1)
        self.server.settimeout(2)
        self.thread = threading.Thread(target=self.run, daemon=True)

    def __enter__(self):
        self.thread.start()
        return self

    def run(self):
        try:
            with self.server.accept()[0] as connection:
                connection.settimeout(2)
                while True:
                    chunk = connection.recv(65536)
                    if not chunk:
                        break
                    self.request += chunk
                if self.drip:
                    for byte in self.response:
                        connection.sendall(bytes([byte]))
                        time.sleep(0.015)
                else:
                    connection.sendall(self.response)
        except (OSError, TimeoutError):
            pass

    def __exit__(self, *_args):
        self.thread.join(3)
        self.server.close()
        if self.thread.is_alive():
            raise AssertionError("fake socket did not terminate")


class SocketTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="mrpc-")
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name) / "s"

    def test_roundtrip_halfclose_and_wire_paths(self):
        with Peer(self.path, wire({"answer": 42})) as peer:
            self.assertEqual(rpc.MdkRpc(self.path).execute("Sync", account=A), {"answer": 42})
        self.assertTrue(peer.request.endswith(b"\n"))
        cli = json.loads(peer.request)["Execute"]["cli"]
        self.assertEqual((cli["home"], cli["socket"]), ("/state", "/state/s"))
        self.assertNotIn(str(self.path), peer.request.decode())

    def test_peer_uid_checked_before_request(self):
        with Peer(self.path, wire({})) as peer:
            with self.assertRaises(rpc.RpcError) as caught:
                rpc.MdkRpc(self.path, expected_uid=os.geteuid() + 1).execute("Sync")
        self.assertEqual(str(caught.exception), "wrong_peer_uid")
        self.assertFalse(caught.exception.uncertain)
        self.assertEqual(peer.request, b"")

    def test_absolute_deadline_bounds_drip_response(self):
        start = time.monotonic()
        with Peer(self.path, wire({}), drip=True):
            with self.assertRaises(rpc.RpcError) as caught:
                rpc.MdkRpc(self.path, timeout=0.08).execute("Sync")
        self.assertEqual(str(caught.exception), "rpc_timeout")
        self.assertTrue(caught.exception.uncertain)
        self.assertLess(time.monotonic() - start, 1)

    def test_expired_caller_deadline_never_sends(self):
        with self.assertRaises(rpc.RpcError) as caught:
            rpc.MdkRpc(self.path).execute("Sync", deadline=time.monotonic() - 1)
        self.assertFalse(caught.exception.uncertain)

    def test_response_limit_and_framing(self):
        for data, category in ((b"x" * 65, "response_too_large"),
                               (b"{}", "invalid_framing"),
                               (b"{}\n{}\n", "invalid_framing")):
            with self.subTest(category=category):
                if self.path.exists():
                    self.path.unlink()
                with patch.object(rpc, "RESPONSE_CAP", 64), Peer(self.path, data):
                    with self.assertRaises(rpc.RpcError) as caught:
                        rpc.MdkRpc(self.path).execute("Sync")
                self.assertEqual(str(caught.exception), category)

    def test_request_limit_checked_without_connection(self):
        with patch.object(rpc, "REQUEST_CAP", 32):
            with self.assertRaisesRegex(rpc.RpcError, "request_too_large"):
                rpc.MdkRpc(self.path).execute("Sync")


class DecodingTests(unittest.TestCase):
    def test_strict_both_json_layers(self):
        invalid = [b'{"x":1,"x":2}', b'{"x":NaN}', b'{"x":Infinity}',
                   b'{"x":1e999}', b'{"x":-1e999}', b'{}{}', b'\xff']
        for data in invalid:
            with self.subTest(data=data), self.assertRaises(rpc.RpcError):
                rpc.strict_json(data)
        for inner in ('{"ok":true,"ok":false,"result":{}}', '{"ok":true,"result":NaN}',
                      '{"ok":true,"result":{"x":1e999}}'):
            with self.assertRaises(rpc.RpcError):
                rpc.decode_response(rpc.encode({"code": 0, "stdout": inner, "stderr": ""}))
        self.assertEqual(rpc.strict_json('{"finite":1.25e2}'), {"finite": 125.0})

    def test_error_output_is_never_exposed(self):
        secret = "DO-NOT-PRINT-COMMAND-OR-KEY"
        data = rpc.encode({"code": 1, "stderr": secret, "stdout": json.dumps({
            "ok": False, "error": {"code": "key_package_not_found", "message": secret}})})
        with self.assertRaises(rpc.RpcError) as caught:
            rpc.decode_response(data)
        self.assertEqual(caught.exception.code, "key_package_not_found")
        self.assertNotIn(secret, str(caught.exception))
        with self.assertRaises(rpc.RpcError) as caught:
            rpc.decode_response(b'{"error":{"code":"server_busy","message":"secret"}}')
        self.assertEqual(caught.exception.code, "server_busy")

    def test_response_requires_code_ok_and_result_object(self):
        for value in ({"code": True, "stdout": "{}", "stderr": ""},
                      {"code": 1, "stdout": '{"ok":true,"result":{}}', "stderr": ""},
                      {"code": 0, "stdout": '{"ok":true,"result":[]}', "stderr": ""}):
            with self.assertRaises(rpc.RpcError):
                rpc.decode_response(rpc.encode(value))


class WrapperTests(unittest.TestCase):
    def setUp(self):
        self.adapter = rpc.MdkRpc("/unused/s")
        self.calls = []

    def respond(self, value):
        def execute(command, *, account=None, deadline=None):
            self.calls.append((command, account))
            return copy.deepcopy(value)
        self.adapter.execute = execute

    def test_send_captured_wn_execute_vector(self):
        # Captured offline from installed wn (c1f5a130...747e5), source inspected
        # at MDK37c248a. Only public synthetic fields; namespace paths normalized.
        self.respond(result(group_id=G, kind=30100, published=1, message_ids=[M]))
        payload = '{"argument":"quotes \\\" Unicode ☃ newline\\n dollar$ backtick`"}'
        self.assertEqual(self.adapter.send(A, G, payload), [M])
        command, account = self.calls[0]
        expected = {"Execute": {"cli": {"home": "/state", "socket": "/state/s", "relay": None,
            "daemon_discovery_relays": [], "daemon_default_account_relays": [],
            "secret_store": "File", "keychain_service": None, "account": A, "json": True,
            "command": {"Messages": {"command": {"SendEvent": {"group_id": G,
                "kind": 30100, "tags": ['["marmish","v1"]'], "content": [payload]}}}}}}}
        self.assertEqual(rpc.MdkRpc.request(command, account), expected)

    def test_identity_create_and_fetch_distinct_account_semantics(self):
        self.respond(identity(A))
        self.assertEqual(self.adapter.create_identity(RELAYS), A)
        self.assertEqual(self.calls[-1], ("CreateIdentity", None))
        self.respond({"account_id": B, "key_package_bytes": 42})
        self.adapter.fetch_keys(A, B, RELAYS)
        self.assertEqual(self.calls[-1], ({"Keys": {"command": {"Fetch": {
            "account": B, "bootstrap_relays": RELAYS}}}}, A))

    def test_group_create_show_invite_accept_and_members(self):
        profile = {"name": "marmish/v1/" + SID, "description": rpc.PURPOSE}
        members = [{"member_id": key, "npub": key, "local": key == A} for key in (A, B)]
        self.respond(result(group_id=G, profile=profile, members=members,
                            message_retention={"disappearing_message_secs": 86400}))
        self.adapter.create_group(A, B, SID)
        self.assertEqual(self.calls[-1][0]["Groups"]["command"]["Create"]["description"], rpc.PURPOSE)
        self.respond(result(group={"group_id": G, "profile": profile}))
        self.adapter.show_group(A, G)
        self.respond(result(group={"group_id": G, "profile": profile}, accepted=True))
        self.adapter.accept(A, G)
        self.respond(result(group_id=G, members=members))
        self.assertEqual(self.adapter.members(A, G), members)
        self.respond(result(invites=[{"group_id": G, "profile": profile,
                                     "welcomer_account_id": B, "pending_confirmation": True}]))
        self.assertEqual(self.adapter.invites(A)[0]["welcomer_account_id"], B)

    def test_history_no_top_level_group_and_capacity_policy_left_to_caller(self):
        rows = [{"message_id": f"{i:064x}", "group_id": G, "from": B, "kind": 30100,
                 "direction": "received", "plaintext": "{}", "tags": [rpc.TAG],
                 "recorded_at": 1, "received_at": 2} for i in range(100)]
        self.respond(result(messages=rows))
        self.assertEqual(len(self.adapter.history(A, G)), 100)
        query = self.calls[-1][0]["Messages"]["command"]["List"]
        self.assertEqual(query, {"group_id": G, "group": None, "kinds": [30100], "before": None,
            "before_message_id": None, "after": None, "after_message_id": None, "limit": 100})
        for field, bad in (("from", "bad"), ("group_id", M), ("direction", "incoming"),
                           ("kind", 9), ("tags", ["bad"]), ("recorded_at", True)):
            broken = copy.deepcopy(rows[:1])
            broken[0][field] = bad
            self.respond(result(messages=broken))
            with self.subTest(field=field), self.assertRaises(rpc.RpcError):
                self.adapter.history(A, G)

    def test_ordinary_invite_nullable_profile_reaches_purpose_policy(self):
        for profile in ({"name": "ordinary chat", "description": None},
                        {"name": None, "description": None}):
            invite = {"group_id": G, "profile": profile,
                      "welcomer_account_id": B, "pending_confirmation": True}
            self.respond(result(invites=[invite]))
            self.assertEqual(self.adapter.invites(A), [invite])
            with self.assertRaisesRegex(rpc.RpcError, "group_purpose_mismatch"):
                self.adapter._profile(invite, SID)
        for profile in ({"name": "chat"}, {"name": 42, "description": None}):
            with self.assertRaises(rpc.RpcError):
                self.adapter._profile({"profile": profile})

    def test_retention_expiry_and_explicit_custom_kind_read_marker(self):
        self.respond(result(group_id=G, disappearing_message_secs=86400, enabled=True,
                            message_retention={"disappearing_message_secs": 86400}))
        self.adapter.retention(A, G, 86400)
        self.assertEqual(self.calls[-1][0], {"Groups": {"command": {"Retention": {
            "group_id": G, "set": "86400s"}}}})
        self.respond(result(now_ms=100, pruned_messages=0, secrets_deleted=0, groups=[{
            "group_id": G, "status": "deferred_unread", "pruned_messages": 0, "secrets_deleted": 0}]))
        self.adapter.expiry(A)
        self.assertEqual(self.calls[-1][0], {"Messages": {"command": "SweepExpired"}})
        self.respond(result(group_id=G))
        self.adapter.mark_read(A, G, M)
        self.assertEqual(self.calls[-1][0], {"Chats": {"command": {"MarkRead": {
            "group": G, "message_id": M}}}})

    def test_history_preserves_mdk_oldest_first_newest_window(self):
        # MDK37c248a account_projection.rs app_messages_with_visibility selects
        # DESC LIMIT, then reverses into (recorded_at,message_id,insert_order) ASC.
        # Local received_at can differ; the adapter must not re-sort or reverse.
        rows = [{"message_id": f"{i:064x}", "group_id": G, "from": B, "kind": 30100,
                 "direction": "received", "plaintext": "{}", "tags": [rpc.TAG],
                 "recorded_at": i, "received_at": 300 - i} for i in range(101, 201)]
        self.respond(result(messages=rows))
        actual = self.adapter.history(A, G)
        self.assertEqual(actual, rows)
        self.assertEqual(actual[0]["recorded_at"], 101)
        self.assertEqual(actual[-1]["recorded_at"], 200)

    def test_payload_limits_and_uncertain_publication(self):
        self.respond(result(group_id=G, kind=30100, published=0, message_ids=[]))
        with self.assertRaisesRegex(rpc.RpcError, "message_not_published"):
            self.adapter.send(A, G, "{}")
        for payload in ('{"x":NaN}', '{"a":1,"a":2}', "[]", '{"x":"\ud800"}',
                        '"' + "x" * rpc.FRAME_CAP + '"'):
            with self.assertRaises(rpc.RpcError):
                self.adapter.send(A, G, payload)


if __name__ == "__main__":
    unittest.main()
