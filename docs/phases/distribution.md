# Prototype distribution and publication

## Scope and phase boundaries

The user authorizes public release assets, a public Radicle mirror, and a simple
installer. This initiative uses the script option, rather than publishing crates
which do not install the application. The Python frontends currently have no
Cargo CLI package, and downstream Cargo does not inherit the workspace's pinned
SQLCipher patch. Cargo publication remains future work; the Actions registry
token must not be consumed by an unrelated or incomplete crate.

1. Approve this design, establish the public Radicle repository using the existing
   identity, and verify Iris replication. Do not replace an unavailable signer.
2. Implement complete native runtime bundles and checked user-local installation,
   validate them offline and through installed commands, and obtain Claude code
   review. Commit and push the reviewed increment to GitHub and Radicle.
3. Add reviewed release automation, build three native archives from one commit,
   publish an explicitly experimental prerelease and verify installation from
   Iris and published assets. Record each platform's actual checks and limitations.

The requested initial targets are Linux x86_64, Linux aarch64, and macOS aarch64,
matching available native runners. Intel macOS is not included or claimed.
Linux ARM64 confinement is currently blocked by runner namespace policy; macOS
confinement has an unresolved broker SIGABRT. Packaging/build success cannot
upgrade these platforms to runtime-validated support. Linux x86_64 installed
end-to-end command execution is a required gate. Existing native validation
gates remain intact; build-only packaging must not disable runtime confinement.

## Runtime package interface

Each versioned tar.gz archive contains one version/target directory with bin/
launchers, tools/ Python runtime modules, libexec/marmish/ native executables
(wnd, its manifest, worker, bridge, keychain and Darwin helpers), patches/mdk/
provenance inputs, the relay canonicalization fixture needed by verification,
project LICENSE, dependency notices, and release.json. Never include fixture
executables, secret identities, command state, caches or CI credentials.

Use a deterministic explicit file allowlist. release.json records format version,
application version, target, source commit, file digests, runtime prerequisites
and experimental validation limitations. Verify daemon provenance using the
existing verifier. Build with locked Rust dependencies, the pinned clean MDK
builder and its exact bootstrap tests. Record native shared-library dependencies.
Collect upstream license texts/notices from the resolved build dependencies;
do not infer redistribution compliance from a crate's name alone.

The installed launchers locate assets relative to themselves and invoke Python
3.11+ using argv arrays, including prefixes containing spaces. Supply bundled
wnd/hash/manifest and server bridge/worker defaults without changing explicit
user options. Keep relay, identity, authorization and cwd choices explicit.
Native keychain helper discovery uses the installed executable. Do not add root,
privilege, public-relay defaults or insecure fallback to make a demo succeed.

## Installer contract and trust boundary

scripts/install.sh is a small POSIX entrypoint using Python 3.11+ for bounded HTTPS
downloads, SHA256 verification, strict archive validation and staged installation.
Its companion installer logic must be self-contained or fetched from the same
immutable source revision; no unverified mutable secondary script execution.
Default entry URL is https://iris.radicle.network/raw/<RID>/head/scripts/install.sh;
documentation also gives a commit-pinned URL. Iris accepts head or a full commit
OID, not an arbitrary branch/tag in that segment. Release binaries stay in GitHub
Releases because Iris raw blobs have a 10 MiB limit.

Resolve the latest experimental release through the GitHub API, or accept an
explicit version. Download that release's target archive and SHA256SUMS via HTTPS.
Checksums detect corruption; they are not independent signatures or reproducible
build proof. No arbitrary download URL configuration is required for production.
Bound response and extraction sizes, reject traversal, absolute names, symlinks,
hardlinks, duplicate paths, special files, mismatched target/version and unexpected
members; verify every payload against release.json. Preserve exact executable
and private-directory modes. Downloads or checks must finish before publication.

Default to a private user-owned prefix under ~/.local/share/marmish, with launchers
in ~/.local/bin, and support an explicit absolute --prefix. Refuse effective root,
unsafe existing destinations and replacing unrelated files. Use immutable
version/target directories and an atomic current selector; refuse version
collisions with different bytes. Reinstallation is idempotent. Never modify
identity/catalog/keychain state or shell profiles. Clearly print PATH guidance.
Failures must preserve the previous installation and clean only owned staging.

