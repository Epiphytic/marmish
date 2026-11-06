"""Real local rsync bootstrap captures; no Marmish binary or network is used.

Run: python3 -m unittest discover -s tools -p test_marmish_rsync_capture.py -v
Old macOS system rsync and missing installations skip with an explicit reason.
Capture-peer disconnects are expected rsync exit 12, not successful transfers.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import unittest


class RsyncCaptureTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rsync = shutil.which("rsync")
        if cls.rsync is None:
            raise unittest.SkipTest("modern rsync is not installed")
        try:
            version = subprocess.run([cls.rsync, "--version"], capture_output=True,
                                     timeout=5, check=True).stdout.decode("ascii", "replace")
        except (OSError, subprocess.SubprocessError):
            raise unittest.SkipTest("cannot inspect installed rsync version") from None
        match = re.search(r"rsync\s+version\s+(\d+)\.(\d+)\.(\d+)", version)
        if match is None or tuple(map(int, match.groups())) < (3, 2, 0):
            raise unittest.SkipTest("requires maintained rsync >=3.2; old macOS system rsync is unsupported")
        cls.version = tuple(map(int, match.groups()))
        cls.has_iconv = "iconv" in version and "no iconv" not in version

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="marmish-rsync-capture-")
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.root.chmod(0o700)
        self.capture_path = self.root / "capture.json"
        self.capture = self.root / "capture"
        # Sole controlled remote-shell child writes argv then exits. It never
        # invokes a shell, rsync, another child, or a network operation.
        self.capture.write_text(
            "#!" + sys.executable + "\n"
            "import json, os, sys\n"
            "with open(os.environ['CAPTURE_PATH'], 'x', encoding='utf-8') as output:\n"
            "    json.dump(sys.argv[1:], output)\n"
        )
        self.capture.chmod(0o700)
        (self.root / "source").mkdir(mode=0o700)
        (self.root / "source" / "sample").write_bytes(b"local fixture\0")
        self.environment = {key: os.environ[key] for key in ("PATH", "LANG", "LC_ALL") if key in os.environ}
        self.environment.update(HOME=str(self.root), TMPDIR=str(self.root), CAPTURE_PATH=str(self.capture_path))

    def invoke(self, source="source/", destination="npub1test@relay.example:destination/", *,
               options=("-as",), rsh_options=(), expect_capture=True):
        remote_shell = " ".join(shlex.quote(value) for value in (str(self.capture), *rsh_options))
        process = subprocess.Popen([self.rsync, *options, "-e", remote_shell, source, destination],
                                   cwd=self.root, env=self.environment, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        try:
            try:
                _, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                # communicate has not reaped the leader on timeout; its owned
                # process group is still anchored while cleanup signals it.
                os.killpg(process.pid, signal.SIGKILL)
                process.communicate(timeout=5)
                self.fail("local rsync capture timed out")
            if expect_capture:
                self.assertEqual(process.returncode, 12, "capture peer intentionally exits before protocol exchange")
                self.assertTrue(self.capture_path.is_file(), "rsync did not invoke its remote-shell child")
                captured = json.loads(self.capture_path.read_text())
                self.assertIsInstance(captured, list)
                self.assertTrue(all(isinstance(value, str) for value in captured))
                return captured
            self.assertNotEqual(process.returncode, 0)
            self.assertFalse(self.capture_path.exists())
            self.assertIn(b"--old-args", stderr)
            return None
        finally:
            if process.returncode is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=5)
            for pipe in (process.stdout, process.stderr):
                pipe.close()

    def assert_supported(self, captured, prefix, *, sender=False, iconv=None, executable="rsync"):
        self.assertEqual(captured[:len(prefix)], prefix)
        tail = captured[len(prefix):]
        self.assertEqual(tail[:2], [executable, "--server"])
        index = 2
        if sender:
            self.assertEqual(tail[index], "--sender")
            index += 1
        cluster = tail[index]
        self.assertRegex(cluster, r"^-[A-Za-z0-9.]+$")
        self.assertIn("s", cluster[1:].split("e", 1)[0], "secluded s must precede capability suffix")
        self.assertEqual(tail[index + 1:], [] if iconv is None else ["--iconv=" + iconv])
        # Exact previously reviewed fixture for the locally grounded version.
        if self.version == (3, 2, 7):
            self.assertEqual(cluster, "-slogDtpre.iLsfxCIvu")

    def test_push_with_user_field_generates_login_prefix(self):
        self.assert_supported(self.invoke(), ["-l", "npub1test", "relay.example"])

    def test_pull_adds_sender_and_preserves_login_prefix(self):
        self.assert_supported(self.invoke(source="npub1test@relay.example:source/", destination="destination/"),
                              ["-l", "npub1test", "relay.example"], sender=True)

    def test_bare_npub_is_host_and_explicit_l_stays_adapter_option(self):
        self.assert_supported(self.invoke(destination="npub1test:destination/", rsh_options=("-l", "remote-alias")),
                              ["-l", "remote-alias", "npub1test"])

    def test_bare_npub_without_l_uses_no_remote_login(self):
        self.assert_supported(self.invoke(destination="npub1test:destination/"), ["npub1test"])

    def test_secluded_paths_and_remote_options_are_absent_from_bootstrap(self):
        self.assert_supported(self.invoke(destination="npub1test@relay.example:-space ;$(touch BAD)/",
                                         options=("-as", "-M--temp-dir=/tmp/a b")),
                              ["-l", "npub1test", "relay.example"])
        self.assertFalse((self.root / "BAD").exists())

    def test_iconv_remote_charset_is_literal_supported_suffix(self):
        if not self.has_iconv:
            self.skipTest("installed rsync was built without iconv")
        for local, remote in (("utf8", "utf8"), ("UTF-8,ISO-8859-1", "ISO-8859-1"), (".", ".")):
            with self.subTest(iconv=local):
                self.capture_path.unlink(missing_ok=True)
                self.assert_supported(self.invoke(options=("-as", "--iconv=" + local)),
                                      ["-l", "npub1test", "relay.example"], iconv=remote)

    def test_non_secluded_capability_s_does_not_enable_secluded_arguments(self):
        captured = self.invoke(options=("-a",), destination="npub1test@relay.example:space x;$(touch BAD)")
        tail = captured[3:]
        self.assertEqual(tail[:2], ["rsync", "--server"])
        cluster = tail[2]
        self.assertNotIn("s", cluster[1:].split("e", 1)[0])
        self.assertIn("s", cluster, "capability suffix has a misleading s")
        self.assertEqual(tail[-2], ".")
        self.assertIn("space", tail[-1])
        self.assertFalse((self.root / "BAD").exists())

    def test_old_args_and_secluded_args_conflict_before_remote_shell(self):
        self.invoke(options=("-as", "--old-args"), expect_capture=False)


if __name__ == "__main__":
    unittest.main()
