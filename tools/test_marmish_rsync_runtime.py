#!/usr/bin/env python3
import os
from pathlib import Path
import shlex
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

import marmish_rsync_runtime as rsync


class OracleTests(unittest.TestCase):
    def test_literal_names_order_independent_and_links_not_followed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            outside = root / 'outside'
            outside.write_bytes(b'outside one')
            left, right = root / 'left', root / 'right'
            left.mkdir(); right.mkdir()
            names = ['z/line\nname', "a/quote'$()", 'a/empty']
            for tree, order in ((left, names), (right, list(reversed(names)))):
                for name in order:
                    path = tree / name
                    path.parent.mkdir(exist_ok=True)
                    path.write_bytes(name.encode())
                (tree / 'link').symlink_to(outside)
            first = rsync.tree_digest(left)
            self.assertEqual(first, rsync.tree_digest(right))
            outside.write_bytes(b'changed external content')
            self.assertEqual(first, rsync.tree_digest(left))
            (right / names[0]).write_bytes(b'changed')
            self.assertNotEqual(first, rsync.tree_digest(right))

    def test_filename_changes_affect_hash_even_when_contents_equal(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            path = root / 'first'
            path.write_bytes(b'')
            before = rsync.tree_digest(root)
            path.rename(root / 'second')
            self.assertNotEqual(before, rsync.tree_digest(root))


class HarnessTests(unittest.TestCase):
    def test_remote_shell_quoting_keeps_paths_and_relay_literal(self):
        qualification = rsync.RsyncQualification.__new__(rsync.RsyncQualification)
        qualification.root = Path("/private/some 'directory")
        qualification.options = SimpleNamespace(marmish=Path('/bin/marmish with spaces'),
                                                relay='wss://relay.example/')
        qualification.rsync = '/usr/bin/rsync'
        command = shlex.split(qualification.shell(compressed=True))
        self.assertEqual(command[0], '/bin/marmish with spaces')
        self.assertIn(str(qualification.root / 'state'), command)
        self.assertEqual(command[-2:], ['rsh', '--compress'])
        self.assertIn('--no-autostart', command)
        self.assertEqual(qualification.command('rsync', '-as'), ['/usr/bin/rsync', '-as'])

    def test_missing_rsync_fails_inside_managed_execution(self):
        qualification = rsync.RsyncQualification.__new__(rsync.RsyncQualification)
        qualification.rsync = None
        with self.assertRaises(rsync.QualificationError) as error:
            with patch.object(rsync.Qualification, 'identify_binary'):
                qualification.identify_binary()
        self.assertEqual(error.exception.category, 'rsync_unavailable')


    def test_transfer_checks_tree_and_collects_literal_not_total_transfer_bytes(self):
        qualification = rsync.RsyncQualification.__new__(rsync.RsyncQualification)
        qualification.report = {'results': []}
        qualification.shell = MagicMock(return_value='literal shell')
        qualification.run = MagicMock(return_value=SimpleNamespace(returncode=0,
            stdout=b'Literal data: 1,024 bytes\nMatched data: 2,048 bytes\n'))
        with patch.object(rsync.time, 'monotonic', side_effect=[1, 3]), \
             patch.object(rsync, 'tree_digest', return_value=('hash', 3072, 4)):
            item = qualification.transfer('delta', 'source/', 'remote:dest/', compressed=True,
                extra=('-z',), expected=(Path('dest'), ('hash', 3072, 4)))
        self.assertEqual(item['literal_bytes'], 1024)
        self.assertEqual(item['matched_bytes'], 2048)
        self.assertEqual(item['tree_bytes_per_second'], 1536)
        self.assertTrue(item['marmish_compression'] and item['rsync_compression'])
        self.assertIn('--stats', qualification.run.call_args.args)
        self.assertIn('-z', qualification.run.call_args.args)
        with patch.object(rsync, 'tree_digest', return_value=('wrong', 3072, 4)), \
             self.assertRaises(rsync.QualificationError) as failure:
            qualification.transfer('delta', 'source/', 'remote:dest/', expected=(Path('dest'), ('hash', 3072, 4)))
        self.assertEqual(failure.exception.category, 'tree_mismatch')

    def test_custom_main_case_hook_is_managed_and_default_cases_not_run(self):
        qualification = MagicMock()
        qualification.report = {'passed': False}
        qualification.initialize.return_value = 'remote'
        qualification.cleanup.return_value = True
        factory = MagicMock(return_value=qualification)
        cases = MagicMock()
        runtime = __import__('marmish_stream_runtime')
        with patch.object(runtime, 'arguments', return_value=SimpleNamespace(output=Path('report'))), \
             patch.object(runtime.signal, 'signal'), patch.object(runtime, 'write_report'), \
             patch.object(runtime.shutil, 'rmtree'), patch('builtins.print'):
            self.assertEqual(runtime.main(factory, cases), 0)
        cases.assert_called_once_with(qualification, 'remote')
        qualification.binary_roundtrip.assert_not_called()
        qualification.cleanup.assert_called_once()


if __name__ == '__main__':
    unittest.main()
