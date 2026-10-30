"""Dependency-free contracts for the reviewed self-hosted workflow blocks."""
from pathlib import Path
import re
import subprocess
import unittest


SOURCE = (Path(__file__).resolve().parent.parent /
          '.github/workflows/native-validation.yml').read_text()
PUSH_GUARD = ("github.repository == 'Epiphytic/marmish' && "
              "github.ref == 'refs/heads/master' && github.event_name == 'push'")
MANAGED_GUARD = ("github.repository == 'Epiphytic/marmish' && "
                 "github.ref == 'refs/heads/master' && "
                 "(github.event_name == 'push' || github.event_name == 'workflow_dispatch')")


def job_block(name):
    """Select a known top-level job, without interpreting YAML."""
    remainder = SOURCE.split('  ' + name + ':\n', 1)[1]
    return re.split(r'^  [a-z-]+:\n', remainder, maxsplit=1, flags=re.MULTILINE)[0]


def step_blocks(job):
    return re.split(r'^      - ', job, flags=re.MULTILINE)[1:]


class ShellContracts(unittest.TestCase):
    def test_run_blocks_have_valid_shell_syntax(self):
        blocks = re.findall(r'^        run: \|\n((?:          .*\n|\n)+)', SOURCE, re.MULTILINE)
        self.assertTrue(blocks)
        for block in blocks:
            script = re.sub(r'\$\{\{[^}]+\}\}', 'fixture', block)
            with self.subTest(script=script.splitlines()[0]):
                result = subprocess.run(['sh', '-n'], input=script, text=True,
                                        capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)


class ManagedWorkflowContracts(unittest.TestCase):
    def setUp(self):
        self.proof = job_block('managed-proof')
        self.steps = step_blocks(self.proof)

    def test_admission_and_independent_scheduling(self):
        self.assertIn('  workflow_dispatch:\npermissions:\n  contents: read\n', SOURCE)
        self.assertIn('    branches: [master]\n', SOURCE)
        self.assertNotIn('pull_request', SOURCE)
        self.assertTrue(self.proof.startswith('    if: ' + MANAGED_GUARD + '\n'))
        for name in ('native', 'live'):
            self.assertTrue(job_block(name).startswith('    if: ' + PUSH_GUARD + '\n'))
        self.assertNotRegex(self.proof, re.compile(r'^    (?:needs|permissions):', re.MULTILINE))

    def test_exact_runner_selection(self):
        configuration = self.proof.split('    strategy:\n', 1)[1].split('    steps:\n', 1)[0]
        self.assertEqual(configuration, '''      fail-fast: false
      max-parallel: 1
      matrix:
        include:
          - platform: linux-x64
            labels: [self-hosted, Linux, X64, bel-ci]
          - platform: linux-arm64
            labels: [self-hosted, Linux, ARM64, laptop-ci]
          - platform: macos-arm64
            labels: [self-hosted, macOS, ARM64, laptop-ci]
    runs-on:
      group: epiphytic-ci
      labels: ${{ matrix.labels }}
''')

    def test_every_step_has_reviewed_budget_and_pins(self):
        self.assertIn('    timeout-minutes: 60\n', self.proof)
        budgets = []
        for step in self.steps:
            values = re.findall(r'^        timeout-minutes: ([0-9]+)$', step, re.MULTILINE)
            self.assertEqual(len(values), 1)
            budgets.append(int(values[0]))
        self.assertEqual(budgets, [2, 5, 6, 1, 3, 10, 25, 2, 3])
        self.assertLess(sum(budgets), 60)
        self.assertIn('uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262\n', self.steps[0])
        self.assertIn('          persist-credentials: false\n', self.steps[0])
        native = step_blocks(job_block('native'))
        # Only the new step deadline differs from the existing reviewed cache/bootstrap.
        for index in (4, 5):
            selected = self.steps[index]
            original = next(step for step in native if step.splitlines()[0] == selected.splitlines()[0])
            self.assertEqual(re.sub(r'^        timeout-minutes: [0-9]+\n', '', selected,
                                    flags=re.MULTILINE), original)

    def test_harness_interfaces_and_authenticated_helper(self):
        self.assertIn('        run: sh tools/bootstrap_ci_python.sh\n', self.steps[1])
        for index in (2, 3):
            self.assertIn("        if: runner.os == 'Linux'\n", self.steps[index])
        self.assertIn('        run: python3 tools/marmish_ci_bwrap.py bootstrap --state "$RUNNER_TEMP/managed-bwrap-state"\n', self.steps[2])
        self.assertIn('        run: python3 tools/marmish_ci_bwrap.py probe\n', self.steps[3])
        command = self.steps[6]
        self.assertIn('        id: proof\n', command)
        for fragment in ('umask 077', 'mkdir "$RUNNER_TEMP/managed-proof"',
                         'if [ "$RUNNER_OS" = Linux ]; then\n            set -- --require-bwrap-helper',
                         'else\n            set --\n          fi', '--total-timeout 1320',
                         "--ci-context '${{ github.sha }}:${{ github.run_id }}:${{ github.run_attempt }}:${{ matrix.platform }}'",
                         '--report "$RUNNER_TEMP/managed-proof/managed-runtime-report.json"'):
            self.assertIn(fragment, command)
        self.assertNotIn('continue-on-error', self.proof)
        self.assertNotIn('--failure-log', self.proof)
        self.assertNotRegex(self.proof, r'\bsudo\b|\bapt-get\b|sysctl\s+-w')

    def test_report_only_upload_and_conservative_cleanup(self):
        upload, cleanup = self.steps[-2:]
        for fragment in ('        if: always()\n',
                         'uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02\n',
                         '          path: ${{ runner.temp }}/managed-proof/managed-runtime-report.json\n',
                         '          name: managed-selfhosted-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.platform }}\n',
                         '          if-no-files-found: warn\n'):
            self.assertIn(fragment, upload)
        self.assertIn("        if: always() && runner.os == 'Linux'\n", cleanup)
        native_cleanup = step_blocks(job_block('native'))[-1].split('        run: |\n', 1)[1]
        self.assertEqual(cleanup.split('        run: |\n', 1)[1],
                         native_cleanup.replace('steps.offline.outcome', 'steps.proof.outcome')
                         .replace('bwrap-state', 'managed-bwrap-state'))


if __name__ == '__main__':
    unittest.main()
