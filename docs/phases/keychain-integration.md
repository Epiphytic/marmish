# K2: keychain selection and private secret handoff

Beads `marmish-d84.2`; depends on the reviewed K1 provider. Claude approved this design in
[the independent review](../reviews/keychain-k2-plan-approved.json). Native key storage must preserve
existing confined transport, durable command ownership and disconnect semantics.

## Frontend selection and named identities

Expose `--key-storage auto|native|files` (default auto), `--key-name NAME`,
`--keychain-helper PATH`, and explicit `--migrate-keys-to-native`. Keep `-i` as
profile directory. A private per-user catalog records named Nostr
identities as immutable references bound exclusively to one profile; SQLCipher masters are independently generated
and bound to each server engine root. Names identify local keys, not remote
execution authority. An explicit name selects a catalog entry; an existing
profile cannot silently switch identity. Without a name, a new profile gets its
own generated named identity. Multiple names can coexist and server/client use
identical selection rules. Catalog location follows XDG_CONFIG_HOME (Linux) or
~/Library/Application Support/Marmish (macOS), with an explicit catalog option
for isolated tests. Enforce private directory/file ownership, no symlinks,
bounded strict JSON, advisory locks and durable atomic publication. Names must
be unique in this catalog; native labels are non-authoritative.

For a fresh profile, auto probes the native helper: available selects native;
only proven absent selects files and announces that choice to stderr. Present
locked/denied/uninitialized/ambiguous/failed backend fails. Native demands native;
files explicitly selects existing file behavior. Persist selection before any
secret-dependent state. Existing profiles remain file-backed unless explicitly
migrated; auto never regenerates identity or master. A saved native reference
never falls back. Unexpected mixed files/references or explicit backend conflict
fails. Helpers run with bounded private stdio, sanitized diagnostics, absolute
validated executable path, no payload argv, an outer deadline and response cap.
Preserve only host keychain discovery variables; helper subprocesses close other
FDs. No key material enters environment or logs.

Provisioning journals random references before native creation, reads back and
validates exact secret/public identity, then publishes the final profile binding
before database creation. Recovery may load only an exact pending reference in
pristine storage; missing/ambiguous or malformed entries never replace existing
state. SQLCipher raw masters never share merely because a name matches.

## Engine and detached workers

Keep existing `Engine::open` file behavior for offline embeddings. Add an explicit
provided-master entry point used by the bridge in native mode. Its private first
stdio configuration carries exactly32 key bytes plus immutable key/root binding;
these fields are not remote protocol or debug output. Under the existing engine
frontend lock, validate a private versioned metadata marker before SQLCipher open.
Native mode refuses engine.key or pending plaintext key; file mode refuses a
native marker. Nonempty state requires the same reference; no silent bootstrap.
StorageKeys remains the sole in-memory key boundary and cipher/spool derivation
stays unchanged.

All worker launches use a small versioned one-shot stdin frame, filled and writer
closed before process execution. Bind job ID and root device/inode alongside the
master; worker validates both against its opened root before SQLCipher or job
claim. Bound receive time and exact frame size, reject trailing bytes, then close
and replace fd0 with /dev/null before running a command. Parent retains no pipe
endpoint after spawn. Use the already-open Engine key for initial and reconciled
launches. Keep env_clear, FD closure, retries, launch uncertainty and process
reaping semantics. Detached workers already holding keys continue through frontend
exit or keychain lock; frontend reopen while locked fails without rerunning jobs.

## Confined MDK

Add `--secret-store marmish-stdin` / JSON `MarmishStdin`.
Maintain a reviewed patch for both platforms: read exactly one small versioned
Nostr secret frame from daemon stdin, close it, validate the derived public key,
and inject an immutable in-memory AccountSecretStore through the central CLI
open_account_home helper. Reject different identities and writes of different
keys, and treats bound-account removal as a rollback-safe no-op. Audit and reject any secret-export CLI operation
in this mode; a store alone cannot prevent other CLI code exporting its loaded key. Existing File behavior remains
only in explicit file mode. Every account-home instance inside the daemon uses
the same injected store; no host D-Bus or securityd access is added. Secret bytes
are zeroized where owned. Python/serialization creates immutable copies; do not
claim complete secret memory erasure. Readback compares raw32 exactly as well as
x-only public identity (opposite scalars can have the same x-only public key). The private RPC Login import path provisions the
selected key through the existing account-setup journal; verify returned public
identity before finalizing the profile; pending references and derived public identity
are persisted before daemon launch. On restart, even when skipping Login, the saved account must match
the supplied key. Import payloads remain private Unix-socket data, never argv.

The daemon frame is exactly72 bytes: magic `MRMNST01`, secret32, expected x-only
public32. Add a stateless `derive_nostr` helper action using MDK's pinned
secp256k1 version0.29.1 to validate scalars and derive public identities without
accessing any vault. Validate before creating the native entry and again after
readback. Do not rely on TOFU for the expected identity. Nostr Login sidecar
accepts nsec encoding; encode private32 only in private memory, never argv.

