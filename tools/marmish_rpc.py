"""Bounded adapter for the pinned MDK daemon's private Unix JSON protocol.

The host socket is mounted as /state/s inside the daemon namespace. Payloads
travel only over that socket. This is an explicitly version-pinned prototype,
not a stable MDK API. Diagnostics never include daemon output or command data.
"""

import json
import math
import os
from pathlib import Path
import re
import socket

try:
    from . import mdk_smoke as smoke
    from .marmish_platform import continuous_time, verify_peer
except ImportError:
    import mdk_smoke as smoke
    from marmish_platform import continuous_time, verify_peer

KIND = 30100
PURPOSE = "marmish/v1 command transport"
FRAME_CAP = 32 * 1024
REQUEST_CAP = 256 * 1024
RESPONSE_CAP = 4 * 1024 * 1024
HISTORY_LIMIT = 100
TAG = ["marmish", "v1"]


class RpcError(Exception):
    """Fixed diagnostic plus whether a mutation might have reached MDK."""

    def __init__(self, category, *, code=None, uncertain=False):
        super().__init__(category)
        self.category = category
        self.code = code
        self.uncertain = uncertain


def require(condition, category="schema_mismatch"):
    if not condition:
        raise RpcError(category)


def _pairs(pairs):
    value = {}
    for key, item in pairs:
        require(key not in value, "invalid_json")
        value[key] = item
    return value


def _constant(_value):
    raise RpcError("invalid_json")


def _finite_float(value):
    number = float(value)
    require(math.isfinite(number), "invalid_json")
    return number


def strict_json(data):
    """Reject ambiguous duplicate keys and JSON extensions at both layers."""
    try:
        return json.loads(data, object_pairs_hook=_pairs, parse_constant=_constant,
                          parse_float=_finite_float)
    except (ValueError, UnicodeError, RecursionError):
        raise RpcError("invalid_json") from None


def encode(value):
    try:
        return json.dumps(value, ensure_ascii=False, allow_nan=False,
                          separators=(",", ":")).encode("utf-8")
    except (ValueError, TypeError, UnicodeError, RecursionError):
        raise RpcError("invalid_json") from None


def _frame_size(value):
    try:
        require(len(value.encode("utf-8")) <= FRAME_CAP, "frame_too_large")
    except UnicodeError:
        raise RpcError("invalid_json") from None


def _checked(function, *args):
    try:
        return function(*args)
    except smoke.SmokeError as error:
        raise RpcError(str(error)) from None


def public_key(value):
    return _checked(smoke.public_key, value)


def identifier(value):
    return _checked(smoke.identifier, value)


def _uint(value):
    require(type(value) is int and 0 <= value <= (1 << 64) - 1)
    return value


def _account_result(value, account):
    require(public_key(value.get("account_id")) == account)
    require(public_key(value.get("npub")) == account)
    return value


def _group_result(value, group):
    require(identifier(value.get("group_id")) == group)
    return value


def _error_code(value):
    if isinstance(value, dict):
        code = value.get("code")
        if isinstance(code, str) and re.fullmatch(r"[a-z][a-z0-9_]{0,63}", code):
            return code
    return None


def decode_response(data):
    outer = strict_json(data)
    require(isinstance(outer, dict), "invalid_response")
    if "error" in outer:
        raise RpcError("daemon_error", code=_error_code(outer["error"]))
    require(type(outer.get("code")) is int and isinstance(outer.get("stdout"), str)
            and isinstance(outer.get("stderr"), str), "invalid_response")
    inner = strict_json(outer["stdout"])
    require(isinstance(inner, dict), "invalid_response")
    if inner.get("ok") is False:
        raise RpcError("mdk_error", code=_error_code(inner.get("error")))
    require(outer["code"] == 0 and inner.get("ok") is True
            and isinstance(inner.get("result"), dict), "invalid_response")
    return inner["result"]


