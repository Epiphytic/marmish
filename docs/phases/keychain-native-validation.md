# K3: native keychain and real Marmot validation

Beads `marmish-d84.3`; K2 must pass integrated code review before this phase closes.
Independent Claude design review approved this acceptance design; see
`docs/reviews/keychain-k3-plan-approved.json`. Existing provider/engine focused
tests remain K2. Linux acceptance can complete independently, with native macOS
gates recorded separately as pending. Commit reviewed K2 before running acceptance;
report the additional K3 harness diff and its digest.

Use fresh disposable server/client/intruder keys and explicitly selected relays
`wss://relay.eu.whitenoise.chat/` and `wss://relay.us.whitenoise.chat/`, as authorized
for testing. No default relay selection is introduced. Use a clean pinned MDK
build with the K2 bootstrap patch, mandatory provenance, and actual confined
client/server execution. Reuse the existing real Marmot harness assertions for
special argv bytes, separate binary streams, cancellation, retries/replay denial,
room purpose, frontend disconnect and worker completion, durable MLS/output,
new sessions/retention, and actual CLI processes.

Linux runs against a private D-Bus session with activation directories disabled,
owned temporary HOME/XDG directories and GNOME Keyring state. Assert bus socket,
uid and root ownership before native calls. Initialize only that owned service;
never inspect, unlock or change the ambient desktop keychain. Pass only bounded
native discovery variables to test child frontends and helper; no API tokens,
ambient proxy variables or user state. Native key names differ per disposable
profile; the server SQLCipher reference is distinct from every Nostr reference.
Each new profile uses auto mode (explicitly testing the default). Confirm actual
native references and absence of MDK secret.json/engine.key at rest. Restarted
frontends must recover the same identities and persisted command output.

Add an explicit migration acceptance run starting in files mode. First execute
and complete a real command through Marmot; quiesce, migrate both endpoints,
verify exact public identity and byte-preserved encrypted state/output, confirm
legacy files absent, then execute another command and reconnect. Never change a
master or identity to repair a failing run. K2 offline tests cover intermediate
journal recovery, wrong/missing keys and malformed worker/bootstrap frames.
A locked native keychain must prevent a new frontend from starting without
fallback; already running workers must complete from their in-memory keys.
Unlock only the owned test collection and verify recovery without duplicate work.

Bound live workloads, total runtime and stage deadlines; record fixed sanitized
failure categories and assertion results, executable/source/manifest digests,
public identities and relay URLs only. Keep all raw keys, runtime databases,
command journals, output and provider state outside the repository. Clean up
owned processes and temporary state on success/failure without killing unrelated
processes; preserve failure evidence only if it contains no private data.

Native macOS remains a distinct runner gate: run the existing disposable
explicit-keychain provider tests, including helper replacement ACL behavior,
then the native patched-daemon confinement/bootstrap tests. Full frontend native
validation requires a dedicated owned default keychain context; never modify the
runner user's default keychain or search list to make a test pass. If that context
is unavailable, report the gate pending rather than treating static/cross-platform
checks as runtime evidence. Linux/macOS interoperability can reuse explicit files
mode as a separate transport gate; it does not prove native keychain support.

Exit: independent Claude auto-mode review of the integrated result and sanitized
validation evidence; fix useful findings; document any unresolved native runner
limits explicitly. Commit and push reviewed validation changes and record exact
results in Beads. The feature must not be called fully validated on macOS while
its native gate is pending.

## Independent review clarifications and acceptance matrix

Parameterize the existing harness's explicit files choice: native acceptance uses
one shared owned catalog and keychain, distinct server/client/intruder names,
and an unnamed auto profile. Test reopening each name with its bound profile,
refusing a different name on an existing profile and refusing adoption from a
second directory. A new unknown name on a fresh profile intentionally provisions
a key; it is not an error. Changing `-i` to a fresh unnamed profile intentionally
creates another identity. The catalog contains Nostr references only; assert its
exact expected entries and separately assert the server SQLCipher reference in
its profile/marker. Assert all provider entries correspond to those references,
without duplicates or orphan entries. Concurrent catalog startup may return the
fixed busy/lock result; an explicit bounded retry must preserve references and
identities. Test that no native named selection is ignored in files/absent mode.

The default matrix includes private-bus service absent (files plus announcement),
present uninitialized (failure), fresh locked (failure), existing files with
native present (sticky files), and native saved with service absent/locked
(failure, no fallback). Exact categories/deadlines/no secret files and no native
entries on refused operations are asserted by the provider, keys and integration
tests plus the live owned-provider run. Public mode/lock files and pending
nonsecret references are recoverability metadata, not forbidden secret artifacts.
Missing prerequisites are pending, never a passing skip.

Inspect live owned process argv/environment on Linux for raw, hex, base64 and
nsec key forms. Validate MDK and workers' sanitized environments omit the host
bus address; workers and injected daemon fd0 become /dev/null after bootstrap.
The bridge intentionally retains its private RPC stdin pipe, and the short-lived
helper intentionally receives its request over stdin. Inspect their intended FD
sets accordingly. K2 confinement acceptance must prove the MDK daemon cannot
reach host paths/bus sockets. Remote commands are authorized programs running as
the local user and are intentionally not sandboxed; they can reach that user's
files or services if programmed to do so. Do not invent a confinement promise
for them. Their inherited environment and secret handoff descriptors remain
restricted as specified. Scan profiles/catalog/journals/engine/MDK and sanitized
logs/report for known test secret forms, excluding only the owned provider's
credential storage itself. Never save the secret needles in the report.

For both native endpoints, check MarmishStdin capability, exact P and Whoami
before use and after restart/migration. Refuse a missing-capability manifest
before account mutation (K2 negative). Bind reports to the reviewed K2 commit,
source guard, patch and executable digests; report any additional harness diff.

Migration acceptance keeps the same identity and MLS group, fingerprints quiesced
SQLCipher/spool files before and after the migration transaction, and verifies
exact retained output and subsequent command execution. Refuse migration with
an active worker; preserve legacy bytes, refs and encrypted state. Retry uses
the same reserved refs. For lock/disconnect, a mid-job command writes a single
`once` marker; lock the owned collection, stop frontends, wait for completion,
assert both reopen attempts fail without files or rerun, then unlock and recover
that same invocation's output and single marker. K2 fault tests cover COMMITTED
cleanup recovery; real migration must not bypass these boundaries.

Check exact provider reference counts before teardown. Provider state is removed
with its entire owned temporary context after stopping only owned PIDs/process
groups with verified identity; passwords go via stdin. Verify owned processes
are gone and private paths removed. Each live scenario is capped at 900 seconds,
with a total 2400-second budget including stage cleanup; timeout is a failed or
pending result with its fixed reason, never a passing assertion.

macOS helper default selection requires a disposable dedicated user/VM with its
verified default keychain path inside that owned account. HOME/XDG overrides
alone do not isolate securityd. Without that context, run no default-helper
native call and emit `macos_native_default_keychain: pending` plus missing
query/add/status/ACL/default/injected-runtime gates. Explicit-keychain tests are
provider-only evidence. Report Linux validated and macOS pending separately.
Reports allow only outcome/platform/scope/commit/digests/relays/public identities,
assertion names/timings/fixed stages/categories/cleanup status and limitations;
no raw command data, runtime paths, credentials or provider stdout.
