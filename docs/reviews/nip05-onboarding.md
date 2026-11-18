# Claude Code review of native NIP-05 onboarding

## Design

Claude Code Sonnet 5.5, session `ccba6cc6-b3d8-4afa-9e57-ac77cea221b3`,
reviewed the [phase design](../phases/nip05-onboarding.md). The first round
requested a disposable pre-consent signer, a precise Arti setup and cleanup
path, explicit dependencies and fixture scope, and a publication sequence
that verifies the directory points to the local key. The revised design
addressed those findings and received approval before implementation.

## Integrated source

Claude Code Sonnet 5.5, session `ed4e00be-2d67-4c2e-b642-cfd179a3f31c`,
reviewed the staged native source, dependencies, tests, documentation and
self-hosted workflow in auto mode. It approved the reviewed increment with
no blocking findings. It ran 17 passing targeted cases for directory parsing,
metadata verification and real relay I/O, CLI parser invariants, setup
confirmation and config commit, and the loopback TLS fixture. The metadata
latency benchmark remained opt-in in that review.

The reviewer identified a late `--update` refusal on existing profiles.
The check now runs during the local validation before a directory query.
The README also explains the typed confirmation with a controlling terminal.
Local benchmark results and environment limits are recorded in the
[validation report](../validation/nip05-onboarding.md).

Claude Code re-reviewed the follow-up in the same Sonnet 5.5 session in auto
mode and approved it. The reviewer ran 17 targeted tests, including the
real sign-and-publish fixture, config commit, both relay fixtures and the
loopback TLS fixture. Format and Clippy checks passed. The review covered
the test-only relay, directory and terminal seams and their separation
from production paths.

The same Claude session approved the test-only follow-up in auto mode after
an Apple libc pointer correction. The reviewer ran the controlling-terminal
fixture, the isolated publication fixture, the full 22-test profile filter,
the core filters and the native formatting check on Linux. macOS runtime
coverage remains pending.
