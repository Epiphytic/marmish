"""Offline tests. Fake executables never import MDK or contact a relay."""

import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import signal
import socket
import stat
import sys
import tempfile
import time
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

import mdk_smoke as smoke

A = "01" * 32
B = "02" * 32
GROUP = "03" * 32
MESSAGE = "04" * 32
RELAYS = ["wss://relay.example/"]


@contextlib.contextmanager
def simulated_linux_admission():
    with contextlib.ExitStack() as stack:
        stack.enter_context(patch.object(smoke, 'sys', SimpleNamespace(platform='linux')))
        if not hasattr(smoke.os, 'WNOWAIT'):
            stack.enter_context(patch.object(smoke.os, 'WNOWAIT', 0, create=True))
        yield


def identity(account=A):
    return {"account_id": account, "npub": account, "local_signing": True,
            "key_package": {"published": True, "bytes": 32},
            "relay_lists": {"complete": True, "missing": [], "default_relays": RELAYS,
                            "bootstrap_relays": RELAYS,
                            "nip65": {"kind": 10002, "relays": RELAYS, "read_relays": RELAYS, "write_relays": RELAYS}, "inbox": {"kind": 10050, "relays": RELAYS}}}


class ValidationTests(unittest.TestCase):
    def test_relay_canonicalization(self):
        self.assertEqual(smoke.relay_url("wss://RELAY.example:443"), RELAYS[0])
        self.assertEqual(smoke.relay_url("wss://relay.example:8443/"),
                         "wss://relay.example:8443/")

    def test_relay_rejects_ambiguous_forms(self):
        for value in ("ws://relay.example", "wss://a,b", "wss://user@a", "wss://a?",
                      "wss://a#", "wss://a/path", "wss://a:0", "wss://a:65536",
                      "wss://a:", "wss://a\n", "wss://a..b", "wss://-a.b",
                      "wss://a\\b", "wss://[::1]", "wss://é.example"):
            with self.subTest(value=value), self.assertRaises(smoke.SmokeError):
                smoke.relay_url(value)

    def test_response_rejects_raw_and_wrong_schema(self):
        for value in (b"secret bad text", b'{"result":[]}', b'{"ok":false,"result":{}}',
                      b'{"result":{}} trailing', b'[]'):
            with self.assertRaises(smoke.SmokeError):
                smoke.response(value)
        self.assertEqual(smoke.response(b'{"ok":true,"result":{"x":1}}'), {"x": 1})

    def test_identity_and_relay_validation(self):
        self.assertEqual(smoke.verify_identity(identity(), RELAYS), A)
        for mutate in (
            lambda v: v.update(local_signing=False),
            lambda v: v.update(local_signing=1),
            lambda v: v.update(npub=B),
            lambda v: v.update(key_package=None),
            lambda v: v["key_package"].update(published=1),
            lambda v: v["key_package"].update(bytes=True),
            lambda v: v["key_package"].update(bytes=0),
            lambda v: v["key_package"].update(bytes="32"),
            lambda v: v["relay_lists"].update(complete=False),
            lambda v: v["relay_lists"].update(complete=1),
            lambda v: v["relay_lists"].update(missing=["inbox"]),
            lambda v: v["relay_lists"]["nip65"].update(kind=10050),
            lambda v: v["relay_lists"]["inbox"].update(kind=10002),
            lambda v: v["relay_lists"]["nip65"].update(read_relays=["wss://decoy.example"]),
            lambda v: v["relay_lists"]["nip65"].update(write_relays=[]),
            lambda v: v["relay_lists"]["nip65"].update(write_relays="wss://relay.example"),
            lambda v: v["relay_lists"]["inbox"].update(relays=["wss://decoy.example"]),
            lambda v: v["relay_lists"].update(default_relays=["wss://decoy.example"]),
        ):
            value = identity()
            mutate(value)
            with self.assertRaises(smoke.SmokeError):
                smoke.verify_identity(value, RELAYS)

    def test_npub_checksum_and_key_shape(self):
        self.assertEqual(smoke.public_key(A), A)
        for value in (None, "nsec1" + "q" * 58, "npub1" + "q" * 58, ("ab" * 32).upper(), "ab"):
            with self.assertRaises(smoke.SmokeError):
                smoke.public_key(value)

    def test_npub_official_nip19_vector_and_changed_checksum(self):
        # Public-key vector from https://github.com/nostr-protocol/nips/blob/master/19.md
        npub = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6"
        expected = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
        self.assertEqual(smoke.public_key(npub), expected)
        with self.assertRaisesRegex(smoke.SmokeError, "invalid_public_identity"):
            smoke.public_key(npub[:-1] + "q")

    def test_invite_and_membership(self):
        invite = {"group_id": GROUP, "pending_confirmation": True, "welcomer_account_id": A}
        self.assertIsNone(smoke.matching_invite({"invites": []}, GROUP, A))
        self.assertEqual(smoke.matching_invite({"invites": [invite]}, GROUP, A), invite)
        for field, value in (("welcomer_account_id", B), ("welcomer_account_id", None),
                             ("pending_confirmation", False)):
            with self.assertRaises(smoke.SmokeError):
                smoke.matching_invite({"invites": [{**invite, field: value}]}, GROUP, A)
        with self.assertRaises(smoke.SmokeError):
            smoke.matching_invite({"invites": [invite, invite]}, GROUP, A)
        members = {"members": [{"member_id": A, "npub": A, "local": True},
                               {"member_id": B, "npub": B, "local": False}]}
        smoke.verify_members(members, (A, B), A)
        members["members"][1]["local"] = True
        with self.assertRaises(smoke.SmokeError):
            smoke.verify_members(members, (A, B), A)

    def test_receive_checks_exact_remote_record(self):
        record = {"message_id": MESSAGE, "group_id": GROUP, "direction": "received",
                  "from": A, "kind": smoke.KIND, "plaintext": "synthetic", "tags": []}
        self.assertEqual(smoke.received_message({"messages": [record]}, GROUP, A,
                                               "synthetic", [MESSAGE]), MESSAGE)
        for field, value in (("group_id", "aa"), ("direction", "sent"), ("from", B),
                             ("plaintext", "changed"), ("kind", 1), ("tags", None)):
            with self.subTest(field=field), self.assertRaises(smoke.SmokeError):
                smoke.received_message({"messages": [{**record, field: value}]},
                                       GROUP, A, "synthetic", [MESSAGE])
        self.assertIsNone(smoke.received_message({"messages": []}, GROUP, A, "x", [MESSAGE]))

    def test_send_publication_is_count_not_boolean(self):
        self.assertEqual(smoke.sent_ids({"published": 1, "message_ids": [MESSAGE]}), [MESSAGE])
        for count in (True, False, 0, -1, "1", None):
            with self.assertRaises(smoke.SmokeError):
                smoke.sent_ids({"published": count, "message_ids": [MESSAGE]})


