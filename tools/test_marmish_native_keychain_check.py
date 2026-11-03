"""Offline supervisor contracts; real native provider success is a CI gate."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

import marmish_native_keychain_check as check


class InstalledProviderSupervisor(unittest.TestCase):
    def test_product_timeout_reaps_only_its_owned_group(self):
        original = check.subprocess.Popen
        children = []
        def start(*args, **kwargs):
            child = original(*args, **kwargs)
            children.append(child)
            return child
        with mock.patch.object(check.subprocess, 'Popen', side_effect=start), \
                mock.patch.object(check.os, 'killpg', wraps=check.os.killpg) as kill:
            with self.assertRaisesRegex(check.Refused, 'operation_timeout'):
                check.execute(['/bin/sh', '-c', 'exec /bin/sleep 5'], {}, timeout=0.05)
        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll())
        self.assertTrue(all(call.args[0] == children[0].pid for call in kill.call_args_list))

    def test_completed_leader_descendant_is_failure_after_owned_cleanup(self):
        child = mock.Mock(pid=98765)
        child.poll.return_value = 0
        # Group initially remains, is signalled, then disappears after TERM.
        with mock.patch.object(check.os, 'killpg', side_effect=[None, None, ProcessLookupError]) as kill:
            with self.assertRaisesRegex(check.Refused, 'owned_descendant_remained'):
                check.stop(child)
        self.assertEqual([call.args[0] for call in kill.call_args_list], [98765] * 3)
        child.wait.assert_not_called()

    def test_naturally_completed_descendant_exit_before_signal_still_fails(self):
        child = mock.Mock(pid=98765)
        child.poll.return_value = 0
        with mock.patch.object(check.os, 'killpg', side_effect=[None, ProcessLookupError]) as kill:
            with self.assertRaisesRegex(check.Refused, 'owned_descendant_remained'):
                check.stop(child)
        self.assertEqual([call.args[0] for call in kill.call_args_list], [98765] * 2)

    def test_capture_limit_is_enforced_while_child_runs(self):
        with mock.patch.object(check, 'MAX_CAPTURE', 16):
            with self.assertRaisesRegex(check.Refused, 'operation_output_limit'):
                check.execute(['/bin/sh', '-c', 'printf 12345678901234567890'], {}, timeout=1)

    def test_helper_only_requests_public_metadata(self):
        qualification = check.Qualification.__new__(check.Qualification)
        qualification.bins, qualification.environment = Path('/owned/bin'), {}
        value = {'version': 1, 'ok': True, 'result': {'items': []}}
        with mock.patch.object(check, 'execute', return_value=(0, json.dumps(value).encode())) as run:
            self.assertEqual(qualification.helper('list'), value)
            request = json.loads(run.call_args.args[3])
            self.assertEqual(request, {'version': 1, 'action': 'list', 'namespace': 'marmish-v2'})
            with self.assertRaisesRegex(check.Refused, 'forbidden_helper_operation'):
                qualification.helper('load')
            self.assertEqual(run.call_count, 1)

    def test_profile_refuses_file_fallback_and_shared_reference(self):
        with tempfile.TemporaryDirectory() as tmp:
            qualification = check.Qualification.__new__(check.Qualification)
            qualification.state = Path(tmp)
            directory = qualification.state / 'default'
            directory.mkdir()
            manifest = {'backend': 'keychain', 'phase': 'ready', 'nostr_ref': 'first', 'sqlcipher_ref': 'second'}
            path = directory / 'profile.json'
            path.write_text(json.dumps(manifest))
            self.assertEqual(qualification.manifest('default'), manifest)
            (directory / 'keys').mkdir()
            with self.assertRaisesRegex(check.Refused, 'plaintext_fallback'):
                qualification.manifest('default')
            (directory / 'keys').rmdir()
            path.write_text(json.dumps(dict(manifest, sqlcipher_ref='first')))
            with self.assertRaisesRegex(check.Refused, 'separate_key_references'):
                qualification.manifest('default')

    def test_linux_restart_reaps_then_waits_for_bus_release_before_new_owner(self):
        provider = check.LinuxProvider(Path('/owned'), {})
        old, fresh = mock.Mock(), mock.Mock()
        provider.keyring = old
        calls = []
        with mock.patch.object(check, 'stop', side_effect=lambda child: calls.append(('stop', child))), \
                mock.patch.object(provider, 'wait_service', side_effect=lambda present: calls.append(('bus', present))), \
                mock.patch.object(check.subprocess, 'Popen', side_effect=lambda *a, **kw: calls.append(('start', kw)) or fresh):
            provider.lock()
        self.assertEqual(calls[:2], [('stop', old), ('bus', False)])
        self.assertEqual(calls[2][0], 'start')
        self.assertEqual(calls[2][1]['stdin'], subprocess.DEVNULL)
        self.assertEqual(calls[3], ('bus', True))

    def test_staged_inputs_must_match_before_any_provider_construction(self):
        with tempfile.TemporaryDirectory() as tmp:
            staging = Path(tmp)
            (staging / 'asset').write_bytes(b'changed')
            pins = staging / 'pins.json'
            pins.write_text(json.dumps({'asset': '0' * 64}))
            with mock.patch.object(check, 'Qualification') as constructor:
                with self.assertRaisesRegex(check.Refused, 'staged_input_changed'):
                    check.inner(staging / 'bootstrap', staging, staging / 'report', pins)
                constructor.assert_not_called()


if __name__ == '__main__':
    unittest.main()
