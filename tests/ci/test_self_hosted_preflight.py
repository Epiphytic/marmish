"""Offline trust-policy mutations and real shell-probe refusal checks.

Run with: python3 -m unittest discover -s tests/ci -v
No network, Docker daemon, hosted runner or third-party Python dependency is used.
"""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / '.github/workflows/self-hosted-preflight.yml'
PROBE = ROOT / 'scripts/self-hosted-preflight.sh'
CHECKOUT = 'actions/checkout@11d5960a326750d5838078e36cf38b85af677262'
LABELS = {
    ('self-hosted', 'Linux', 'X64', 'bel-ci'),
    ('self-hosted', 'Linux', 'ARM64', 'laptop-ci'),
    ('self-hosted', 'macOS', 'ARM64', 'laptop-ci'),
}


def validate_policy(text):
    """Check the actual dispatch surface before a trusted pool may execute it."""
    def require(condition, reason):
        if not condition:
            raise ValueError(reason)

    require(re.search(r'^on:\n  workflow_dispatch:\s*\n(?=\S)', text, re.M), 'manual trigger')
    require(not re.search(r'^\s+(?:push|pull_request|pull_request_target|schedule|workflow_run):', text, re.M), 'automatic trigger')
    require("if: github.repository == 'Epiphytic/marmish' && github.ref == 'refs/heads/master' && github.event_name == 'workflow_dispatch'" in text, 'trusted dispatch')
    permissions = re.findall(r'^permissions:\n((?:  [^\n]+\n)+)', text, re.M)
    require(permissions == ['  contents: read\n'], 'readonly permissions')
    require(not re.search(r'^ +permissions:', text, re.M), 'job permissions override')
    jobs = text.split('\njobs:\n', 1)
    require(len(jobs) == 2 and re.findall(r'^  ([A-Za-z0-9_-]+):$', jobs[1], re.M) == ['inventory'], 'single inventory job')
    require(len(re.findall(r'^    runs-on:', text, re.M)) == 1, 'single runner selection')
    require('concurrency:\n  group: marmish-self-hosted-preflight\n  cancel-in-progress: false\n' in text, 'dedicated concurrency')
    require(re.search(r'^      max-parallel: 1$', text, re.M), 'serial admission')
    require(re.search(r'^      fail-fast: false$', text, re.M), 'complete inventory')
    labels = re.findall(r'^\s+labels: \[([^\]]+)\]$', text, re.M)
    require(len(labels) == 3 and {tuple(x.strip() for x in label.split(',')) for label in labels} == LABELS, 'native labels')
    require('    runs-on:\n      group: epiphytic-ci\n      labels: ${{ matrix.labels }}\n' in text, 'runner group')
    require(re.search(r'^    timeout-minutes: 10$', text, re.M), 'bounded job')
    actions = re.findall(r'^\s+- uses: (\S+)$', text, re.M)
    require(actions == [CHECKOUT], 'pinned checkout only')
    require('          persist-credentials: false\n' in text, 'checkout credentials')
    require('PREFLIGHT_RUNNER_ENVIRONMENT: ${{ runner.environment }}' in text, 'runner marker')
    require('PREFLIGHT_PLATFORM: ${{ matrix.platform }}' in text, 'platform binding')
    require('run: sh scripts/self-hosted-preflight.sh probe' in text, 'real probe')
    require(re.search(r'if: always\(\)\n(?:[^\n]*\n){0,4}[^\n]*run: sh scripts/self-hosted-preflight.sh cleanup', text), 'always cleanup')
    for forbidden in ('secrets.', 'sudo ', 'continue-on-error', 'ubuntu-24.04', 'macos-15', 'github.token', 'GITHUB_TOKEN'):
        require(forbidden not in text, 'forbidden capability')