## Release and mirror automation

Use trusted manual workflow dispatch from master with a strict version input.
Build on the native epiphytic-ci Linux X64 bel-ci, Linux ARM64 laptop-ci and
macOS ARM64 laptop-ci runners; laptop builds serialize. Jobs use clean checkout,
isolated source/staging locations, verified trusted build caches, bounded execution and least permissions.
Do not pass publication credentials to compilers, tests or build subprocesses.
Publish only after all three archives and checks pass. Publication uses an
explicit experimental prerelease tag and checksums; do not overwrite an existing
release/version. Include clear per-platform build/runtime status in release notes.

Radicle initialization uses master (the actual repository default), public
visibility, existing signing identity and existing node. Record RID and add rad
remote. Explicitly synchronize to Iris, verify repository metadata and exact raw
committed bytes before claiming availability. GitHub and Radicle are both source
homes; README lists both. AGENTS requires pushing changes and release tags to
both and verifying remote refs/seed visibility; if one fails, record and report
the partial publication rather than declaring synchronization. No secret node
keys, passphrases or tokens enter git, logs, archives or documentation.

## Acceptance checks

Offline tests cover package closure and provenance, launcher argv/override
handling, spaces and special characters in install paths, version/target parsing,
checksum corruption, archive traversal/links/duplicates/size limits, unsafe
prefixes, idempotence and rollback. Installer help and both installed command
helps run from a fresh prefix without the source checkout on PYTHONPATH.

Native jobs build guarded MDK, production Rust executables, licensing records and
the exact complete archives; inspect dynamic dependencies and run installed
offline smoke checks. On Linux x86_64, execute disposable client/server commands
through Marmot with the installed distribution, explicit relays and byte/special
character assertions. Preserve owned-state cleanup and sanitized evidence.
Fetch the installer from Iris and verify bytes against git; fetch published
assets and verify SHA256 and a fresh installation. Store independent Claude
plan/code reviews and validation evidence under docs/reviews and docs/validation.
Use Beads for execution tracking and review dispositions.

## Reviewed contract refinements

These refinements supersede any less-specific wording above.

- The version-root tree is exactly bin/{marmish,marmishd}, tools/ runtime Python
  modules, libexec/marmish/{wnd,wnd.manifest.json,marmish-worker,
  marmish-engine-bridge,marmish-keychain}, patches/mdk/{marmish-bootstrap.patch,
  marmish-restricted.patch,source-guard.json},
  tests/fixtures/relay-canonicalization.json, release.json, LICENSE and licenses/.
  Darwin adds libexec/marmish/{marmish-lifetime,marmish-probe}. The package builder
  runs verify_manifest against the staged tree, not just the original build.
- Add an installed-layout resolver in marmish_app.py: parser defaults replace
  required=True only for a recognized installed tree. Resolve the version root
  once to its immutable physical path. Native helper defaults are absolute; no
  PATH fallback is used for the default installed helper. Explicit options retain
  argparse semantics, including --opt=value and abbreviations. Test the remote
  command divider and a decoy helper earlier in PATH.
- Bundle bin launchers invoke python3 -I -B with an explicit controlled tools
  import directory and argv arrays. Do not import from CWD/PYTHONPATH, and never
  write bytecode into the installation. Prefix bin launchers have a marker and
  safely quoted immutable absolute version-bin paths. Installation stages both
  launchers, validates existing marker ownership and can roll back replacements.
  Existing processes retain immutable old paths; keep old versions, do not delete
  running executables. No automatic state migration or downgrade is performed;
  profile/schema incompatibility fails according to existing frontends.
- A read-only --doctor reports interpreter and platform prerequisites. Linux
  checks include /usr/bin/python3 (also >=3.11), bwrap, unshare, /usr/sbin/ip,
  /usr/sbin/sysctl, /etc/ssl/certs and /etc/nsswitch.conf. Darwin requires macOS15+,
  sandbox-exec, codesign and ifconfig. Static prerequisite findings do not prove
  confinement works. Missing optional keychain service is not probed by help or
  doctor; the runtime retains its existing auto-provider rules.
