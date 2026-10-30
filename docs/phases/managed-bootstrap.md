# Increment 1: managed bootstrap and application setup

Status: Claude design approval conditional on phase0 native gates; not implemented. Depends on approved
[increment 0](managed-runtime-spike.md), its native packaging evidence and Claude
review of this design. The user authorized uv implementation and installer CI;
no further permission request is needed for scoped work after design approval.
Phase 0 design is approved; its pins/interfaces remain candidates until native
evidence passes. Do not claim its pending platform results as completed. Beads owns execution tracking and review dispositions.

## Deliverable and boundary

Ship `scripts/install.sh` without a system-Python requirement: a small POSIX shell
bootstrap obtains pinned uv and CPython, then runs the existing Python installer
under that private interpreter. The application also remains Python. Keep the
strict archive verifier and immutable native distribution; do not introduce
wheels, package indexes, uvx, source builds or a second dependency resolver.
Services and installed CLI launchers use absolute retained interpreter paths;
none runs uv or downloads at startup. Plain installation never touches keys.

Add app-owned `marmish --setup` for the existing foreground client/server
quickstart behavior, reusing its terminal handling, diagnostics, child lifecycle,
explicit relay, default identity and `--key-storage auto` semantics. This phase
does not implement unified ownership, Both mode, persistent roles or services.
The requested c/s/b default-Both experience depends on later reviewed identity
owner/role phases; presenting Both as working before then is forbidden.
A small c/s selection improves the existing role prompt without implying Both.

Actual contracts inspected: installer `install_archive`, `install_locked`,
`publish_installation`, `read_installation`, `quickstart` and `run_options`;
`marmish_release.launcher`; `marmish_installed.validate_record`; existing install,
quickstart and archive-acceptance tests; `installer-validation.yml` native matrix.
Existing archive launchers execute `python3`, and their bytes are release-hashed.
Rewriting them in place would invalidate archive provenance and idempotence.

## Source and runtime trust

Carry increment 0's reviewed per-target uv/PBS URLs, sizes, exact versions,
SHA256s and source revision as literals in `scripts/install.sh`; a test requires
exact equality with the phase-0 fixture. A reviewed change updates
pins; no ambient UV/PIP/PYTHON configuration, mutable latest interpreter choice
or executable upstream installer script is used. The initial script remains the
HTTPS project-source trust anchor; document commit-pinned reproducibility and
mutable-head trust honestly. Current Marmish release discovery may retain its
validated experimental-release selection; recorded archive digest and source
commit bind the selected application, distinct from pinned interpreter bytes.
No claim that same-publisher checksums independently authenticate a publisher.

Before Python exists, the shell refuses root, parses a finite exact option set,
selects target and prepares only scratch-hosted uv/CPython. It never computes a
default prefix, touches an installation prefix or creates a retained slot.
Run the Python installer under that verified scratch interpreter, including for
uninstall; discard scratch after completion. Detailed path/ownership validation
and prefix flock happen in Python before final-path runtime installation.
No eval, shell-interpreted user values or fetched executable installer script.

Both parsers use one acceptance table; set Python `allow_abbrev=False` and
explicitly document removal of the old argparse abbreviation behavior:

| Input | Shell and Python behavior |
| --- | --- |
| `-h`, `--help` alone | Identical static help, exit 0, no downloads |
| `--debug`, `--quickstart`, `--uninstall` | Exact flags; quickstart/uninstall mutually exclusive |
| `--version V`, `--version=V` | Require exact experimental-version syntax and nonempty value |
| `--prefix P`, `--prefix=P` | Require nonempty absolute canonical spelling; Python validates filesystem |
| Uninstall plus version | Reject before network |
| Unknown/abbreviated option, positional arg, missing value | Reject before network |
| Help with other args, repeated scalar/boolean flags | Reject consistently before network |