Pass the bounded frame via fd0 through Linux outer namespace, Bubblewrap and
_inner child and through Darwin lifetime shim; close/replace it after one read.
No command children inherit it. Source attestation covers all changed CLI/account
sources and manifests, and provenance records the new patch on both platforms.
Rebuild pinned MDK and run exact account/in-memory-store tests plus transport
confinement checks; an unpatched binary must reject native mode before use.

## Explicit migration

Migration holds exclusive profile/catalog and engine locks, checks worker
quiescence (refuse migration while jobs are running), reads existing keys through
protected no-follow descriptors, creates unique native entries and verifies key
and public identity, publishes a resumable migration journal, and atomically
switches references. Only after reference commitment remove owned plaintext key
files. Preserve originals until that commitment; any uncertain provider outcome
retains recoverable state and fails rather than selecting a new key. On restart,
recognize journaled mixed state and complete cleanup after readback. Do not claim
secure erasure from unlink on journaling/COW filesystems. Rollback after native commitment is out of scope; retry cleanup explicitly. Migration tests cover failures around
every publication boundary and wrong key/identity with unchanged encrypted state.

## Validation and ownership

Delegate frontend/catalog/helper controller to one agent, engine/bridge/worker
to another, MDK patch/build/confinement handoff to another; root coordinates shared
interfaces, docs, actual tests and review. No overlapping file edits. Tests cover
multiple names and selection, sticky defaults, lock/absence/errors, malformed
references, migration/restart faults, no argv/environment/FD secret leaks, partial
worker frames and detached completion/output recovery. Actual isolated Linux
keychain tests and real disposable Marmot execution are mandatory. Native macOS
checks remain a separate explicit gate if runners cannot execute them. Obtain
Claude detailed design approval before implementing K2, then integrated auto-mode
code review, checks, exact staged audit and commit/push.

## Review clarifications (B1–B8)

The derive_nostr action is a K2 additive extension of the frozen K1 helper schema,
not a K1 claim. Validate scalar and derive P before vault create; publish the
verified key/public binding before any MDK open, then finalize account after
matching Login/Whoami. Compare any existing public AccountHome metadata to P
before invoking MDK setup recovery; refuse multiple/different accounts or setup
journals during migration. Catalog binds a Nostr reference exclusively to one
persistent random profile ID; a second profile cannot adopt it (multi-device
remains gated). Named key selection can reopen its bound profile; changing -i
is not enrollment authorization. SQL markers include independent persistent
random engine-root IDs; dev/ino are an additional per-launch worker binding.

Injected store method table: has_secret_for_label=false;
has_secret_for_account_id=true only for injected P; load_secret only for bound P;
write_secret accepts exact bound account and raw32 as idempotent no-op, otherwise
error; remove_secret for bound P is an Ok no-op for setup rollback, never a vault
delete, and rejects unknown identities. No environment/default-keychain/file
fallback in injected mode. Startup capability/Whoami RPC reports mode and P;
existing committed accounts are verified without reimport. Implement the Python
MdkRpc import_nsec sidecar and Login shape explicitly; upstream daemon support
already exists but the Python adapter currently does not expose it.

Use two maintained patches: Darwin-only restricted transport and both-platform
key bootstrap. Source guard v2 pins per-platform digests of complete cli/src,
marmot-account/src and marmot-app/src plus their manifests and Cargo.lock.
Manifest records injected-key capability and exact patch hashes and is required
on native Linux as well as Darwin. Missing capability, wrong mode or wrong P
fails before account mutation. Test exact setup-journal transitions in pinned MDK.
Linux prefilled fd0 goes only to unshare; outer ip/sysctl/self-test commands use
DEVNULL. Darwin shim parent replaces its fd0 after fork; child inherits reader.
Seatbelt explicitly permits inherited bootstrap read. Daemon consumes before
runtime/socket startup with deadline, exact EOF and replacement fd0=DEVNULL;
there is no reexec. Each launch gets a fresh pipe, never a reusable endpoint.

Engine marker is `engine.native.json`, strict version1 JSON with backend native,
purpose sqlcipher, immutable reference and random root_id. Native marker publishing
uses protected pending file plus exclusive platform publication and directory
sync. File mode: engine.key present, native marker absent, no native pending.
Native mode: exact marker/reference/root_id, no engine.key or plaintext pending.
Fresh native root: no nonempty database/jobs/legacy files; publish marker before
DB create. Existing nonempty root missing marker or mismatching binding fails;
wrong supplied key must fail SQLCipher authentication without key regeneration.
Both frontend and worker validate mode from disk: worker frame includes explicit
mode, reference/root_id (native) or fixed file-mode tag, job and dev/ino. Worker
compares marker before SQL open/claim. Create/close the pipe in spawn after durable
worker-start reservation, using std::io::pipe with owned close-on-exec handles;
all error paths drop both endpoints. Fill <=PIPE_BUF and close writer pre-exec.
Preserve descriptor closure on both platforms; no Darwin pipe2 assumption.