- scripts/install.sh embeds all installer Python; no second executable fetch.
  List releases and choose the first non-draft matching experimental version,
  rather than /releases/latest (which omits prereleases). Explicit --version is
  supported. Download via HTTPS only with bounded time/size and redirect hosts
  api.github.com, github.com, release-assets.githubusercontent.com and
  objects.githubusercontent.com. Ignore ambient proxy configuration; provide
  fixed actionable network/rate-limit errors without leaking response secrets.
  Mutable Iris head means trusting the current served script. Also document the
  immutable source-commit URL. Same-release checksums are integrity checks only.
- Use a bounded gzip decoder and strict USTAR reader/manual extraction. Enforce
  member/count/decompressed-size/path-length limits and an ASCII canonical file
  allowlist (so case/Unicode collisions cannot be smuggled in). Reject PAX/GNU
  extensions, sparse entries, links, unsafe mode bits, duplicate members and
  nonzero trailing tar/gzip data. Create regular files exclusively in a newly
  owned staging tree, reject all existing symlink prefix components and never
  extract through externally supplied directories. Validate release.json fields,
  exact payload set, all digests and expected modes. No tarfile.extractall.
- Provide --uninstall for marker-owned launchers/selector only, retaining old
  version trees and all user state. No arbitrary recursive deletion is required.
- Record minimum glibc required by the actual ELF symbols in each Linux build,
  and check it on installation. Do not advertise a distro floor below the measured
  requirement. Record external shared libraries and their availability; SQLCipher
  and OpenSSL remain vendored. macOS builds are ad-hoc signed, not notarized;
  upgrades may prompt for Keychain access again or fail closed if access is
  denied. Do not bypass Gatekeeper, disable quarantine or change keychain policy.
  Verify code signatures after extraction on the native build runner.
- License collection covers both exact dependency graphs/lockfiles: Marmish and
  patched pinned MDK. Collect full root and vendored LICENSE/COPYING/NOTICE/
  COPYRIGHT texts from resolved source trees, recording package/source/digests.
  Explicitly include SQLCipher, nested OpenSSL and secp256k1 notices, plus MDK's
  original MIT license (2024–2026 Internet Privacy Foundation). Retain MPL source
  availability and original pinned source/revision references where applicable.
  Missing license texts/unknown metadata fail packaging until reviewed; do not
  silently ignore copyleft obligations. The optional attribution question received
  no preference during implementation; use Marmish contributors as communicated,
  preserving the repository's existing MIT declaration.
- Before public Radicle initialization, scan all reachable Git history for
  credential patterns and audit tracked private-state paths and publication
  inputs. GitHub is already public; existing author email, public repository
  history and tracked sanitized Beads/review evidence are within the user's
  explicit mirror-publication request. Do not demand a second authorization to
  mirror that existing public history, and do not rewrite history. A discovered
  actual credential is a concrete blocker requiring remediation.
- Update AGENTS.md, README and docs/agent-spec.md as deliverables. Existing
  general statements withholding release authority are superseded for this
  initiative by the user's explicit release and public Radicle request recorded
  in Beads marmish-ic3 (2026-10-29). Future unrelated releases retain their normal
  authorization requirements. Build jobs receive no publication credentials. Shared native toolchain/Cargo caches are restored only in trusted master jobs serialized with native validation; source and staging remain isolated and locked build identities are checked.
- Publish requires build/provenance/offline installation success on all three
  targets and installed Linux x86_64 real-Marmot execution. Do not require the
  known-blocked ARM/macOS runtime gates to pass as packaging checks or suggest
  they passed. release.json, installer output and release notes name experimental
  status and the exact target runtime limitation. No Intel macOS artifact is
  implied by macOS ARM64 support.

## Hosted-runner fallback after assignment failure

The initial dispatch waited over five minutes with all self-hosted native jobs
unassigned; the user was notified. To complete this explicitly authorized public
binary release, distribution builds may use standard GitHub-hosted native VMs:
`ubuntu-24.04` (x86_64), `ubuntu-24.04-arm` (aarch64), and `macos-15` (aarch64).
GitHub documents these as native standard public-repository runners. Existing
self-hosted native validation remains unchanged, including its known failures.
Use a separate distribution concurrency group because these VMs do not share
self-hosted hardware; run the three independent builds concurrently. Keep all
build, provenance, license, installer and installed Linux AMD64 Marmot acceptance
gates unchanged. No provider/keychain tests use a runner's ambient keychain.