@unittest.skipUnless(sys.platform == "linux", "Linux waitid ownership contract")
class ProcessTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="msh-test-")
        self.root = Path(self.temporary.name)
        self.processes = smoke.Processes(time.monotonic() + 5)

    def tearDown(self):
        self.processes.close()
        self.temporary.cleanup()

    def fake(self, body):
        path = self.root / "fake"
        path.write_text("#!" + sys.executable + "\n" + body)
        path.chmod(0o700)
        return str(path)

    def test_private_copy_hash_and_mismatch(self):
        fake = self.fake("print('fake')\n")
        digest = hashlib.sha256(Path(fake).read_bytes()).hexdigest()
        destination = self.root / "copy"
        smoke.copy_binary(fake, destination, digest)
        self.assertEqual(stat.S_IMODE(destination.stat().st_mode), 0o700)
        with self.assertRaisesRegex(smoke.SmokeError, "binary_hash_mismatch"):
            smoke.copy_binary(fake, self.root / "bad", "0" * 64)
        link = self.root / "link"
        link.symlink_to(fake)
        with self.assertRaises(OSError):
            smoke.copy_binary(str(link), self.root / "linked-copy", digest)

    def test_bounded_fake_output_and_schema(self):
        fake = self.fake("print('{\"ok\":true,\"result\":{\"fake\":true}}')\n")
        result = self.processes.run([fake], {}, self.root)
        self.assertEqual(smoke.response(result), {"fake": True})
        self.assertFalse(self.processes.children)
        self.fake("import os\nos.write(1,b'x'*(1024*1024+1))\n")
        with self.assertRaisesRegex(smoke.SmokeError, "output_limit"):
            self.processes.run([fake], {}, self.root)

    def test_timeout_and_process_group_cleanup(self):
        fake = self.fake("import os,time\nfrom pathlib import Path\n"
                         "pid=os.fork()\n"
                         "if pid==0: time.sleep(60)\n"
                         "Path('grandchild.pid').write_text(str(pid))\ntime.sleep(60)\n")
        with self.assertRaisesRegex(smoke.SmokeError, "call_timeout"):
            self.processes.run([fake], {}, self.root, timeout=0.5)
        child = self.processes.children[0]
        pid = child.pid
        grandchild = int((self.root / "grandchild.pid").read_text())
        # Record Linux process starttime so a later reused PID cannot fool the check.
        grandchild_stat = Path(f"/proc/{grandchild}/stat")
        before = grandchild_stat.read_text().rsplit(")", 1)[1].split()
        self.assertNotIn(before[0], ("Z", "X"))
        started = before[19]
        self.assertIsNone(child.returncode)
        self.processes.close()
        self.processes = smoke.Processes(time.monotonic() + 5)
        with self.assertRaises(ProcessLookupError):
            os.kill(pid, 0)
        self.assertIsNotNone(child.returncode)
        end = time.monotonic() + 2
        while True:
            try:
                after = grandchild_stat.read_text().rsplit(")", 1)[1].split()
            except FileNotFoundError:
                break
            if after[0] in ("Z", "X") or after[19] != started:
                break
            self.assertLess(time.monotonic(), end, "owned grandchild remained live after cleanup")
            time.sleep(0.01)

    def test_daemon_pipes_drained_and_log_watchdog(self):
        logs = self.root / "logs"
        logs.mkdir()
        self.processes.logs.append(logs)
        fake = self.fake("import os,time\nos.write(2,b'x'*200000)\ntime.sleep(60)\n")
        _child, buffers = self.processes.spawn([fake], {}, self.root, daemon=True)
        self.processes.pause(0.15)
        self.assertEqual(buffers, [bytearray(), bytearray()])
        (logs / "wnd.log").write_bytes(b"x" * (smoke.CAP + 1))
        with self.assertRaisesRegex(smoke.SmokeError, "daemon_log_limit"):
            self.processes.tick()

    def test_two_private_peer_homes_explicit_socket_and_clean_env(self):
        peers = []
        sockets = []
        observed = []
        processes = self.processes
        def spawn(args, env, cwd, daemon=False):
            observed.append((args, env, cwd, daemon))
            server = socket.socket(socket.AF_UNIX)
            server.bind(args[args.index("--socket") + 1])
            sockets.append(server)
            return object(), []
        poison = {"WN_SOCKET": "poison", "WN_ALLOW_LOOPBACK_RELAYS": "1",
                  "WN_DEV_SECRET": "poison", "HTTPS_PROXY": "poison", "PATH": "poison",
                  "SSL_CERT_FILE": "poison", "ANTHROPIC_API_KEY": "poison"}
        try:
            with patch.dict(os.environ, poison), patch.object(processes, "spawn", side_effect=spawn), \
                    patch.object(processes, "run", return_value=b'{"ok":true,"result":{"running":true}}') as readiness:
                for name in ("a", "b"):
                    peers.append(smoke.Peer(self.root, name, "/fake/wn", "/fake/wnd", RELAYS, processes))
                self.assertEqual(readiness.call_count, 2)
                self.assertTrue(all(call.args[0][-2:] == ["daemon", "status"]
                                    for call in readiness.call_args_list))
            self.assertNotEqual(peers[0].home, peers[1].home)
            self.assertNotEqual(peers[0].socket, peers[1].socket)
            for args, env, cwd, daemon in observed:
                self.assertTrue(daemon)
                self.assertEqual(args[args.index("--discovery-relays") + 1], RELAYS[0])
                self.assertEqual(args[args.index("--default-account-relays") + 1], RELAYS[0])
                self.assertEqual(env["HOME"], str(cwd))
                self.assertEqual(env["RUST_LOG"], "warn")
                self.assertFalse(set(poison) & set(env))
                self.assertEqual(stat.S_IMODE(cwd.stat().st_mode), 0o700)
            peer = peers[0]
            peer.account = A
            with patch.object(processes, "run", return_value=b'{"ok":true,"result":{}}') as run:
                peer.call("groups", "invites")
                args = run.call_args.args[0]
                self.assertIn("--socket", args)
                self.assertEqual(args[-4:], ["--account", A, "groups", "invites"])
        finally:
            for server in sockets:
                server.close()

    def test_retry_read_only_and_mutation_not_repeated(self):
        calls = []
        def read():
            calls.append(1)
            if len(calls) == 1:
                return {"ready": False}
            return {"ready": True}
        with patch.object(self.processes, "pause"):
            self.assertTrue(smoke.await_read(self.processes, read, lambda v: True if v["ready"] else None))
        self.assertEqual(len(calls), 2)
        with patch.object(self.processes, "run", side_effect=smoke.SmokeError("call_timeout")) as run:
            peer = object.__new__(smoke.Peer)
            peer.prefix, peer.account, peer.processes = ["/fake/wn", "--socket", "/private/s"], A, self.processes
            peer.env, peer.home = {}, self.root
            with self.assertRaisesRegex(smoke.SmokeError, "call_timeout"):
                peer.call("groups", "create", "fixed", B)
            self.assertEqual(run.call_count, 1)