class WorkflowPolicyTests(unittest.TestCase):
    def test_checked_in_workflow_obeys_trust_policy(self):
        validate_policy(WORKFLOW.read_text())

    def test_mutations_cannot_relax_trusted_pool_contract(self):
        original = WORKFLOW.read_text()
        mutations = [
            ('  workflow_dispatch:', '  push:'),
            ("github.repository == 'Epiphytic/marmish'", 'true'),
            ("github.ref == 'refs/heads/master'", 'true'),
            ("github.event_name == 'workflow_dispatch'", 'true'),
            ('contents: read', 'contents: write'),
            ('max-parallel: 1', 'max-parallel: 3'),
            ('fail-fast: false', 'fail-fast: true'),
            ('group: marmish-self-hosted-preflight', 'group: marmish-native-validation'),
            ('cancel-in-progress: false', 'cancel-in-progress: true'),
            ('group: epiphytic-ci', 'group: other'),
            ('self-hosted, Linux, X64, bel-ci', 'ubuntu-24.04'),
            ('self-hosted, Linux, ARM64, laptop-ci', 'self-hosted, Linux, X64, laptop-ci'),
            ('self-hosted, macOS, ARM64, laptop-ci', 'self-hosted, macOS, X64, laptop-ci'),
            (CHECKOUT, 'actions/checkout@main'),
            ('persist-credentials: false', 'persist-credentials: true'),
            ('${{ runner.environment }}', 'self-hosted'),
            ('${{ matrix.platform }}', 'linux-amd64'),
            ('if: always()', 'if: success()'),
            ('timeout-minutes: 10', 'timeout-minutes: 60'),
            ('run: sh scripts/self-hosted-preflight.sh probe', 'run: true'),
        ]
        for before, after in mutations:
            with self.subTest(contract=before):
                self.assertIn(before, original, 'mutation must affect checked-in workflow')
                with self.assertRaises(ValueError):
                    validate_policy(original.replace(before, after, 1))
        for extra in ('  pull_request:\n', '  schedule:\n'):
            with self.subTest(trigger=extra.strip()), self.assertRaises(ValueError):
                validate_policy(original.replace('  workflow_dispatch:\n', '  workflow_dispatch:\n' + extra, 1))
        for runner in ('ubuntu-latest', 'macos-14'):
            with self.subTest(extra_job=runner), self.assertRaises(ValueError):
                validate_policy(original + '\n  other:\n    runs-on: ' + runner + '\n    steps:\n      - run: true\n')