Edit the existing release.yml: use plain hosted runs-on labels, max-parallel 3,
and concurrency group marmish-experimental-distribution. Publish still needs all
three build jobs from that same dispatch and exact source. Before any release
mutation, and again before publishing the draft, require GitHub master to equal
the dispatch source commit; this prevents the older queued workflow revision
from being published by the new publisher. Cancel and verify cancellation of the
superseded pending distribution dispatch before dispatching the new revision;
leave self-hosted native validation queued for its intended runners. No automatic
reuse of an old dispatch's artifacts or publication is allowed.

Install only missing system build/runtime prerequisites on disposable Ubuntu VMs
with their documented sudo facilities (Bubblewrap, iproute2, util-linux,
CA certificates and binutils), and log installed package versions. Hosted Linux
acceptance uses the product-default /usr/bin/bwrap from the authenticated Ubuntu
package manager, without private Bubblewrap overrides. Remove private acquisition
and cleanup steps from this hosted-only workflow. If namespace/confinement probes
fail, the live job fails and no release is published; never change sysctl,
AppArmor or namespace policy. No production host configuration changes are part
of this fallback. Build jobs still receive no publication credentials.

Use a hosted-distribution cache prefix with no restore fallback to self-hosted
native/distribution caches. Restore only on trusted master. Record free disk
space before builds and retain 90-minute job bounds. Record actual measured glibc
requirements. Release notes identify native GitHub-hosted builds and the run URL.
Reword release limitations to say ARM64 confined execution is not validated in
this distribution; namespace/SIGABRT observations refer explicitly to earlier
self-hosted evidence, not unperformed hosted runtime tests. Update user-facing
installation text consistently. Commit and push the reviewed increment to both
source homes before dispatching it.

### macOS temporary-path correction

The first hosted macOS offline run exposed `/var` versus `/private/var` aliases.
Canonicalize trusted checkout roots before comparing resolved Cargo manifest
paths in license collection; preserve rejection of genuinely external local
dependencies and escaping license symlinks. Use canonical temporary-directory
paths in package/install test fixtures so expected absolute helpers match runtime
resolution and the installer can retain its symlink-component rejection. Do not
relax installer path validation. Add an aliased-checkout regression test runnable
on Linux and macOS, retain external-escape negatives, and rerun native jobs.


### Failed installed-acceptance diagnostics

The first hosted Linux x86_64 archive built and installed successfully, then its
live child returned failure after approximately 12 seconds. Acceptance currently
replaces the child report with `installed_live_command_failed`, losing the stage
and fixed error category needed to distinguish host confinement failure from a
transport or installed-default regression. A separate local direct installed
rerun passed all 15 file-storage assertions in 212.392 seconds; this does not
explain or override the hosted failure. Keep publication blocked on the native
hosted live gate.

Limit this increment to `tools/marmish_release_acceptance.py`, focused tests in
`tools/test_marmish_release.py`, and sanitized review/validation documentation.
Do not alter the product, confinement policy, installer, workflow gates, relays,
timeouts, or live harness behavior. Root obtains independent Claude approval of
this design before implementation and reviews the completed change before push.

On the existing nonzero-child or non-passed-outcome branch, keep outer
`outcome=failed` and `error=installed_live_command_failed`, preserving the existing
source commit, archive SHA256 and target bindings supplied by acceptance. Add a
nested `diagnostic` object containing only `format=1`, `stage`, `error`,
`assertions`, and `local_cleanup`. Cleanup accepts only `complete` or `incomplete`;
missing or invalid cleanup values become `unknown`. Never copy the child's top-level fields or bindings. A child with
`outcome=passed` and a nonzero exit remains a failed acceptance. Success still
requires both zero exit and child outcome passed; diagnostic content cannot
change that predicate. Malformed/nonobject JSON and process failures retain
their existing fixed failure categories.

Validate diagnostic fields independently, accepting only exact strings in
source-controlled frozen vocabularies. The stage vocabulary is the literal
checkpoint names in the current E2E source plus `setup` and `complete`; unknown,
missing or nonstring stages become `unknown`. The assertion vocabulary contains every literal passed to `passed()` in the
current E2E source, including the 15 file-storage installed-E2E assertions and
native-only assertions. Accept only a list of at
most 64 entries, filter to exact known strings, deduplicate while retaining
order, and otherwise emit an empty list. Do not infer that a listed assertion
passed independently of the child report or promote these diagnostics to release
acceptance evidence.