Use canonical platform temporary roots, fresh mode0700 scratch and fixed system
tools. Manual HTTPS redirects have a five-hop bound, host/scheme validation at
every hop and no curl `-L`; impose time, byte and output bounds as in phase 0.
Linux uses `sha256sum`, macOS `shasum -a 256`. Read-only CPython bytes are rehashed
immediately before uv reads local one-entry metadata with a mandatory valid hash.
Run uv with `--no-config --offline --no-cache`, owned scratch HOME/XDG/TMPDIR,
explicit install directory, no system discovery and `UV_PYTHON_CACHE_DIR` unset.
Only uv gets scratch HOME/XDG/TMPDIR. The Python installer runs with real HOME
and filtered real-user environment so `Path.home()` selects existing defaults;
application/setup inherits real HOME and required desktop/keychain session
addresses. Filter loader/Python injection throughout; no user configuration may
influence uv. Never direct installation or identity initialization into scratch.

Keep the embedded marked Python installer initially: shell writes the exact
marked region to its owned scratch file and invokes it with managed Python
`-I -B`; assert marker uniqueness and payload equality in tests. No fetched
secondary installer or unsafe heredoc interpolation. `tools` extraction tests
must accommodate the new shell stub while continuing to test the same body.
Keep `inflate`, `extract_ustar`, `validate_manifest` and begin/end markers stable
for the phase-0 harness; new runtime logic wraps rather than replaces them.

## Layout, publication and recovery

Use existing defaults: prefix `~/.local/share/marmish`, launchers `~/.local/bin`;
`--prefix ABS` keeps launchers in that prefix's `bin`. Preserve root refusal,
canonical/ownership checks, unrelated-file refusal and current profile locations.

- `versions/marmish-V-T/`: exact verified release tree, unchanged `release.json`
  and every payload hash, including its original `bin` launchers.
- `runtimes/rt1-T-H/`: final-path managed base interpreter and minor links, where
  H is SHA256 of canonical JSON `[receipt-format,target,CPython-version,
  PBS-build,PBS-archive-sha256,uv-archive-sha256]`. Use the complete digest;
  reuse under the prefix flock only after complete validation. The mode0600
  receipt records those identities, distribution/executable relative paths and
  PBS distribution tree entries/modes/digests/symlinks. Hash only that PBS tree,
  not uv's adjacent `.lock`, `.cache` or minor-link entries; validate those links
  separately for containment. No venv or moving/renaming a completed slot.
  Preserve application/native licenses and collect uv/PBS licensing provenance.
- `.marmish-install.json` format 2: version, target, launcher_dir, runtime slot,
  interpreter relative path, runtime-receipt digest and launcher format. Validate
  exact fields/types/bounds, containment and ownership; no arbitrary executables.
  `current` continues to select the immutable release tree.

Format-2 installer-owned outer launchers invoke the retained absolute Python
with `-I -B -c` and the immutable archive's tools directory as an argv value,
then `guarded_main(client_main/server_main)`; preserve argument boundaries.
The embedded installer cannot import `marmish_release` (not shipped in archives).
Keep its generator self-contained and parity-tested against the extended
`marmish_release.launcher` template, preserving legacy generation. Every string
is shell-quoted; no user value is interpolated into Python source. Installed
layout resolution still finds native companions beside the verified tools tree.

Retain format-1 marker/launcher recognition exactly for existing installations.
Upgrade validates legacy ownership before replacing outer launchers; old archive
versions, profiles, Keychain items and databases remain untouched. Format-2
ownership checks require exact generated launcher bytes and verify runtime
receipt/tree/link constraints before reusing an existing runtime; uv's mere
"already installed" result never authorizes reuse. No silent downgrade to PATH
Python if a retained runtime is missing, modified or incompatible.

