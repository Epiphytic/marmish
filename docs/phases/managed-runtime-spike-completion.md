# Packaging proof completion and native diagnostics

Amendment to [increment 0](managed-runtime-spike.md), Beads `marmish-x6z.1`.
Design status: initial Claude verdict NOT APPROVED; the reviewer explicitly
approved conditionally without another round once all seven corrections and
essential test conditions below were incorporated. They are incorporated here;
implementation and native acceptance remain pending. See the
[review disposition](../reviews/managed-runtime-spike-completion-design.md).
No new production behavior, host policy change or gate waiver is authorized.

Source baseline: `dd9abc7`. Local Trial12 passed its implemented gates; native
run 41136583828 passed original installer jobs but failed managed packaging.
Linux failed two actual namespace tests; macOS failed the native-help call.
The failed report omitted the native exit, so its actual cause remains unproved.
Keep that evidence unchanged; add new reports alongside it, never overwrite it.

## Correct the macOS gate explicitly

This replaces the original macOS **native `--help` execution** gate; Linux still
requires native help exit 0. Source inspection establishes macOS `wnd` calls
`initialize()` before argument parsing and requires `MARMISH_RESTRICTED_CONFIG`.
The allowlisted probe environment omits that variable. Expected macOS behavior
is exit 1 and exactly `marmish_restricted_start_failed\n` across combined
stdout/stderr, with no extra bytes. A signal, timeout, exit 0, different output
or whitespace fails. Do not supply real config or create acknowledgment/identity
state to make this packaging probe pass. Real configured macOS `wnd` startup is
deferred to the later bootstrap/confinement increments. This probe proves signed
binary loading, dyld resolution and reaching the guard; not help or confinement.

Bind this expectation to the extracted artifact: require its daemon manifest's
`patch_sha256` to equal the SHA256 of the verified restricted patch and require
`restricted_protocol == 1`. Retain existing archive/signature/source/hash checks.
The patch is Darwin-only; never apply this expected refusal to Linux artifacts.

Make report fields platform-specific: Linux `native_probe_mode` and
`native_execution_probe` describe native help plus manifest provenance; Darwin
uses `restricted_startup_guard_refusal`. Emit a successful mode only after exact
exit/output validation for both slots. Replace Darwin's unconditional
`installed_help_doctor_native_help_and_literal_argv` assertion with a fixed
assertion naming the restricted guard instead of native help. Leave Linux's
successful-help assertion intact. No report may imply macOS native help passed.
Run both slots before raising. Failure diagnostics contain only fixed slot labels
`slot_1`/`slot_2`, nonnegative integer `exit_code` or null, positive `signal` or
null, boolean `marker_exact`, and bounded output byte count. Export no bytes.

## Linux probe ladder mirrors failing tests

Before the full tools suite, run each fixed probe independently with the exact
suite `test_env`/PATH, ten-second deadline, output limit and owned process-group
cleanup. Darwin runs none. Keep the full suite unchanged after collecting all
probes, including both required namespace tests; a passing ladder is diagnostic,
not a replacement for those tests.