The error vocabulary is the frozen union of all reachable literal categories,
not just E2E and sandbox errors. Inspect `require()` call literals and default
category arguments plus `SmokeError`, `RpcError` and `SandboxError` constructor
literals in the E2E, `mdk_smoke`, RPC, app, protocol, key storage, native-validation,
transport/sandbox, installed-layout, bootstrap, MDK-manifest and platform helpers
reached by the harness. Include the explicit fixed report categories
`bwrap_provenance_unavailable`, `transport_failure` and
`local_io_or_schema_failure`. The implementation records the exact module list
and static string union for review; conservatively include the locally imported
helper closure when a category's reachability is uncertain. Store these finite
sets as hard-coded Python constants, with no runtime source scanning, network
lookup, arbitrary upstream text or regex-only acceptance. Missing, nonstring or
unrecognized errors become `unrecognized_error`.

Add offline AST drift tests over that explicit local module list. Collect literal
categories passed to `require`, `SmokeError`, `RpcError` and `SandboxError`,
including named/default category arguments where used, and require them to be
members of the frozen error set. Separately collect every E2E `checkpoint()` and
`passed()` literal and require membership in the frozen stage/assertion sets.
The tests run from source; installed acceptance never parses Python source to
expand its vocabulary. A new literal requires an intentional reviewed constant
update. Dynamic RPC error text remains unrecognized even if it contains only
alphanumeric characters. Test an arbitrary 60-character alphanumeric token and
verify that it is absent from the entire written evidence.

Keep unknown fields, exception text, stdout/stderr, paths, identities, relay
responses, command contents and credentials out of failure evidence. Retain no
raw output even when JSON contains extra fields.

Focused offline tests cover a failed well-formed child with recognized stage,
error and assertions; missing/unknown/malicious diagnostic values; malformed list
shape and oversize list; deduplication; cleanup finite-value validation; AST
vocabulary drift; the 60-character secret sentinel; forged source/archive/target/outcome
fields; a nonzero exit with child outcome passed; and unchanged timeout,
non-JSON and successful behavior. Verify failure still raises after writing
sanitized evidence and that the existing aggregate publisher rejects it. After
review, commit and push both source homes and dispatch one fresh same-commit
three-target workflow. Only its all-target installation gates and Linux x86_64
installed live pass can authorize publishing those artifacts. If it fails,
inspect the new sanitized category before proposing a production fix.

### Python-version-stable AST drift fingerprints

The second hosted macOS run failed the offline diagnostic vocabulary drift test,
so this run does not qualify for any local-execution publication fallback. The
same failure is reproducible locally with the already-installed CPython 3.14.7;
the default CPython 3.12.3 test passes. CPython 3.13+ `ast.dump` omits empty list
fields by default, changing the reviewed `RpcError(str(error))` fingerprint by
removing `keywords=[]`. Explicit `show_empty=True` on CPython 3.14 reproduces the
existing reviewed fingerprint exactly. This is a test representation issue, not
a reason to relax the error vocabulary or dynamic-call drift gate.

After independent Claude design approval, change only
`tools/test_marmish_release.py`: introduce one focused stable AST dump helper
that always passes `include_attributes=False`, adds `show_empty=True` only when
`sys.version_info >= (3, 13)`, and otherwise uses the existing pre-3.13 argument
set. Route the single diagnostic dynamic-category fingerprint dump through it.
Preserve the exact reviewed structural strings and counts, literal vocabularies,
module closure and all production behavior. Do not catch arbitrary TypeError,
regenerate fingerprints for each interpreter, or ignore mismatches. Add a golden
regression for the nested `str(error)` call including its empty `keywords=[]`,
and retain the full real-source drift test as the integration check.

Run the focused distribution suite using already-installed CPython 3.11, 3.12
and 3.14, with no downloads, live tests or builds. Obtain independent Claude code
review, commit/push the correction and reviewed plans to both source homes, and
dispatch a fresh three-target same-source native distribution. Record the exact
full source commit and run ID/attempt in Beads and validation evidence. No failed
macOS offline gate is replaced by local Linux evidence. Only after every native
offline/build/installation gate passes can the following conditional fallback be
considered for a separately confirmed hosted Linux namespace-policy failure.