The scratch-interpreter Python body performs root refusal, `safe_path`,
`owned_directory(private)` and prefix flock first. Under that same lock it
recovers any pending publication, validates/reuses a complete deterministic slot
or exclusively creates the final-path slot and calls pinned uv with private uv
environment. Write/fsync its receipt only after full verification. A receipt-less
slot found under this lock cannot have a live conforming installer; remove only
that validated owned incomplete slot, never reclaim by elapsed time. A complete
but invalid slot fails closed rather than replacing an active runtime. Uninstall
uses scratch Python and never creates a retained runtime in the target prefix.
Keep the lock through archive validation and publication; no separate competing
final-slot creation protocol. Scratch-hosted concurrent downloads are harmless.

Keep caught-error rollback of old marker, selector and exact launcher bytes.
Add a bounded owner-only transaction record with old/new validated publication
identities `(format,version,target,slot,receipt-digest)` and launcher-dir binding;
regenerate exact marker/selector/launcher bytes deterministically. Write it before
replacing publication files; fsync record/files/parent directories. Startup under the prefix lock resolves a
pending transaction by restoring the complete validated old publication (or
removing only this transaction's fresh launchers for a first install), then
retries. Remove transaction only after publication and directory fsync. Reject
inconsistent/unowned recovery material rather than guessing. No profile cleanup.
This extends recovery without claiming atomic multi-file replacement.

`--uninstall` removes only owned public launchers/selector/marker, retaining
runtime/version trees and all user state as today. The shell entrypoint uses
the same pinned bootstrap path before calling the verifier/uninstaller; it does
not execute a discovered existing interpreter before verification merely to
parse its JSON receipt. This may need downloads even for uninstall: disclose it
and fail without modifying the installation when bootstrap bytes are unavailable
offline. Offline application startup remains mandatory; offline shell uninstall
is deferred. No runtime garbage collection or retained-version deletion.

## Managed interpreter inside confinement

New archive code must remove Linux's hidden system-Python dependency, not just
its launcher lookup: `_doctor` executes `/usr/bin/python3`, and the Linux broker
resolver, unshare outer process and bwrap inner process hardcode it. Update these
paths to the installed runtime contract: `realpath(sys.executable)` inside a
validated slot/PBS distribution with matching architecture, never an unvalidated
environment variable or whichever interpreter PATH supplies. The app discovers
the receipt through this fixed slot ancestry, not mutable `current`/installation
marker; no app-version cryptographic binding is claimed. Receipt/marker/runtime
are owner-writable and provide tamper detection, not protection against the same
Unix user. New archives advertise `MANAGED_HELPER_RUNTIME_API = 1` independently
of `SETUP_API_VERSION`. If a new archive runs through its old `bin` launcher
under non-managed Python, doctor/transport must refuse rather than fall back.
Source checkouts have an explicit separately labeled development path only.

At install/reuse, fully rehash the PBS distribution; bound receipt to 4MiB,
10,000 entries and 1GiB runtime tree (refine downward from phase-0 evidence).
At every transport start validate owned receipt/path/mode/architecture/containment
and hash an explicit reviewed startup closure: Python, libpython, required
extensions and startup-critical stdlib files, bounded at 128 files/64MiB total.
Freeze the exact per-target closure from dependency/import audit; measure startup
cost in phase 0/1 and fail if it exceeds the existing startup deadline. Full-tree
rehash is not silently added to every confined spawn; other file corruption is
caught on install/reuse, not promised universally at start. The receipt lists
what was checked. The owner resolves the real PBS distribution root; host
resolver children and `unshare` run that absolute executable with
`-I -B`; the existing bounded-output/time/kill and clean-env rules remain. Pass a
validated runtime mount description in supervisor-owned config, not from a relay
or remote peer. Bind only that PBS distribution read-only at `/marmish-python`
inside bwrap, invoke its fixed `/marmish-python/bin/python3` with `-I -B`, and
verify resolved base prefix matches the mount. Do not mount the installation
prefix, user HOME, runtime receipt directory or sibling identities into the
sandbox. Internal PBS symlinks must stay within that distribution. Validate
runtime selection/config in the supervisor before namespace UID remapping;
the outer process consumes only the validated supervisor-owned description.

Before approving this change in code review, audit actual candidate `readelf`
(or `otool`) dependencies, extension-module loading and Python startup file
access from phase-0/native evidence: do not assume PBS is entirely static or
that its stdlib alone suffices. Linux retains existing `/usr` system-library and
certificate mounts; add only the reviewed PBS root, no broader filesystem or
network access. macOS's existing resolver uses `sys.executable`; verify its
managed dependencies and assert no native broker profile change is needed:
Python runs host-side; do not add its distribution to native sandbox read rules
or allow new network origins. The native daemon stays separate from Python.

Doctor reports executing managed Python identity and managed helper-runtime
readiness, replacing the mandatory Linux system-Python check for new archives;
retain all other native prerequisites and the static-only disclaimer. Actual
confined startup/negative tests must still prove capabilities dropped, no DNS
or external routes inside, permitted-origin forwarding only, blocked origins,
no new profile/runtime writes and byte-preserving command operation. Mask or
remove `/usr/bin/python3` inside the native test environment as well as PATH;
otherwise a passing doctor/helper test does not prove independence. A platform
unable to run required confinement tests remains explicitly blocked, even if
installer and static probes pass. Detected runtime integrity failure refuses startup.

Old archives cannot acquire these code changes through a wrapper. They remain
installable with explicit legacy system-helper prerequisites; managed frontend
launch alone is not advertised as complete system-Python independence. Newly
built archives are required for that claim and its actual confined acceptance.

## Setup ownership and older archives

Move existing quickstart orchestration into the verified app module with
`SETUP_API_VERSION = 1` and explicit `setup_main(argv)`/`marmish --setup` dispatch;
use existing module payload names to avoid needless release-schema changes.
Installer quickstart opens/validates the controlling foreground terminal before
installation, closes download temporaries/releases locks before setup, then
calls the absolute installed launcher with `--setup` (and `--debug` as requested).
App setup reacquires the terminal and owns existing bounded signal/child cleanup.
`--setup` is mutually exclusive with commands/init/doctor; normal destination
syntax and bytes after `--` keep their existing meaning.

For old released archives, inspect only the verified app module's setup API
constant with `ast.parse` under the fixed interpreter: permit only one literal
integer assignment at module scope, reject duplicate/nonliteral declarations,
and never import archive code for capability discovery. Read the managed-helper
constant separately by the same rule; absent means legacy helper prerequisites.
Setup API 0/missing means invoke the retained installer
legacy quickstart implementation; API 1 delegates to the app; unsupported API
fails explicitly. Do not infer capability from arbitrary failure or swallow a
failed setup into fallback. Existing `.1`/`.2`-style archives remain installable
with their current payload and current-role foreground setup. Tests bind each
capability path to exact fixture content. New archives advertise only implemented
behavior. A legacy-helper Linux archive prints its system-Python requirement in
the install summary; quickstart fails early with that category when unavailable,
before the ordinary doctor/init sequence. Server authorization remains
invocation-only in this increment; durable config migration belongs later.
Identity initialization/keychain errors remain failures; no fallback secret file,
replacing existing keys or hiding confinement errors. Setup may repeat safely.

## Validation, CI and release gates

Extend `.github/workflows/installer-validation.yml` (push to master, PR, manual),
keeping pinned checkout, minimal read permissions, fail-fast false and native
`ubuntu-24.04`, `ubuntu-24.04-arm`, `macos-15` matrix. Increase the 10-minute
timeout to a bounded value justified by phase-0 measurements; add path triggers
for runtime pin data, setup/app, shared launchers and acceptance helpers. Preserve
existing mocked/offline archive and PTY tests; add actual managed-bootstrap jobs
using phase-0-frozen artifacts and bounded native archives, not only imports of
the Python body. A CI testing interpreter may drive assertions; the installer
subprocess must have no usable system Python, and installed launchers must keep
working after uv/download/cache removal. No untrusted PR receives release secrets.

Required evidence per native target:

- Fresh real shell published-archive install; old/new capability wrapper tests;
  installed help/doctor, matching native helper versions/signatures/libraries,
  exact argv including quotes, newlines, dollar signs, Unicode and spaced paths.
- Actual verified-archive install under managed Python; full relevant tools suite;
  setup c/s, keychain-auto/init argv, foreground/no-terminal denial, EOF/invalid
  input, cancellation/repeated signals, child reap and unchanged old profile.
- Hostile environment/config, failing PATH python sentinel, digest mismatch,
  null/missing hash, redirect host/scheme/hop failure, missing/truncated/oversize
  artifacts, wrong target, traversal/link/archive schema and ownership rejection.
- Concurrent installers, repeat install, runtime tampering, upgrade failure at
  each publication step, SIGKILL recovery, format-1 migration, retained-version
  independence, uninstall bootstrap failure without mutation, owned uninstall
  retention and refusal of unrelated launchers.
- No real-home uv cache/config/bin changes; real profile/keychain untouched by
  plain install. Linux enforced network-denial restart; macOS offline policy and
  deleted downloader evidence recorded without claiming independent denial.

Keep fixture seams internal: no arbitrary production origin/hash override or
insecure flag merely to serve test bytes. Distinguish evidence precisely:

- The unchanged real shell entrypoint installs frozen published `.2` assets.
  With `/usr/bin/python3` masked, legacy Linux doctor must fail exactly its
  `system_python_3_11` check; static prerequisites otherwise remain visible.
  This is compatibility evidence, not managed-helper independence.
- A test-copy wrapper redirects only the installer release source to a local
  newly built verified archive, following the existing quickstart test technique.
  Assert every other shell/body byte is identical; run real uv/bootstrap/parser/
  install/publication paths. Report it as wrapper evidence, not public download.
- Native builds for these new-archive gates run in the existing release/native
  build jobs, handing off exact same-run/attempt bundles; ordinary PR installer
  CI keeps unit/fake seams and frozen-published-archive coverage. No fake-helper
  fixture substitutes for new-archive independence/confinement release evidence.
- On hosted Linux use a privileged private mount namespace (`sudo unshare --mount
  --propagation private`), bind a refusing sentinel over `/usr/bin/python3`, then
  drop to the original runner UID for installer/transport execution. Never modify
  the host mount namespace or run installer as root. Keep the namespace until
  children exit. Separately mask PATH python. If namespace creation or nested
  confinement is denied by Ubuntu policy, mark the independence gate blocked;
  do not weaken host security or claim PATH-only masking proves the requirement.
  This privilege is CI-fixture isolation only, not Marmish root execution or
  authorization for implementing privileged Unix-user mapping.

Implementation/acceptance depends on phase 0 passing hash/mandatory-hash and
negative uv tests, full `tools` tests under 3.13.13 and offline retention on all
three native targets. Its evidence must include PBS Python/libpython/every
extension's NEEDED dependencies, layout, file count/bytes/symlinks and runtime
verification cost inputs. Missing platform evidence blocks that platform gate;
no success is inferred from API support or build completion.

Phase-0 published-archive doctor with system Python present records
`system_python_dependency`; its passing packaging proof cannot satisfy these
new-archive gates. Native helper smoke is not proof of Marmot execution or
confinement; retain ARM/macOS warnings and report the known macOS abort separately.
Before closure: Claude auto-mode integrated code/test review, sanitized per-target
CI evidence and updated agent-spec/README/build-plan, `.gitignore`/staged secret
audit, then commit/push to GitHub and Radicle with verified refs/Iris replication.
A new binary release requires its existing scoped publication authorization and
native release gates; source bootstrap delivery does not invent platform support.

The CI namespace fixture must explicitly assert nonzero euid/egid, empty
supplementary groups and cleared capabilities before test execution; scrub
`SUDO_*` variables. Its sentinel is in runner temp. It never changes host sysctl,
AppArmor policy or host mounts.