class MdkRpc:
    """One connection per command, one absolute deadline for all socket I/O."""

    def __init__(self, socket_path, *, timeout=10.0, expected_uid=None,
                 rpc_home="/state", rpc_socket="/state/s", peer_identity=None):
        require(isinstance(timeout, (int, float)) and not isinstance(timeout, bool)
                and math.isfinite(timeout) and timeout > 0, "invalid_timeout")
        self.socket_path = str(Path(socket_path))
        self.timeout = timeout
        for path in (rpc_home, rpc_socket):
            require(isinstance(path, str) and path.startswith("/") and "\0" not in path,
                    "invalid_rpc_path")
        self.rpc_home, self.rpc_socket = rpc_home, rpc_socket
        self.peer_identity = peer_identity
        self.expected_uid = os.geteuid() if expected_uid is None else expected_uid
        require(type(self.expected_uid) is int and self.expected_uid >= 0, "invalid_uid")

    @staticmethod
    def request(command, account=None, *, home="/state", socket_path="/state/s"):
        if account is not None:
            account = public_key(account)
        return {"Execute": {"cli": {
            "home": home, "socket": socket_path, "relay": None,
            "daemon_discovery_relays": [], "daemon_default_account_relays": [],
            "secret_store": "File", "keychain_service": None,
            "account": account, "json": True, "command": command,
        }}}

    def execute(self, command, *, account=None, deadline=None):
        payload = encode(self.request(command, account, home=self.rpc_home,
                                      socket_path=self.rpc_socket))
        require(len(payload) <= REQUEST_CAP, "request_too_large")
        end = continuous_time() + self.timeout
        if deadline is not None:
            require(isinstance(deadline, (int, float)) and math.isfinite(deadline),
                    "invalid_timeout")
            end = min(end, deadline)
        written = False
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
                self._timeout(connection, end)
                connection.connect(self.socket_path)
                _checked(verify_peer, connection, self.expected_uid, self.peer_identity)
                self._timeout(connection, end)
                # A partial send can be accepted even when sendall fails.
                written = True
                connection.sendall(payload + b"\n")
                connection.shutdown(socket.SHUT_WR)
                result = decode_response(self._receive(connection, end))
                return result
        except RpcError as error:
            error.uncertain = written
            raise
        except (TimeoutError, socket.timeout):
            raise RpcError("rpc_timeout", uncertain=written) from None
        except OSError:
            raise RpcError("rpc_io", uncertain=written) from None

    @staticmethod
    def _timeout(connection, end):
        remaining = end - continuous_time()
        require(remaining > 0, "rpc_timeout")
        connection.settimeout(remaining)

    def _receive(self, connection, end):
        data = bytearray()
        while True:
            self._timeout(connection, end)
            chunk = connection.recv(min(65536, RESPONSE_CAP + 1 - len(data)))
            self._timeout(connection, end)
            if not chunk:
                break
            data.extend(chunk)
            require(len(data) <= RESPONSE_CAP, "response_too_large")
        require(data.endswith(b"\n") and b"\n" not in data[:-1], "invalid_framing")
        return data[:-1]

    def _operation(self, namespace, command, account):
        account = public_key(account)
        value = self.execute({namespace: {"command": command}}, account=account)
        try:
            return _account_result(value, account)
        except RpcError as error:
            error.uncertain = True
            raise

    def create_identity(self, relays):
        relays = self._relays(relays)
        value = self.execute("CreateIdentity")
        return _checked(smoke.verify_identity, value, relays)

    @staticmethod
    def _relays(relays):
        require(isinstance(relays, (list, tuple)) and 1 <= len(relays) <= 16,
                "invalid_relay")
        return list(dict.fromkeys(_checked(smoke.relay_url, relay) for relay in relays))

    def sync(self, account):
        account = public_key(account)
        value = _account_result(self.execute("Sync", account=account), account)
        require(isinstance(value.get("messages"), list)
                and isinstance(value.get("joined_groups"), list))
        return value

    def fetch_keys(self, account, peer, relays):
        peer = public_key(peer)
        value = self.execute({"Keys": {"command": {"Fetch": {
            "account": peer, "bootstrap_relays": self._relays(relays),
        }}}}, account=account)
        require(public_key(value.get("account_id")) == peer)
        require(_uint(value.get("key_package_bytes")) > 0)
        return value

    def create_group(self, account, peer, session_id, retention="1d"):
        require(isinstance(session_id, str) and re.fullmatch(r"[0-9a-f]{32}", session_id),
                "invalid_session")
        require(isinstance(retention, str) and re.fullmatch(r"[0-9]{1,20}[smhdw]?", retention),
                "invalid_retention")
        unit = retention[-1]
        scale = {"s": 1, "m": 60, "h": 3600, "d": 86400, "w": 604800}
        seconds = int(retention[:-1]) * scale[unit] if unit in scale else int(retention)
        _uint(seconds)
        value = self._operation("Groups", {"Create": {
            "name": "marmish/v1/" + session_id, "members": [public_key(peer)],
            "description": PURPOSE, "retention": retention,
            "image": None, "image_media_type": None,
        }}, account)
        identifier(value.get("group_id"))
        _checked(smoke.verify_members, value, [public_key(account), public_key(peer)],
                 public_key(account))
        self._profile(value, session_id)
        policy = value.get("message_retention")
        require(isinstance(policy, dict)
                and type(policy.get("disappearing_message_secs")) is int
                and policy["disappearing_message_secs"] == seconds, "retention_mismatch")
        return value

    @staticmethod
    def _profile(group, session_id=None):
        profile = group.get("profile")
        require(isinstance(profile, dict)
                and all(field in profile and (profile[field] is None
                        or isinstance(profile[field], str))
                        for field in ("name", "description")))
        if session_id is not None:
            require(profile["name"] == "marmish/v1/" + session_id
                    and profile["description"] == PURPOSE, "group_purpose_mismatch")

    def show_group(self, account, group):
        group = identifier(group)
        value = self._operation("Groups", {"Show": {"group_id": group}}, account)
        require(isinstance(value.get("group"), dict))
        self._profile(_group_result(value["group"], group))
        return value

    def members(self, account, group):
        group = identifier(group)
        value = self._operation("Groups", {"Members": {"group_id": group}}, account)
        _group_result(value, group)
        members = value.get("members")
        require(isinstance(members, list) and len(members) <= 100)
        found = set()
        for member in members:
            require(isinstance(member, dict))
            key = public_key(member.get("member_id"))
            require(key not in found and public_key(member.get("npub")) == key
                    and member.get("local") is (key == public_key(account)))
            found.add(key)
        return members

    def invites(self, account):
        value = self._operation("Groups", "Invites", account)
        invites = value.get("invites")
        require(isinstance(invites, list) and len(invites) <= 200)
        for invite in invites:
            require(isinstance(invite, dict))
            identifier(invite.get("group_id"))
            public_key(invite.get("welcomer_account_id"))
            require(invite.get("pending_confirmation") is True)
            self._profile(invite)
        return invites

    def accept(self, account, group):
        group = identifier(group)
        value = self._operation("Groups", {"Accept": {"group_id": group}}, account)
        require(value.get("accepted") is True and isinstance(value.get("group"), dict))
        self._profile(_group_result(value["group"], group))
        return value

    def send(self, account, group, payload):
        require(isinstance(payload, str), "invalid_payload")
        _frame_size(payload)
        require(isinstance(strict_json(payload), dict), "invalid_payload")
        group = identifier(group)
        # A successful MDK execution with an unrecognized result cannot prove
        # non-delivery either. Preserve uncertainty through wrapper validation.
        value = self._operation("Messages", {"SendEvent": {
            "group_id": group, "kind": KIND, "tags": [encode(TAG).decode()],
            "content": [payload],
        }}, account)
        try:
            _group_result(value, group)
            require(type(value.get("kind")) is int and value["kind"] == KIND)
            return _checked(smoke.sent_ids, value)
        except RpcError as error:
            error.uncertain = True
            raise

    def history(self, account, group):
        group = identifier(group)
        value = self._operation("Messages", {"List": {
            "group_id": group, "group": None, "kinds": [KIND],
            "before": None, "before_message_id": None,
            "after": None, "after_message_id": None, "limit": HISTORY_LIMIT,
        }}, account)
        messages = value.get("messages")
        require(isinstance(messages, list) and len(messages) <= HISTORY_LIMIT)
        ids = set()
        for row in messages:
            self._message(row, group)
            require(row["message_id"] not in ids)
            ids.add(row["message_id"])
        return messages

    @staticmethod
    def _message(row, group):
        require(isinstance(row, dict))
        _group_result(row, group)
        identifier(row.get("message_id"))
        public_key(row.get("from"))
        require(row.get("direction") in ("sent", "received")
                and type(row.get("kind")) is int and row["kind"] == KIND
                and isinstance(row.get("plaintext"), str)
                and isinstance(row.get("tags"), list))
        _frame_size(row["plaintext"])
        for tag in row["tags"]:
            require(isinstance(tag, list) and all(isinstance(item, str) for item in tag))
        _uint(row.get("recorded_at"))
        _uint(row.get("received_at"))

    def retention(self, account, group, seconds=None):
        group = identifier(group)
        setting = None if seconds is None else str(_uint(seconds)) + "s"
        value = self._operation("Groups", {"Retention": {
            "group_id": group, "set": setting,
        }}, account)
        _group_result(value, group)
        actual = _uint(value.get("disappearing_message_secs"))
        require(value.get("enabled") is (actual != 0))
        require(isinstance(value.get("message_retention"), dict)
                and value["message_retention"].get("disappearing_message_secs") == actual)
        require(seconds is None or actual == seconds, "retention_mismatch")
        return value

    def expiry(self, account):
        value = self._operation("Messages", "SweepExpired", account)
        for field in ("now_ms", "pruned_messages", "secrets_deleted"):
            _uint(value.get(field))
        groups = value.get("groups")
        require(isinstance(groups, list) and len(groups) <= 200)
        for group in groups:
            require(isinstance(group, dict))
            identifier(group.get("group_id"))
            require(group.get("status") in (
                "no_expired_messages", "pruned", "deferred_clock_skew", "deferred_unread",
                "deferred_scan_exhausted", "failed"))
            _uint(group.get("pruned_messages"))
            _uint(group.get("secrets_deleted"))
        return value

    def mark_read(self, account, group, message_id):
        group = identifier(group)
        value = self._operation("Chats", {"MarkRead": {
            "group": group, "message_id": identifier(message_id),
        }}, account)
        return _group_result(value, group)