class ProbeTests(unittest.TestCase):
    def run_probe(self, *, system='Linux', architecture='x86_64', uid='1001', marker='self-hosted', identity='owned-test-execution', hostile_versions=False):
        with tempfile.TemporaryDirectory() as temporary:
            commands = Path(temporary)
            for name, body in {
                'uname': 'case "$1" in -s) printf "%s\\n" "$FAKE_SYSTEM";; -m) printf "%s\\n" "$FAKE_ARCH";; *) exit 1;; esac',
                'id': 'printf "%s\\n" "$FAKE_UID"',
            }.items():
                executable = commands / name
                executable.write_text('#!/bin/sh\n' + body + '\n')
                executable.chmod(0o700)
            if hostile_versions:
                for name in ('python3', 'node', 'git', 'curl', 'cc', 'make', 'perl', 'tar', 'gzip', 'unzip', 'getconf'):
                    executable = commands / name
                    executable.write_text('#!/bin/sh\nprintf "%s\\n" "private-user /private/home/secret 24.4.1 credentials=do-not-log"\n')
                    executable.chmod(0o700)
                # Prevent any registry call or owned-container mutation.
                docker = commands / 'docker'
                docker.write_text('#!/bin/sh\nexit 1\n')
                docker.chmod(0o700)
            env = {
                'PATH': str(commands) + ':/usr/bin:/bin',
                'PREFLIGHT_PLATFORM': 'linux-amd64',
                'PREFLIGHT_RUNNER_ENVIRONMENT': marker,
                'EPIPHYTIC_EXECUTION_ID': identity,
                'GITHUB_RUN_ID': '123456', 'GITHUB_RUN_ATTEMPT': '1',
                'FAKE_SYSTEM': system, 'FAKE_ARCH': architecture, 'FAKE_UID': uid,
            }
            return subprocess.run(['/bin/sh', str(PROBE), 'probe'], env=env, capture_output=True, text=True, timeout=10)

    def test_shell_syntax(self):
        result = subprocess.run(['/bin/sh', '-n', str(PROBE)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_wrong_architecture_os_root_and_identity_fail_closed(self):
        for changed in ({'architecture': 'aarch64'}, {'system': 'Darwin'}, {'uid': '0'},
                        {'marker': 'github-hosted'}, {'marker': ''}, {'identity': ''}):
            with self.subTest(changed=changed):
                result = self.run_probe(**changed)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('owned-test-execution', result.stdout + result.stderr)
                self.assertNotIn('/home/', result.stdout + result.stderr)

    def test_tool_versions_never_echo_arbitrary_host_details(self):
        result = self.run_probe(hostile_versions=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for secret in ('private-user', '/private/', 'credentials', 'do-not-log', 'owned-test-execution'):
            self.assertNotIn(secret, result.stdout + result.stderr)
        self.assertTrue(result.stdout.strip(), 'probe must produce capability evidence')
        for line in result.stdout.splitlines():
            self.assertRegex(line, r'^[a-zA-Z0-9_.-]+=[a-zA-Z0-9_.:-]+$')


class ContainerOwnershipTests(unittest.TestCase):
    def exercise(self, mode, state, ready='', docker_present=True):
        """Run the real shell with an offline Docker state machine, never Docker."""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            fake_commands = {
                'uname': 'case "$1" in -s) echo Linux;; -m) echo x86_64;; *) exit 1;; esac',
                'id': 'echo 1001',
                'docker': '''printf '%s\\n' "$*" >> "$DOCKER_LOG"
case "$1" in
    --version) echo 'Docker version 28.0.0';;
    info) echo '["name=rootless"]';;
    container)
        case "$DOCKER_STATE" in
            occupied) echo 'marmish-preflight-123456-1-linux-amd64';;
            unavailable) exit 1;;
            absent) :;;
            *) exit 97;;
        esac;;
    rm) :;;
    pull) exit 98;;
    *) exit 99;;
esac''',
            }
            for name, body in fake_commands.items():
                if name == 'docker' and not docker_present:
                    continue
                executable = root / name
                executable.write_text('#!/bin/sh\n' + body + '\n')
                executable.chmod(0o700)
            output, log = root / 'outputs', root / 'docker-calls'
            env = {
                'PATH': str(root) + (':/usr/bin:/bin' if docker_present else ''),
                'PREFLIGHT_PLATFORM': 'linux-amd64',
                'PREFLIGHT_RUNNER_ENVIRONMENT': 'self-hosted',
                'EPIPHYTIC_EXECUTION_ID': 'owned-test-execution',
                'GITHUB_RUN_ID': '123456', 'GITHUB_RUN_ATTEMPT': '1',
                'GITHUB_OUTPUT': str(output),
                'PREFLIGHT_DOCKER_READY': ready,
                'DOCKER_STATE': state, 'DOCKER_LOG': str(log),
            }
            result = subprocess.run(['/bin/sh', str(PROBE), mode], env=env,
                                    capture_output=True, text=True, timeout=10)
            return (result, output.read_text() if output.exists() else '',
                    log.read_text().splitlines() if log.exists() else [])

    def test_preexisting_name_is_not_claimed_or_removed(self):
        result, output, calls = self.exercise('probe', 'occupied')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('preflight_error=container_name_occupied', result.stderr)
        self.assertEqual(output, '')
        self.assertTrue(any(call.startswith('container ls ') for call in calls))
        self.assertFalse(any(call.startswith(('rm ', 'pull ', 'run ')) for call in calls))
        cleanup, _, cleanup_calls = self.exercise('cleanup', 'occupied', ready='')
        self.assertEqual(cleanup.returncode, 0, cleanup.stderr)
        self.assertIn('container_cleanup=not_attempted', cleanup.stdout)
        self.assertEqual(cleanup_calls, [], 'foreign container must not be inspected or removed')

    def test_claim_is_recorded_before_pull_failure(self):
        result, output, calls = self.exercise('probe', 'absent')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(output, 'docker_ready=true\n')
        self.assertIn('container_smoke=missing_image', result.stdout)
        self.assertTrue(any(call.startswith('pull ubuntu@sha256:') for call in calls))
        self.assertFalse(any(call.startswith('run ') for call in calls))

    def test_attempted_cleanup_proves_absence_and_rejects_uncertainty(self):
        for state, success in (('unavailable', False), ('occupied', False), ('absent', True)):
            with self.subTest(state=state):
                result, output, calls = self.exercise('cleanup', state, ready='true')
                self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)
                self.assertEqual(output, '')
                self.assertEqual(calls, [
                    'rm -f marmish-preflight-123456-1-linux-amd64',
                    'container ls -a --filter name=^/marmish-preflight-123456-1-linux-amd64$ --format {{.Names}}',
                ])
                if success:
                    self.assertEqual(result.stdout, 'container_cleanup=absent\n')
                else:
                    self.assertIn('preflight_error=cleanup_uncertain', result.stderr)

    def test_unattempted_cleanup_does_not_require_docker(self):
        result, output, calls = self.exercise('cleanup', 'unavailable', ready='', docker_present=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, 'container_cleanup=not_attempted\n')
        self.assertEqual(output, '')
        self.assertEqual(calls, [], 'unattempted cleanup must work without an engine')


if __name__ == '__main__':
    unittest.main()