class EntrypointTests(unittest.TestCase):
    def test_full_exchange_uses_source_contract_without_processes(self):
        calls = []
        sent = {}
        class FakePeer:
            def __init__(self, root, name, *_args):
                self.name = name
                self.account = None

            def call(self, *args):
                calls.append((self.name, args))
                key = A if self.name == "a" else B
                if args == ("create-identity",):
                    return identity(key)
                if args[:2] == ("keys", "fetch"):
                    return {"account_id": B, "key_package_bytes": 32}
                if args[:2] == ("groups", "create"):
                    return {"group_id": GROUP}
                if args == ("groups", "invites"):
                    return {"invites": [{"group_id": GROUP, "pending_confirmation": True,
                                         "welcomer_account_id": A}]}
                if args[:2] == ("groups", "accept"):
                    return {"accepted": True, "group": {"group_id": GROUP}}
                if args[:2] == ("groups", "members"):
                    return {"members": [{"member_id": k, "npub": k, "local": k == key}
                                        for k in (A, B)]}
                if args[:2] == ("groups", "retention"):
                    return {"group_id": GROUP, "enabled": True, "disappearing_message_secs": 86400}
                if args[:2] == ("messages", "send-event"):
                    sent.update(sender=key, payload=args[-1])
                    return {"published": 1, "message_ids": [MESSAGE]}
                if args[:2] == ("messages", "list"):
                    return {"messages": [{"message_id": MESSAGE, "group_id": GROUP,
                                          "direction": "received", "from": sent["sender"],
                                          "plaintext": sent["payload"], "kind": smoke.KIND, "tags": []}]}
                raise AssertionError(args)
        args = SimpleNamespace(wn="/fake/wn", wnd="/fake/wnd", wn_sha256="0"*64,
                               wnd_sha256="1"*64, relay=RELAYS)
        report = {"assertions": []}
        processes = SimpleNamespace(run=lambda *_args: b"wn 0.13.0\n")
        with patch.object(smoke, "Peer", FakePeer), \
                patch.object(smoke, "copy_binary", side_effect=lambda _src, dst, _digest: str(dst)), \
                patch.object(smoke.secrets, "token_hex", side_effect=lambda n: "a" * (2 * n)):
            smoke.exchange(args, Path("/private/test"), processes, report)
        self.assertEqual(report["outcome"], "passed")
        self.assertEqual(len(report["assertions"]), 4)
        self.assertEqual(sum(args == ("create-identity",) for _, args in calls), 2)
        self.assertEqual(sum(args[:2] == ("groups", "create") for _, args in calls), 1)
        self.assertEqual(sum(args[:2] == ("messages", "send-event") for _, args in calls), 2)
        payloads = [
            '{"direction":"' + direction + '","nonce":"' + "a" * 32
            + '","type":"marmish-synthetic-smoke-v1"}'
            for direction in ("alice_to_bob", "bob_to_alice")
        ]
        self.assertEqual(calls, [
            ("a", ("create-identity",)),
            ("b", ("create-identity",)),
            ("a", ("keys", "fetch", B, "--bootstrap-relays", RELAYS[0])),
            ("a", ("groups", "create", "marmish-smoke-" + "a" * 16, B, "--retention", "1d")),
            ("b", ("groups", "invites")),
            ("b", ("groups", "accept", GROUP)),
            ("a", ("groups", "members", GROUP)),
            ("a", ("groups", "retention", GROUP)),
            ("b", ("groups", "members", GROUP)),
            ("b", ("groups", "retention", GROUP)),
            ("a", ("messages", "send-event", GROUP, "30100", payloads[0])),
            ("b", ("messages", "list", GROUP, "--kind", "30100", "--limit", "20")),
            ("b", ("messages", "send-event", GROUP, "30100", payloads[1])),
            ("a", ("messages", "list", GROUP, "--kind", "30100", "--limit", "20")),
        ])

    def check_main_cleanup(self, *, error=None, interrupt=False, process_failure=False,
                           state_failure=False):
        args = ["--live", "--wn", "/fake/wn", "--wnd", "/fake/wnd", "--wn-sha256", "0"*64,
                "--wnd-sha256", "0"*64, "--relay", RELAYS[0]]
        saved_handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
        saved_umask = os.umask(0o077)
        os.umask(saved_umask)
        output = io.StringIO()
        close = Mock(side_effect=OSError("not_reported") if process_failure else None)
        manager = SimpleNamespace(close=close)
        def exchange(_args, root, _processes, report):
            self.assertEqual(os.umask(0o077), 0o077)
            self.assertEqual(stat.S_IMODE(root.stat().st_mode), 0o700)
            report["stage"] = "offline_fixture"
            if interrupt:
                signal.getsignal(signal.SIGTERM)(signal.SIGTERM, None)
            if error:
                raise error
            report.update(outcome="passed", stage="complete")
        # Actual temporary state; all daemon/CLI creation stays mocked.
        with tempfile.TemporaryDirectory(prefix="msh-finally-test-") as parent:
            root = Path(parent) / "private"
            def create_root(**_kwargs):
                root.mkdir(mode=0o700)
                return str(root)
            real_rmtree = smoke.shutil.rmtree
            with simulated_linux_admission(), \
                    patch.object(smoke.os, "waitid", create=True) as waitid, \
                    patch.object(smoke.tempfile, "mkdtemp", side_effect=create_root), \
                    patch.object(smoke, "Processes", return_value=manager), \
                    patch.object(smoke, "exchange", side_effect=exchange), \
                    patch.object(smoke.shutil, "rmtree", side_effect=(
                        OSError("not_reported") if state_failure else real_rmtree)) as remove, \
                    patch.object(smoke.subprocess, "Popen") as spawn, \
                    contextlib.redirect_stdout(output):
                code = smoke.main(args)
            waitid.assert_not_called()
            close.assert_called_once_with()
            remove.assert_called_once_with(root)
            spawn.assert_not_called()
            self.assertEqual(root.exists(), state_failure)
        for sig, handler in saved_handlers.items():
            self.assertIs(signal.getsignal(sig), handler)
        self.assertEqual(os.umask(saved_umask), saved_umask)
        report = json.loads(output.getvalue())
        self.assertNotIn("not_reported", output.getvalue())
        return code, report

    def test_main_success_finally_restores_handlers_umask_and_state(self):
        code, report = self.check_main_cleanup()
        self.assertEqual(code, 0)
        self.assertEqual(report["outcome"], "passed")
        self.assertEqual(report["local_cleanup"], "complete")

    def test_main_exchange_failure_still_cleans_and_reports(self):
        code, report = self.check_main_cleanup(error=smoke.SmokeError("call_timeout"))
        self.assertEqual(code, 1)
        self.assertEqual(report["outcome"], "failed")
        self.assertEqual(report["stage"], "offline_fixture")
        self.assertEqual(report["error"], "call_timeout")
        self.assertEqual(report["local_cleanup"], "complete")

    def test_main_interruption_still_cleans_and_reports(self):
        code, report = self.check_main_cleanup(interrupt=True)
        self.assertEqual(code, 1)
        self.assertEqual(report["error"], "interrupted")
        self.assertEqual(report["local_cleanup"], "complete")

    def test_main_process_cleanup_failure_still_removes_state(self):
        code, report = self.check_main_cleanup(process_failure=True)
        self.assertEqual(code, 1)
        self.assertEqual(report["outcome"], "failed")
        self.assertEqual(report["error"], "cleanup_failure")
        self.assertEqual(report["local_cleanup"], "failed")

    def test_main_state_cleanup_failure_report_and_restore(self):
        code, report = self.check_main_cleanup(state_failure=True)
        self.assertEqual(code, 1)
        self.assertEqual(report["outcome"], "failed")
        self.assertEqual(report["error"], "cleanup_failure")
        self.assertEqual(report["local_cleanup"], "failed")

    def test_missing_opt_in_or_required_configuration_spawns_nothing(self):
        for args in ([], ["--live"], ["--wn", "/bin/false"]):
            with patch.object(smoke.subprocess, "Popen") as spawn, contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    smoke.main(args)
                spawn.assert_not_called()

    def test_invalid_configuration_sanitized_without_spawn(self):
        args = ["--live", "--wn", "/secret/wn", "--wnd", "/secret/wnd", "--wn-sha256", "0"*64,
                "--wnd-sha256", "0"*64, "--relay", "wss://secret@bad"]
        output = io.StringIO()
        with simulated_linux_admission(), \
                patch.object(smoke.os, "waitid", create=True) as waitid, \
                patch.object(smoke, "Processes") as processes, \
                patch.object(smoke, "exchange") as exchange, \
                patch.object(smoke.subprocess, "Popen") as spawn, \
                contextlib.redirect_stdout(output):
            self.assertEqual(smoke.main(args), 1)
            spawn.assert_not_called()
            processes.assert_not_called()
            exchange.assert_not_called()
            waitid.assert_not_called()
        value = json.loads(output.getvalue())
        self.assertEqual(value["error"], "invalid_relay")
        self.assertNotIn("secret", output.getvalue())


