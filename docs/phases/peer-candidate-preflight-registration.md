# Private candidate preflight registration

This implements the local part of startup steps 1 and 2 in the
[recipient daemon mount](peer-candidate-daemon-mount.md). The starting tree is
`08ca5d7`. The startup and actor call sites belong to the composed daemon
phase. This leaf provides their local authority, with no Peer cursor or
stage opened, no socket, and no candidate behavior mounted on the daemon.

## Checked startup scope

- Preserve the one prepared `Arc<ProfileLease>` from startup to actor.
  Extend the Linux AMD64 Ready-at-acquire field, capture and check in
  `identity.rs` and the Ready scope in `peer_ready_scope.rs` to
  `any(test, marmish_peer_receiver_candidate)`. Keep test children
  explicitly gated. Capture requires a previously Ready identity with
  the prepared account and device, Client or Both Manual and the existing
  normalized Peer relay binding. An absent or excluded config, unsupported
  platform, bad ownership, scope or issuer evidence is unavailability.
- Add a private, socket-free checked-route binding using the same held
  `Arc<ProfileFiles>` as the Ready scope. Use the existing
  `read_issued_held`, issuer derivation, config/CA/attestation checks and
  monotonic qualification clock in `CheckedPeerRoute`. At construction
  compare the qualification's config descriptor identity and byte digest to
  the Ready scope's checked config witness. At subsequent checks revalidate
  both witnesses and the account/device of the supplied prepared material.
  Reuse `ProfileFiles::recheck` to compare the owner lock, manifest descriptor
  and parsed manifest against the lease; retain the initial bounded manifest
  and config byte witnesses for inode and bytes. This leaves ordinary config
  binding and relay behavior unchanged.
- Expose only private candidate interfaces through `peer_relay_trust`:
  `ReceiverPreflight::prepare(&Arc<ProfileLease>, &IdentityMaterial,
  &CommandConfig) -> Result<Option<Self>>`,
  `register(self, &Arc<ProfileLease>, &IdentityMaterial) ->
  Result<RegisteredReceiver, RegistrationFailure>`, and
  `RegisteredReceiver::attach(self, &Arc<ProfileLease>, &IdentityMaterial)
  -> Result<LocalReceiverRegistration<'_>>` (the return lifetime belongs to
  the passed lease). The returned value retains `ReceiverQuota` borrowing
  that lease and the same opaque checked route for a later independently
  authorized port bind. It supplies no route opener, role override, native
  turn or receive authority. The registration and attachment types have
  private fields, no Clone and no fixture conversion. The future startup
  caller may perform the read-only `RegisteredReceiver::recheck` before
  transferring the original value to the actor. Later actor attachment
  rechecks again and consumes it.

## Directory and protected names

- Stock fresh profiles have no `peer/`. Only after a locally qualified
  Ready scope, use its already-held private root directory descriptor to
  open `peer` via `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`. For an exact
  ENOENT, call `mkdirat` on that descriptor with mode 0700 and reopen the
  leaf by the same no-follow path. Also handle an EEXIST race by reopening
  and validating it. This creates at most the empty member directory. Verify
  directory type, effective UID and exact 0700 mode without chmod or
  taking ownership, require the same device and supported local filesystem
  using descriptor stat/statfs, and sync the held parent when this turn
  created the member. Reuse the existing profile directory validation and
  local-filesystem allowlist. Compare the held member device/inode with a
  fresh no-follow open relative to the same rechecked root; compare the
  dispatch's absolute, canonical parent identities to both held
  descriptors. File or symlink in place of the directory, unstable path,
  mount crossing, replaced inode or failed fsync refuses. Never repair,
  remove or reuse a substituted directory. An empty validated directory
  may remain after refusal. No file is created in this leaf.
- The dedicated member topology is the existing
  `provisional-known.db`, `provisional-unknown.db` and
  `receiver-join-snapshot.db`, with each of `-journal`, `-wal`, and
  `-shm`, reserved under those exact directories. The stock-capped
  `peer/cursor.db` is intentionally not a managed-stage path. Cursor
  creation/open, all members present/absent checks and charged recovery
  belong to the actor after the existing listener and ordinary readiness.
  The actor can initialize only its cursor in the already-checked directory
  rather than treating it as a pre-existing store. Registration never
  deletes a prior image or sidecar. Retain existing protected-path and
  same-inode alias checks for default opens and deletes.

## Irreversible turn

- In `peer_quota_dispatch.rs`, use the same forwarding descriptor factory
  and stock libSQLCipher VFS checks for the candidate. Widen only the
  fixed receiver topology, exact lease ownership and necessary Linux
  shared registration code. Keep fixture-only ProfileSlots/scalar
  charge, arbitrary alternate snapshot, Founder, interruption, recovery
  and one-poll native fixtures test-only. Candidate registration installs
  all names in the immutable dispatch before publishing the wrapper as
  SQLite's default; an empty or late reservation is impossible through
  candidate APIs. Final pre-mutation rechecks cover Ready scope,
  qualification, original Arc, directory and topology. No await or
  database open in this turn. Safe initial SQLite global initialization
  for the VFS is distinct from opening a database.
- Serialize the default mutation with a private process-owned singleton
  on the startup thread. Before mutation, prove the expected captured
  stock/managed VFS and forwarding descriptor. Divide
  `RegistrationFailure` into `Untouched` for refusals while the stock
  default is independently still verified, and `Indeterminate` from
  attempted `sqlite3_vfs_register`, displaced default/stock, or uncertain
  global state onward. Keep the candidate dispatcher/reservation for
  process life, including uncertain registration; never unregister or
  attempt a second default mutation to recover. An exact check of an
  existing registration must prove pointer and same opaque lease,
  device/inode and complete topology. It issues no second attachment.
  Refusal to use Peer after a still-verified registered open is local
  unavailability. A broken default/stock invariant cannot authorize
  ordinary opening through the wrapper.
- Actor attachment verifies the exact registered wrapper still defaults,
  original stock and managed descriptors remain current, the exact
  reservation is installed, and both directory descriptors and current
  Ready scope still match the same lease, material, topology and route.
  Conversion into the borrowed receiver quota does not register or
  rebind. The composed daemon uses its already planned fatal startup
  behavior for Indeterminate and preserves the original ordinary
  `check_opened_binding` error. This leaf makes no changes to daemon,
  handshake, candidate CLI, checkpoint, inventory, native operations
  or transport.

## Checks and handoff

- Use deterministic scope tests for excluded policy and first-run Ready,
  exact private descriptor evidence, different lease/material, wrong
  config/CA/manifest, owner, mode, non-directory and symlink member,
  member/root replacement, and same-lease exact rechecks.
- Registration changes the process default irreversibly. Run dedicated
  isolated Linux AMD64 subprocess cases for the real registration and
  attachment core, untouched stock default after a local mismatch,
  protected main and all three sidecar names refusing unscoped opens
  and deletes, ordinary SQLite forwarding before any Peer open, foreign
  lease after registration, displaced default and no second mutation.
  Use the existing private fixture strategy for process isolation. For
  scoped registration unit probes, a test-only setup can provide the
  already checked local witness within its defining module. No test-only
  trust input, fixture port or witness constructor is exported in the
  candidate binary. The composed phase owns privately installed,
  end-to-end, real qualified Ready startup and the positive network proof.
- Run the focused tests, rustfmt, strict scoped Clippy and the normal
  and explicit candidate non-test compile gates. Keep the default and
  `installed-checks` paths independent of the candidate mount. Claim no
  non-Linux behavior until its separately reviewed qualification.