### Conditional local execution gate for a hosted namespace-policy failure

This is a narrow procedural exception to the preceding hosted-live requirement,
subject to independent Claude design approval before use. It applies only to the
native distribution from a future fully recorded frozen source/run. Before
using it, record the full 40-character source commit, originating workflow run ID
and attempt in Beads and the reviewed validation record; use that exact identity
throughout every step below. The current run with a failed macOS offline test
does not qualify. The exception is inactive unless the hosted
Linux x86_64 diagnostic identifies a namespace/confinement startup failure and
bounded read-only host evidence confirms that the hosted namespace policy caused
it. The namespace-start category alone does not establish that cause. Record the
exact sanitized startup failure category and, where runner access/logs permit,
the fixed sysctl integer values and the owned namespace probe described below.
A failed probe explicitly reporting EPERM/EACCES, together with the same hosted
live namespace-start failure, establishes that the host denies the namespace
operation required by the current product. It does not identify AppArmor as the
specific cause. Record unavailable settings/log access honestly; do not infer an
exact policy culprit from timing, a restrictive setting alone or a generic
namespace failure. If permission denial is not established, the cause remains
ambiguous and this exception stays inactive. An unclassified failure, transport failure, product defect, missing native
installation proof or incomplete cleanup does not qualify. Do not weaken or
change host confinement, AppArmor, namespace settings or workflow `needs` gates.

Before a new dispatch, independently review a workflow-only observation step
immediately after Ubuntu prerequisite installation. It prints one finite JSON
record with integer-or-null values read from only these fixed paths:
`/proc/sys/kernel/apparmor_restrict_unprivileged_userns`,
`/proc/sys/kernel/unprivileged_userns_clone`, and
`/proc/sys/user/max_user_namespaces`. Read bounded ASCII decimal input, reject
noninteger/oversized values to null, and emit no arbitrary file contents. Run
only `/usr/bin/unshare --user --map-root-user --net /usr/bin/true` with a
10-second subprocess timeout and a fixed C locale. Capture its output privately;
print only return code or null, success, timeout, and a known-permission-denial
boolean recognizing the C-locale EPERM/EACCES messages. Never print raw stdout,
stderr or exception text. Treat an unavailable executable/read/probe as an
explicit unavailable/null observation; the observation step exits successfully
and cannot block compilation or satisfy the existing live acceptance gate.
The short-lived owned probe exits with its namespace; no settings, capabilities,
AppArmor profiles or namespace policy are changed. Require explicit independent
design approval before editing `.github/workflows/release.yml` to add this step.
Its only purpose is honest bounded host evidence for the conditional trigger,
not an execution fallback or alternative success test. The Linux-only step sets
`continue-on-error: true` and a one-minute step timeout; unexpected observation
failures therefore do not fail the build. The subprocess receives only a fixed
minimal PATH and C locale, uses stdin/stdout DEVNULL and a private temporary
stderr file read to at most 4096 bytes for known denial classification. Each
fixed sysctl read is at most 65 bytes and accepts only at most 20 decimal digits
plus an optional newline. Emit the finite JSON to stdout and
`GITHUB_STEP_SUMMARY` only, with no artifact or private stderr upload. Summary
write failure is ignored. The existing real installed live gate is unchanged.

Wait for the originating workflow run/attempt and every build job to terminate.
Verify from the exact job logs and artifacts that all three native builds,
locked provenance/license collection, offline distribution checks, staged
binary smoke checks and native installer/help/idempotence checks passed. The
hosted x86_64 combined acceptance step may have failed only in its subsequent
live confinement startup; verify that its preceding native installation checks
completed. Both ARM64 native installation jobs must have succeeded. The full
local x86_64 acceptance below must additionally pass installation, live command
execution and marker-owned uninstall. Preserve the hosted failed report as
separate failure evidence; never relabel a failed job or workflow as successful.

Download precisely the three target artifacts from that single terminated
run/attempt. Record run ID, attempt, source SHA, artifact names/IDs and available
GitHub artifact digest metadata; verify downloaded artifact bytes against that
metadata using the documented artifact digest semantics. Require the run head
SHA and every embedded/sidecar manifest to equal the frozen source. Construct a
new private inputs directory containing exactly one original archive and sidecar
per target. Do not rebuild, repackage, substitute an earlier local archive or mix
artifacts from another run/attempt. Preserve an immutable downloaded copy and
compute each original archive SHA256. A missing artifact or unverifiable source
is a blocker, not permission to fill the gap.