Migration states are PENDING → NATIVE_CREATED → COMMITTED → CLEANED. PENDING
journals exact refs, existing public identity and original backend before creates;
NATIVE_CREATED records verified raw bytes/public identity through refs only;
COMMITTED is one atomic profile.json publish naming both native bindings. No
persistent journal contains secrets. Cleanup after commitment publishes the exact
engine marker then unlinks legacy masters/Nostr files and syncs directories.
Only this exact journal authorizes temporary mixed storage; arbitrary mixed
files/references fail. Recovery re-loads native entries and compares original
raw bytes while they exist, then finishes marker/unlink; no regeneration.

Before provisioning migration, hold profile→catalog→MDK-home locks and engine
frontend plus all worker/I/O locks with terminal-process validation. Use existing
Engine::into_cleanup/CleanupReady quiescence machinery, retaining its guard through
commit; extend local bridge with narrowly scoped migration commit under that
guard. Do not send cleanup_ready over Marmot. For a client without engine state,
only profile/catalog/MDK-home locks apply; client entry with engine state must
refuse migration and direct to server context for quiescence. Reject setup journals,
multiple/different MDK accounts and legacy pending keys. Existing-engine migration
recovery may open exact legacy file under the authorized migration journal only,
matching the native entry byte-for-byte; it cannot start/reconcile commands until
migration is clean. Rollback after native commitment is out of scope; preserve
native refs and resume cleanup. Before commitment failures retain originals and
journal for explicit retry, without trying unrelated orphan references.

## Integration API and sequencing

The keys module owns durable profile publication through KeySession.commit(value),
so no caller can omit binding persistence before database launch. Root supplies
public profile fields; key bindings are immutable once committed. Named selection
requires native mode; explicit files or auto absence with --key-name is an error,
not an ignored name. Catalog records canonical profile directory plus dev/ino and
random profile_id; copied/moved profiles are refused pending an explicit future
move operation. This gate avoids unsupported duplicate MLS enrollment.

Native profiles persist account=P with initialized=false before daemon launch.
After authenticated capability check, query Whoami: if exact account exists,
verify P and resume; if absent, perform Login with private import_nsec sidecar.
Only matching success publishes initialized=true. Existing account metadata is
prechecked against P before daemon starts; bound pending setup may resume only
for the same identity. Explicit migration refuses any pending setup journal.

Shared APIs: ConfinedDaemon(..., bootstrap: bytearray|None=None); bootstrap
ownership transfers to transport, which wipes it on start success/failure.
MdkRpc.capabilities sends raw JSON string `MarmishCapabilities` through the same
peer-authenticated socket/framing; decoded result is exactly version1,
secret_store=`MarmishStdin`, public_key=P (File mode reports File/null).
Injected daemon eager and periodic reconciliation remain gated until a verified
operational request, with metadata validation before any mutation. Capability
request itself is mutation-free. Handshake failure closes the daemon.

KeySession.prepare_nostr returns backend/profile_id/reference/account/secret;
prepare_master returns backend/profile_id/root_id/reference/secret and no master
in files mode. Secret dataclass fields have repr disabled. Public methods omit
secrets. Keys module owns helper derive_nostr action extension and protected
catalog/journals; root owns Profile/server orchestration and MdkRpc adapter.
Engine agent owns all engine/bridge/worker files; daemon agent owns patches,
transport and builder/manifest/source-guard plus their tests. Root owns lockfile
and workspace integration. Agree bridge wire fields before edits.

Migration cannot use normal Engine::open because it reconciles. Add a dedicated
first bridge operation that opens existing legacy storage only to authenticate,
enumerate and acquire quiescence, without initialization, config mutation or
reconciliation; retain the consuming guard with Store closed. Return the legacy
master only over this private bridge after locks and quiescence are established.
Keep that guard until a narrowly scoped migration commit validates the expected
native binding and marker/legacy bytes, publishes marker, unlinks legacy key and
syncs. If recovery finds the authorized marker plus legacy key, this same path
compares both and completes without launching anything. Normal remote operations
are disabled on a migration bridge. Only exact COMMITTED journal/profile/native
reference authorizes this recovery, never an arbitrary mixed state.


## Concrete engine bridge contract

Bridge configuration v2 adds `storage`: `{mode:"files"}` or
`{mode:"native"|"migrate_native",root_id:<hex64>,reference:{version:1,purpose:"sqlcipher",id:<hex64>},master:<canonical base64 raw32>}`.
The legacy version1 configuration without storage stays file-backed.
`migrate_native` is an exclusive first initialization path that authenticates and
quiesces without reconciliation; `migration_commit` takes no binding arguments.
The root supplies the protected legacy master after reserving journaled target
references; the guard validates that master and the unchanged legacy file before
commit. The earlier conceptual return-master operation is unnecessary.

Worker frames are exactly144 bytes: magic8, mode1, reserved7 zero, job16,
root device8/inode8, root ID32, key ID32 and master32. File mode uses zero IDs.
Strict typed bridge initialization rejects duplicate/unknown fields and trailing
data; owned secret-bearing buffers are zeroized and debug output is prohibited.

Fresh files-mode selection is durably recorded in `keys.mode.json` before daemon
startup, bound to the profile path, device, inode and random profile ID. An auto
retry honors this selection even if a native service later appears. Explicit
migration removes this nonsecret marker only after committed cleanup.