1. Presence check for fixed `ip` and `sysctl` tools.
2. Bare `unshare --user --map-root-user --net` running true.
3. bwrap alone with the tests' exact `--unshare-user --unshare-pid --uid 0
   --gid 0 --cap-drop ALL --proc /proc --dev /dev` flags and read-only host-root
   arrangement, running true. Preserve other required test flags/order.
4. The same outer unshare wrapping the same bwrap stage. Within the isolated
   outer network namespace, reproduce the tests' `ip link set lo up` and
   `sysctl -q -w net.ipv4.ip_unprivileged_port_start=0` preparation before bwrap.

Read and export the existing workflow's three policy values as bounded integers
or null with fixed unavailable categories: `apparmor_restrict_unprivileged_userns`,
`unprivileged_userns_clone`, `max_user_namespaces`. Never write them. Report only
fixed operation names, exits/signals, missing-tool/timeouts and optional fixed
hints for literal permission-denied/operation-not-permitted messages. No raw
output and no causal claim from a substring. Ubuntu's distro bwrap AppArmor
profile permitting user namespaces while bare unshare is denied is a hypothesis
this ladder tests. Any self-hosted-runner switch, workflow sysctl change or
capability skip proposed afterward requires its own design review. No root
elevation, host policy modification, identities or relay traffic in this work.

## Complete executable interruption gate

Add internal `observer` to `run()`, called in the same bounded directory walk
that computes `total_bytes`, not a second rglob pass. Cap at 50,000 scanned
entries per poll and a fixed observation deadline within the existing command
timeout; poll every 10ms once staging appears. Discover the actual uv staging
location empirically, never hardcode it. Match only a regular distribution file
with a known suffix outside every final `cpython-*` slot path. Give this case its
own complete `environment(case_root)` so HOME/TMPDIR debris is case-owned.

Before killing require `child.poll() is None`; SIGKILL the owned process group,
reap and require `returncode == -SIGKILL`, observed true, and `os.killpg(pid, 0)`
raising `ProcessLookupError` after a bounded cleanup wait. Surviving descendants
fail. Distinguish fixed failure categories: `finished_before_interruption`,
observation timeout and observer exception. An exception fails closed with the
whole group reaped; a naturally failing child is not an interrupted extraction.

Do not use `discover_interpreter` for non-exposure: it misses staging paths.
Require no `cpython-*` entries or minor-version links under `slot/python`, no
interpreter outside the known staging tree, and no launcher directory. Delete
only owned case debris. Retry into a new slot (never reuse an interrupted one),
prove it succeeds and assert the old debris is not visible from the retry.
A real early completion is a failed gate, not grounds to accept a mocked kill.

## Complete substitution and cwd gates

Add `after_rehash=None` to `install_python`, invoked exactly once between
`verify_asset` and uv launch. Pass in-process only, never via argv/environment;
the substitution case fails if not invoked. Copy the archive to a new inode and
change only gzip MTIME bytes 4–7. Verify equal decompressed streams with bounded
streaming; no full decompression/recompression in memory. Atomically replace the
case archive with this regular mode0400 same-owner substitute after the primary
rehash. Original expected metadata digest stays unchanged. Require actual uv
rejection: privately match `Expected:` to the original SHA256 and `Computed:` to
the substitute's actual SHA256, not arbitrary hex. No final interpreter/launcher.
This intentional second-layer test is the sole reviewed exception to primary
verification of the exact bytes uv reads; never execute a substituted artifact.

Add `cwd=None` to `run()`, default HOME. Require an absolute existing non-symlink
owned directory whose resolved path is relative to `owned_root`, before spawn.
Place invalid `uv.toml`, `[tool.uv]` pyproject and bogus `.python-version` in
child cwd/parents, plus invalid `uv/uv.toml` under the private config directory.
Prove explicit child cwd isolation against these decoys, not just HOME config.

## Ambient state case and restoration

Use slot one's install as clean control and slot two's real install as hostile
positive; require identical interpreter identities. Inject only UV/PIP/PYTHON,
LD/DYLD and MARMISH sentinel variable families into the parent. Never change
HOME or PATH: the real-home snapshot uses them. Resolve `--report` absolute at
`main` entry. Save caller cwd through an fd and restore with `fchdir`; restore
environment in finally on success and exception. Any restore failure sets
`ok=False`, never produces a passing result or loses the report path.

Keep the injection window from just before slot-two install through installed
launcher checks; end it before `full_tools_acceptance` compiler discovery.
Point `UV_CACHE_DIR` and `UV_PYTHON_INSTALL_DIR` to empty owned directories which
must stay empty; point `UV_PYTHON_DOWNLOADS_JSON_URL` to invalid owned metadata.
Assert actual child environment keys equal the allowlist. A missing-file
`MARMISH_RESTRICTED_CONFIG` sentinel detects leakage via failed Linux help;
macOS's identical refusal marker cannot detect it, so assert the allowlist.
Observe no sentinel use and preserve literal launcher argv under the hostile
caller cwd. Never read or alter actual user configuration.

## Essential regression tests and acceptance

- Private output never enters reports; diagnostic fields/types/size are fixed.
  Missing tools/timeouts have fixed categories; Darwin runs no Linux probes.
  Linux still requires help exit 0. Darwin exit 0, exit 1 with wrong/extra output,
  a signal, timeout and extra marker whitespace all fail.
- Interruption: fake uv finishing early yields `finished_before_interruption`;
  throwing observer fails with reaping; surviving group descendant is detected;
  natural nonzero exit never counts as intentional interruption.
- Substitution: uncalled callback, invalid gzip replacement and uv accepting
  changed bytes (`negative_accepted`) all fail.
- cwd: outside-root, symlink and relative paths fail before spawn.
- Ambient state: environment/cwd restored after success and exception; restoration
  failure forces `ok=False`; absolute report path remains valid.

Run focused tests normally and with symlinked TMPDIR. Then make one real bounded
local proof with Linux's complete ladder and all three executable cases actually
executed, not mocked. Remove unmet fields only for actual passing gates. The next
native run must record macOS's exact numeric exit and `marker_exact: true` with
correct mode/assertion; anything else is a precise blocker, never an inferred fix.
Claude reviews integrated code/evidence before dual-remote publication and the
next native run; preserve sanitized failed and successful reports side by side.

Design corrections do not close phase 0: all three native targets must pass the
original gates, including both Linux namespace tests with no waiver. The separate
native confinement SIGABRT remains open. macOS gets neither independently enforced
offline denial nor proof of native help, identity initialization or confinement.