class AdmissionSimulationTests(unittest.TestCase):
    def test_simulation_preserves_existing_wnowait_and_restores_missing_attribute(self):
        original_sys = smoke.sys
        for present in (False, True):
            sentinel = object()
            local_os = SimpleNamespace(WNOWAIT=sentinel) if present else SimpleNamespace()
            with self.subTest(present=present), patch.object(smoke, 'os', local_os):
                with simulated_linux_admission():
                    self.assertEqual(smoke.sys.platform, 'linux')
                    self.assertTrue(hasattr(local_os, 'WNOWAIT'))
                    if present:
                        self.assertIs(local_os.WNOWAIT, sentinel)
                self.assertEqual(hasattr(local_os, 'WNOWAIT'), present)
                if present:
                    self.assertIs(local_os.WNOWAIT, sentinel)
                self.assertIs(smoke.sys, original_sys)


class DarwinAdmissionTests(unittest.TestCase):
    def check_darwin_refusal(self, *, simulated):
        args = ["--live", "--wn", "/fake/wn", "--wnd", "/fake/wnd", "--wn-sha256", "0"*64,
                "--wnd-sha256", "0"*64, "--relay", RELAYS[0]]
        handlers = {sig: signal.getsignal(sig) for sig in (signal.SIGINT, signal.SIGTERM)}
        saved_umask = os.umask(0o077)
        os.umask(saved_umask)
        output = io.StringIO()
        admission = (patch.object(smoke, 'sys', SimpleNamespace(platform='darwin'))
                     if simulated else contextlib.nullcontext())
        with admission, patch.object(smoke.subprocess, 'Popen') as spawn, \
                patch.object(smoke.tempfile, 'mkdtemp') as allocate, \
                patch.object(smoke, 'Processes') as processes, \
                patch.object(smoke, 'exchange') as exchange, \
                patch.object(smoke.os, 'waitid', create=True) as waitid, \
                contextlib.redirect_stdout(output):
            self.assertEqual(smoke.main(args), 1)
            for operation in (spawn, allocate, processes, exchange, waitid):
                operation.assert_not_called()
        report = json.loads(output.getvalue())
        self.assertEqual(report['error'], 'unsupported_platform')
        self.assertEqual(report['outcome'], 'failed')
        self.assertEqual(report['local_cleanup'], 'complete')
        for sig, handler in handlers.items():
            self.assertIs(signal.getsignal(sig), handler)
        self.assertEqual(os.umask(saved_umask), saved_umask)

    def test_simulated_darwin_refusal_preserves_process_state(self):
        self.check_darwin_refusal(simulated=True)

    @unittest.skipUnless(sys.platform == 'darwin', 'actual Darwin smoke admission')
    def test_actual_darwin_refusal_preserves_process_state(self):
        # No platform or WNOWAIT admission patch: production sees actual Darwin.
        self.check_darwin_refusal(simulated=False)


if __name__ == "__main__":
    unittest.main()
