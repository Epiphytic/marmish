"""Mock-only contracts for the temporary encrypted legacy diagnostic."""
import contextlib
import hashlib
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest


class ConfidentialTraceWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.repository = Path(__file__).resolve().parent.parent
        self.enterContext(contextlib.chdir(self.repository))
        self.text = (self.repository / '.github/workflows/legacy-confidential-trace.yml').read_text()
        self.steps = re.split(r'^      - ', self.text, flags=re.M)[1:]

    def capture_shell(self):
        step = next(step for step in self.steps if step.startswith('name: Capture encrypted failure evidence\n'))
        return '\n'.join(line[10:] for line in step.split('        run: |\n', 1)[1].splitlines()) + '\n'

    def mocked_capture(self, *, status=1, trace='present', crypto='ok', runner='Linux', capability='ok'):
        temporary = self.enterContext(tempfile.TemporaryDirectory())
        root = Path(temporary)
        binaries = root / 'bin'
        binaries.mkdir()
        python = binaries / 'python3'
        python.write_text('#!' + sys.executable + '\n' + '''import os, sys
from pathlib import Path
if len(sys.argv) > 1 and sys.argv[1] == 'tools/marmish_managed_runtime_spike.py':
    expected = '1' if os.environ['RUNNER_OS'] == 'Linux' else None
    if os.environ.get('MARMISH_LEGACY_SKIP_NAMESPACE_TESTS') != expected:
        raise SystemExit(98)
    path = Path(sys.argv[sys.argv.index('--failure-log') + 1])
    kind = os.environ['MOCK_TRACE']
    if kind != 'missing':
        path.write_bytes(b'PRIVATE_TRACE' if kind == 'present' else b'x' * 2097153)
        path.chmod(0o600)
    raise SystemExit(int(os.environ['MOCK_STATUS']))
os.execv(sys.executable, [sys.executable, *sys.argv[1:]])
''')
        python.chmod(0o700)
        openssl = binaries / 'openssl'
        openssl.write_text('''#!/bin/sh
if [ "$1" = version ]; then echo 'OpenSSL mock'; exit 0; fi
if [ "$1" = x509 ]; then exit 0; fi
if [ "$2" = -help ]; then [ "$MOCK_CAPABILITY" = ok ]; exit $?; fi
if [ "$MOCK_CRYPTO" != ok ]; then echo 'PRIVATE_CRYPTO_ERROR' >&2; exit 1; fi
while [ "$1" != -out ]; do shift; done
printf CIPHERTEXT > "$2"
''')
        openssl.chmod(0o700)
        output = root / 'output'
        env = dict(os.environ, PATH=str(binaries) + ':/usr/bin:/bin', RUNNER_TEMP=str(root),
                   RUNNER_OS=runner, GITHUB_OUTPUT=str(output), MOCK_STATUS=str(status),
                   MOCK_TRACE=trace, MOCK_CRYPTO=crypto, MOCK_CAPABILITY=capability,
                   MARMISH_LEGACY_SKIP_NAMESPACE_TESTS='ambient-invalid')
        result = subprocess.run(['sh', '-eu'], input=self.capture_shell(), text=True, capture_output=True,
                                env=env, timeout=15)
        self.assertNotIn('PRIVATE_TRACE', result.stdout + result.stderr)
        self.assertNotIn('PRIVATE_CRYPTO_ERROR', result.stdout + result.stderr)
        self.assertEqual(list(root.glob('marmish-confidential.*/failure.log')), [])
        self.assertEqual(list(root.glob('marmish-confidential.*/openssl.stderr')), [])
        return result, output.read_text() if output.exists() else '', root

    def test_manual_source_guard_matrix_and_workflow_concurrency(self):
        header, jobs = self.text.split('jobs:\n', 1)
        self.assertIn('on:\n  workflow_dispatch:\n', header)
        self.assertNotIn('  push:', header)
        self.assertNotIn('  pull_request:', header)
        self.assertIn('concurrency:\n  group: legacy-private-trace\n  cancel-in-progress: false\n', header)
        self.assertNotIn('concurrency:', jobs)
        self.assertIn('permissions:\n  contents: read\n', header)
        self.assertIn("github.repository == 'Epiphytic/marmish' && github.ref == 'refs/heads/master' && github.event_name == 'workflow_dispatch'", jobs)
        self.assertIn('runner: [ubuntu-24.04, ubuntu-24.04-arm, macos-15]', jobs)
        self.assertIn('      fail-fast: false\n', jobs)
        self.assertIn('    timeout-minutes: 30\n', jobs)
        self.assertIn('SOURCE_COMMIT: ${{ inputs.source_commit }}', jobs)
        self.assertIn('[ "$SOURCE_COMMIT" = "$GITHUB_SHA" ]', jobs)
        self.assertIn('[ "$(git rev-parse HEAD)" = "$SOURCE_COMMIT" ]', jobs)
        self.assertIn('          persist-credentials: false', jobs)

    def test_same_acceptance_toolchain_and_platform_selection(self):
        shell = self.capture_shell()
        self.assertIn('toolchain: 1.97.1\n', self.text)
        self.assertIn("        if: runner.os == 'Linux'\n", self.text)
        self.assertIn('sudo apt-get install -y --no-install-recommends bubblewrap binutils', self.text)
        self.assertIn('python3 tools/marmish_managed_runtime_spike.py --report "$RUNNER_TEMP/managed-runtime-report.json" --failure-log "$scratch/failure.log"', shell)
        self.assertIn('export MARMISH_LEGACY_SKIP_NAMESPACE_TESTS=1', shell)
        self.assertIn('unset MARMISH_LEGACY_SKIP_NAMESPACE_TESTS', shell)
        self.assertIn('set +e\n', shell)
        self.assertIn('status=$?\n', shell)
        self.assertTrue(shell.endswith('exit "$status"\n'))
        self.assertNotIn('continue-on-error', self.text)

    def test_only_public_recipient_and_explicit_cipher_upload(self):
        certificate = self.repository / 'tests/fixtures/legacy-diagnostic-recipient.pem'
        self.assertTrue(certificate.read_text().startswith('-----BEGIN CERTIFICATE-----'))
        self.assertNotIn('PRIVATE KEY', certificate.read_text())
        self.assertIn(hashlib.sha256(certificate.read_bytes()).hexdigest(), self.capture_shell())
        self.assertIn(' -aes-256-cbc -outform DER ', self.capture_shell())
        uploads = [step for step in self.steps if 'uses: actions/upload-artifact@' in step]
        self.assertEqual(len(uploads), 2)
        self.assertIn('path: ${{ runner.temp }}/managed-runtime-report.json', uploads[0])
        self.assertIn('path: ${{ steps.capture.outputs.cipher_path }}', uploads[1])
        for step in uploads:
            self.assertIn('${{ matrix.runner }}', step)
            self.assertIn('retention-days: 1', step)
            for forbidden in ('failure.log', 'openssl.stderr', '.key', '**', 'secrets.'):
                self.assertNotIn(forbidden, step)

    def test_capture_shell_syntax(self):
        result = subprocess.run(['sh', '-n'], input=self.capture_shell(), text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_failed_suite_retains_status_and_encrypts_on_both_platforms(self):
        for runner in ('Linux', 'macOS'):
            with self.subTest(runner=runner):
                result, output, root = self.mocked_capture(status=7, runner=runner)
                self.assertEqual(result.returncode, 7, result.stderr)
                self.assertTrue(output.startswith('cipher_path='))
                cipher = Path(output.strip().split('=', 1)[1])
                self.assertEqual(cipher.read_bytes(), b'CIPHERTEXT')
                self.assertEqual(cipher.parent.parent, root)

    def test_success_without_trace_stays_success(self):
        result, output, _ = self.mocked_capture(status=0, trace='missing')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, '')

    def test_failure_without_trace_is_honest(self):
        result, output, _ = self.mocked_capture(status=7, trace='missing')
        self.assertEqual(result.returncode, 7)
        self.assertIn('confidential_trace: failure_without_trace', result.stderr)
        self.assertEqual(output, '')

    def test_encryption_and_capability_fail_closed_without_upload(self):
        for kwargs, expected in [({'crypto': 'fail'}, 73), ({'capability': 'fail'}, 72)]:
            with self.subTest(kwargs=kwargs):
                result, output, _ = self.mocked_capture(**kwargs)
                self.assertEqual(result.returncode, expected, result.stderr)
                self.assertEqual(output, '')

    def test_oversized_plaintext_refuses_before_encryption(self):
        result, output, root = self.mocked_capture(trace='oversized')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('confidential_trace: plaintext_invalid', result.stderr)
        self.assertEqual(output, '')
        self.assertEqual(list(root.glob('marmish-confidential.*/cipher.der')), [])
