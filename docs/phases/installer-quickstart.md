# Installer quickstart increment

Beads: `marmish-zwn`. This extends the reviewed distribution installer, retaining
its archive verification and platform gates. Existing phased implementation and
dual-publication authorization applies.

## Interface and sequence

Add optional `sh install.sh --quickstart`, compatible with `--prefix` and
`--version`, mutually exclusive with `--uninstall`. Plain installation remains
noninteractive and never accesses identities or keychains. Linux amd64/arm64 and
macOS arm64 (15+) share the same Python implementation; platform and runtime
limitations remain unchanged.

Quickstart opens a controlling terminal with
`os.open("/dev/tty", os.O_RDWR | os.O_NOCTTY)` before downloads or installation.
Require `os.tcgetpgrp(fd) == os.getpgrp()`; background invocation fails with an
actionable diagnostic before reading or writing the terminal. Missing terminal fails with an actionable diagnostic and no install
or identity mutation. The embedded Python uses stdin for source, so all prompts
and subprocess stdin explicitly use this terminal fd. Prompts use `os.write`;
read one byte at a time with `os.read`, capped at 4096 bytes, rejecting EOF, empty
input, whitespace/control characters and invalid UTF-8 after reading the complete
line through its newline (never reject a character midway through a normal line).
On abort, including overlong input, flush only pending terminal input with
`termios.tcflush(fd, termios.TCIFLUSH)` before returning control to the shell.
Successful prompts never flush, preserving valid child type-ahead. This avoids
executing rejected pasted text in the parent shell; bounded draining cannot
guarantee consuming an arbitrarily long remaining line. No buffered reader may
consume type-ahead intended for children. Child stdout/stderr inherit the
installer streams. Children use HOME as cwd and inherit the user environment. No bash-only shell syntax,
GNU-only terminal utility or shell evaluation is needed.

After the existing verified installation, prompt for server/client (reject other
values), explicit relay URL (reject empty input and require a `wss://` prefix before any
launcher invocation, preventing positional option injection), then invoke the installed
launcher by absolute path: `--doctor`, and `-i ~/.marmish/default --relay URL
--key-storage auto --init`. Initialization prints public account JSON for exchange.
Prompt for client public npub/hex on the server, or server public npub/hex on the
client. Reject empty/EOF input. Existing CLI validates relay/key syntax and
backend availability. No public relay default or private-key input is introduced.

Server starts `marmishd -i ID --relay=URL --allow=PEER --cwd HOME` in foreground.
Client starts `marmish -i ID --relay=URL -u PEER URL -- /usr/bin/printf
"%s\n" "Hello through Marmot"`. For the actual
short option use `-uPEER`, so leading dashes cannot become option syntax.
Peer syntax is ultimately checked by the frontend. Display the account JSON
shape and server-first ordering before prompting for a peer; display the server
command authority and invocation-only allowlist before its authorization prompt. Tell users to
start the server before submitting the client prompt and that authorized commands
run as the current Unix user. Use subprocess argv arrays, with terminal stdin;
failed doctor/init/command stops setup with a diagnostic. Children never set
`start_new_session` or `process_group`. Scope handlers for SIGINT, SIGHUP and
SIGTERM to quickstart (including install), converting the first signal into an
exception carrying its number and ignoring repeats during cleanup. A child wait
helper catches interruption, forwards that signal to the owned child if alive,
waits up to five seconds, then kills and reaps if needed. Prevent a spawn/handle-assignment signal race
by deferring signal exceptions during Popen assignment (record the first signal,
then forward/raise immediately once the handle is stored), without blocking
signals in the child. Other exceptional waits
also terminate/kill/reap the child. Restore prior handlers and close the terminal
on every path. Exit 128+signal for interruption, without an install-failure
diagnostic; child nonzero exits produce a concise failed-step diagnostic. Tests
send PTY Ctrl-C and installer-only SIGTERM during a long-running fake server
and prove neither leaves an orphan. Do not
rollback a successful installation or regenerate an identity after setup failure.

Native keychain auto selection, named-key defaults, profile reuse and locking
stay in the existing frontend. This creates a default profile by convention,
not a new implicit CLI identity. Separate profiles remain necessary for colocated
endpoints. No system packages, keychain service, shell profile or namespace policy
is changed. Print absolute launcher/PATH guidance; custom prefixes must work
without PATH discovery. Profile variables need not persist outside installer.

## Implementation and validation boundaries

Installer implementation and README owned by one agent; integration owner adds
focused tests and spec/build-plan updates. Existing installer tests must pass.
Add offline quickstart tests for both roles, absolute custom-prefix execution,
special characters in paths/argv, empty/EOF/invalid role, child failures and
uninstall conflict. Exercise the actual shell wrapper through a controlling PTY
with fake installed commands and deterministic offline release fixtures; prove
terminal prompts survive embedded stdin, and no-key/no-terminal failures stop.
Check shared Linux and Darwin branch selection using existing target fixtures;
if native macOS is unavailable report that explicitly, rather than calling mocks
native acceptance. No real keys or infrastructure needed for this orchestration
change. Native-platform runtime limits from the existing release remain visible.

README becomes a short two-command download/run example, using the required Iris
Radicle URL. Execute only after successful curl, from a file rather than a pipe;
no setup logic is repeated in README. Mention the local filename, prerequisites,
identity exchange and ordinary install-only mode. Update agent spec to distinguish
plain install from opt-in identity-changing quickstart.

Obtain independent Claude design approval before implementation and auto-mode
code review with local tests after integration. Record sanitized reviews and
validation, resolve blocking findings, then commit and push to GitHub and Radicle
and independently verify remote refs and public Iris script bytes.

## Concrete offline seam and native checks

Refactor the current download/install/summary body into
`install_release(options, prefix, launcher_dir)`. `main()` parses and validates
options/version, obtains the terminal for quickstart, calls install_release,
then calls `quickstart(launcher_dir, terminal_fd)` after download temporaries and
installation locks are released. Plain install only calls install_release.
No production network origin, fixture or environment override is added.

The wrapper PTY test writes a disposable copy of the exact shell wrapper,
inserting a test-only replacement for install_release immediately before its
`if __name__` entry point. The replacement installs deterministic owned fake
launchers into a temporary prefix and never fetches anything. This tests the
actual embedded-stdin shell entry point, main, terminal and subprocess lifecycle;
the existing archive suite separately tests real offline installation and trust
checks. Use Python pty.fork (no script(1)) with bounded polling and owned cleanup.
Test parent stdin EOF, tty EOF, absent controlling terminal, signals, and fake
command failures. Never modify the tracked installer to inject fixtures.

A focused GitHub Actions matrix runs installer and quickstart offline suites on
ubuntu-24.04, ubuntu-24.04-arm and macos-15, using the existing pinned checkout and
Python bootstrap. This tests actual native terminal behavior without real keys
or claiming native MDK confinement acceptance. macOS may display native Keychain
permission prompts during real setup; quickstart never suppresses them.

An existing profile's --init must reuse and reprint its account. The frontend
retains responsibility for private profile directories; quickstart does not
create them or remove state after failure. Validate --version before terminal
acquisition/download and reject quickstart+uninstall as argparse exit 2. Absolute
launchers are revalidated against installer ownership metadata before setup;
this is a same-user integrity check, not protection from a hostile same-user
process. Reruns use the unchanged existing CLI reuse contract.

Rejected-paste PTY coverage sends a control character and a shell-looking suffix
and verifies all of that line is consumed before abort; on overlong rejection,
pending input is flushed. No input is ever evaluated as shell code.
