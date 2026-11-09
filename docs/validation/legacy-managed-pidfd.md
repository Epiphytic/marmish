# Managed Python pidfd validation

Beads `marmish-d93.1`; Linux x86-64, managed CPython 3.13.13. These are focused
legacy reference and installed-test-harness observations, not native release
qualification or proof of the historical hosted failure's cause.

The original local two-test run raised two `AttributeError` errors because this
interpreter lacks `os.pidfd_open`. The correction preserves identity checks and
descriptor-only signalling, using libc only when the corresponding Python API
is absent. There is no numeric-PID fallback or retry after an OS error.

| Selection | Executed | Passed | Errors / failures / skips |
|---|---:|---:|---|
| `LinuxPidfdTests` + `PidfdSignalTests` | 21 | 21 | 0 / 0 / 0 |
| `LinuxPidfdIntegrationTests.test_libc_own_process_descriptor_is_noninheritable` | 1 | 1 | 0 / 0 / 0 |
| Original two setsid cleanup/refusal tests | 2 | 2 | 0 / 0 / 0 |

The real libc test forced both Python APIs absent, opened only its own process,
verified noninheritability, checked signal zero and closed the descriptor. The
setsid tests retained their original identity, cross-session refusal and
descriptor-readability assertions. Claude executed the latter two selections
once during completed-code review after inspecting the first result.

All three outer controllers exited 87: their global process observations on a
shared host were incomplete. Each recorded zero tracked survivors and no child
invariant failure, but that is not proof of global cleanup. Private state was
retained; no unattributed process was signalled or deleted. Full modules and
hosted CI remain untested here.

The reviewed four-source-file patch SHA256 is
`40e612f02cbeb458a3a0e38fea4f984bf6f4e078552abc2a037e6903019005a8`,
relative to `d23fce66a0b1a70bb3233da158d6ca7be6ae796a`.
Retained child-result SHA256 values:

| Selection | SHA256 |
|---|---|
| Mocked units | `421394449598cec4c6be1839559e9afd10499c0d9f62326f10bcefc161489acb` |
| Own process | `ca3be2b55a2b82eb9a59eda1c8942d7e29f53d451483c8517b0b9776c1d58750` |
| Setsid | `0d7c54f50ee4e4f0ac4fcad4aa11f799ab462f9646b2b0ef8c2a909ae85322b3` |