Use a clean checkout or isolated clean worktree at the exact frozen source SHA
to run both acceptance and publication, including the checkout installer imported
by acceptance. Require no executable/source edits; the existing build cleanliness
contract may exclude only known Beads metadata, never other changes. On a local
Linux x86_64 host whose ordinary product confinement already works, first verify
that the archive-recorded glibc and shared-library prerequisites are fulfilled
using its installed doctor checks. Preserve that bounded prerequisite result.
Run the unchanged `marmish_release_acceptance.py` against the downloaded x86_64
archive using a newly owned private prefix and the explicit EU/US WhiteNoise
relay URLs. Run no overlapping live test and use disposable file-backed keys.
Require exit zero, the actual installed CLI assertion, all 15 file-storage E2E
assertions, `local_cleanup=complete`, and the normal install/help/idempotence/
uninstall checks. Verify the generated live report's source SHA, exact archive
SHA256 and `wnd_sha256` against the downloaded manifest/payload. A prior direct
E2E pass is diagnostic only and cannot replace this full archive acceptance.
A local failure leaves publication blocked.

Preserve that original acceptance report. In the clean aggregate inputs copy,
add only a separate `operator_provenance` object to the passed report, identifying
`execution_origin=local_linux_amd64_host`, the originating workflow run ID/attempt and
its source SHA, and the hosted namespace failure category. Do not change any
acceptance-generated field, assertion, digest or outcome; verify an exact diff
showing only this added object. This metadata is an explicit trusted-operator
attribution, not a cryptographic host attestation or a new security predicate.
The original unchanged publisher must still independently enforce source/archive/
wnd digest binding, successful installed command execution and complete cleanup.
Keep exactly one `installed-e2e.json` in aggregate inputs. The preserved failed
hosted report remains outside that inputs tree.

Run the existing strict publisher locally with the frozen source, selected
version, clean inputs, a new output directory and `GITHUB_RUN_ID` set to the
verified originating run. Do not call its lower-level functions to bypass
aggregation. Audit the aggregate archive set, manifests, evidence and SHA256SUMS
before invoking publication; use a fresh output directory for a second CLI
invocation because aggregation refuses an existing output. The unchanged
publication path creates a draft, uploads the exact validated aggregate,
downloads and hashes every asset, checks current master and tag identity, then
makes the experimental prerelease visible. The skipped CI publish job stays
skipped; local publication is a separately reviewed operator action under the
user's release authorization.

Existing generated notes truthfully say the binaries are native GitHub-hosted
builds and Linux commands executed through Marmot, without claiming that live
execution occurred on GitHub. Before visibility, the uploaded validation JSON
already carries explicit local execution attribution. Immediately after publish,
edit only the release body to add the same local-host qualification and exact
originating hosted namespace failure category; this is mandatory and must be
recorded in Beads. Preserve source, run URL, limitations, assets, prerelease
status and tag. No deferred body edit is needed to make a
false claim true. Keep the ARM64/macOS runtime limitations explicit.

Verify GitHub master and the Radicle source branch still equal the frozen source
before publication. Preserve the publisher's two fresh GitHub-master checks and
all tag checks. Commit the approved design and any test correction before dispatching the
qualifying future workflow. Once that workflow source is frozen, do not commit
subsequent validation/review records before publishing its already-built
artifacts: that would advance master and must correctly fail source freshness.
Record all operator actions in Beads while source stays frozen, and ensure
executable source is unchanged. Do not commit tracked Beads exports, including
`.beads/interactions.jsonl`, before publication: that also advances master and
must fail freshness even when the change contains no executable code.
After publication, verify the GitHub release tag resolves to that exact source,
then publish/verify the same tag at Radicle without forcing any conflicting ref.
Finally commit and push the reviewed documentation/specification/validation
record to both homes, linking original failed hosted evidence and the new local
archive acceptance evidence honestly. If source changes before release visibility,
abandon this exception for these artifacts and perform a fresh reviewed build;
never bypass the freshness guard.
