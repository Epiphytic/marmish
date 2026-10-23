"""Strict trusted-file policy loading; executable semantics live in Rust tests."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import marmish_app as app
from marmish_policy import FILE_CAP, load_policy
from mdk_smoke import SmokeError

KEY = '11' * 32
OTHER = '22' * 32


class PolicyLoadingTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'policy.json'

    def write(self, data):
        self.path.write_bytes(data if isinstance(data, bytes) else json.dumps(data).encode())
        self.path.chmod(0o600)

    def test_none_and_cumulative_entries_preserved(self):
        self.assertIsNone(load_policy(None, [KEY]))
        policy = {'version': 1, 'defaults': {'allow_exec': ['/usr/**'],
                  'deny_exec': ['/usr/bin/sudo']}, 'keys': [
                  {'principal': KEY, 'restrictions': {'allow_exec': ['/usr/bin/*']}},
                  {'principal': KEY, 'restrictions': {'deny_exec': ['/usr/bin/sh']}}]}
        self.write(policy)
        self.assertEqual(load_policy(self.path, [KEY]), policy)

    def test_absent_allow_differs_from_empty_allow(self):
        self.write({'version': 1, 'defaults': {'allow_exec': []}})
        self.assertEqual(load_policy(self.path, [KEY])['defaults'], {'allow_exec': []})
        self.write({'version': 1})
        self.assertEqual(load_policy(self.path, [KEY])['defaults'], {})

    def test_strict_json_and_schema_fail_with_fixed_category(self):
        cases = [b'{"version":true}', b'{"version":1,"version":1}',
                 b'{"version":1,"defaults":{"allow_exec":[],"allow_exec":["/**"]}}',
                 b'{"version":1,"unknown":null}', b'{"version":NaN}',
                 b'{"version":1e999}', b'\xef\xbb\xbf{"version":1}', b'\xff',
                 b'[' * 2000 + b']' * 2000, b' ' * (FILE_CAP + 1),
                 {'version': 2}, {'version': 1, 'keys': {}},
                 {'version': 1, 'keys': [{'principal': OTHER, 'restrictions': {}}]},
                 {'version': 1, 'keys': [{'principal': KEY, 'restrictions': {}, 'x': 1}]},
                 {'version': 1, 'defaults': {'allow_exec': ['relative']}},
                 {'version': 1, 'defaults': {'allow_exec': ['/bad\\escape']}},
                 {'version': 1, 'defaults': {'allow_exec': ['/dangling\\']}},
                 {'version': 1, 'defaults': {'allow_exec': ['/line\nbreak']}},
                 {'version': 1, 'defaults': {'allow_exec': ['/nul\x00']}},
                 {'version': 1, 'defaults': {'allow_exec': ['/\ud800']}},
                 {'version': 1, 'defaults': {'allow_exec': ['/' + 'x' * 1024]}},
                 {'version': 1, 'defaults': {'allow_exec': ['/**'] * 257}},
                 {'version': 1, 'keys': [{'principal': KEY, 'restrictions': {}}] * 129}]
        for data in cases:
            with self.subTest(data=repr(data)[:90]):
                self.write(data)
                with self.assertRaisesRegex(SmokeError, '^invalid_command_policy$'):
                    load_policy(self.path, [KEY])

    def test_file_trust_checks_and_no_fifo_block(self):
        self.write({'version': 1})
        self.path.chmod(0o666)
        with self.assertRaisesRegex(SmokeError, '^invalid_command_policy$'):
            load_policy(self.path, [KEY])
        self.path.unlink()
        self.path.symlink_to(Path(self.temp.name) / 'absent')
        with self.assertRaisesRegex(SmokeError, '^invalid_command_policy$'):
            load_policy(self.path, [KEY])
        self.path.unlink()
        os.mkfifo(self.path, 0o600)
        with self.assertRaisesRegex(SmokeError, '^invalid_command_policy$'):
            load_policy(self.path, [KEY])

    def test_invalid_config_stops_before_daemon(self):
        self.write({'version': True})
        args = ['-i', str(Path(self.temp.name) / 'identity'), '--relay', 'wss://relay.example/',
                '--wnd', '/unused/wnd', '--wnd-sha256', '00' * 32, '--allow', KEY,
                '--bridge', '/unused/bridge', '--worker', '/unused/worker', '--cwd', self.temp.name,
                '--command-policy', str(self.path)]
        with patch.object(app.os, 'geteuid', return_value=1000), patch.object(app, 'Profile') as profile:
            with self.assertRaisesRegex(SmokeError, '^invalid_command_policy$'):
                app.server_main(args)
            profile.assert_not_called()


if __name__ == '__main__':
    unittest.main()
