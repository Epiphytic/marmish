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
- Store `Arc<ProfileFiles>` in `ReadyReceiverScope` and pass a clone to a
  private socket-free `CheckedPeerRoute::bind_with_profile`. Keep the
  existing fixture wrapper and sampled-clock tests. Use the existing
  `read_issued_held`, issuer derivation, config/CA/attestation checks and
  monotonic qualification clock. A private
  `HeldQualification::matches_config` compares its held config descriptor
  identity and byte digest with the Ready scope's witness. Rechecks validate
  both files and the supplied material. On every register, registered
  recheck and attach, rederive the zeroizing issuer key from that exact
  lease and material and verify the held schema-3 record's existing
  issuer MAC with `Mac::verify_slice`. The same account and device with
  a substituted SQLCipher root cannot pass. Retain the authenticated
  record, with no added secret storage or diagnostic.
  Reuse `ProfileFiles::recheck` to compare the owner lock, manifest descriptor
  and parsed manifest against the lease; retain the initial bounded manifest
  and config byte witnesses for inode and bytes. This leaves ordinary config
  binding and relay behavior unchanged.
- Expose only private candidate interfaces through `peer_relay_trust`:
  `ReceiverPreflight::prepare(&Arc<ProfileLease>, &IdentityMaterial,
  &CommandConfig) -> Result<Option<Self>>`,
  `register(self, &Arc<ProfileLease>, &IdentityMaterial) ->
  Result<RegisteredReceiver, RegistrationFailure>`, and
  `RegisteredReceiver::attach<'lease>(self, &'lease Arc<ProfileLease>,
  &IdentityMaterial) -> Result<LocalReceiverRegistration<'lease>>`. The
  attached value holds one `ReceiverQuota<'lease>` and the same opaque
  checked route. Expose `quota(&self) -> &ReceiverQuota<'lease>` and transfer
  both through a single private owner interface when the reviewed port
  bind is implemented. Future inventory, `receiver_files` and
  `Reservation::owner` borrow this quota instead of calling fixture
  `bind_receiver`. The actor owns the Arc, material and borrowed
  registration as separate locals in its top-level future. A native
  `QuotaTurn` retains its non-Send marker and ends before any await.
  These types have private fields, no Clone and no fixture conversion.
  The caller may perform a read-only `RegisteredReceiver::recheck`
  before transfer. Actor attachment rechecks and consumes the witness.
  No route opener, role override, native turn or receive authority is
  provided here.

## Directory and protected names

- Stock fresh profiles have no `peer/`. `prepare` is read-only, and
  excluded profiles return before any directory operation. Only
  `register`, after its full local scope recheck and before attempting
  SQLite registration, uses the already-held private root descriptor to
  open `peer` via `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`. For an exact
  ENOENT, call `mkdirat` on that descriptor with mode 0700 and reopen the
  leaf by the same no-follow path. Also handle an EEXIST race by reopening
  and validating it. This creates at most the empty member directory. Verify
  directory type, effective UID and exact 0700 mode without chmod or
  taking ownership, require the same device and supported local filesystem
  using descriptor stat/statfs. Reuse the existing profile directory
  validation and local-filesystem allowlist. Compare the held member
  device/inode with a fresh no-follow open relative to the same rechecked
  root, retain absolute canonical-path equality, and compare those
  absolute parent identities to both held descriptors. After the final
  reopen comparison, sync the held parent when this turn created the
  member. A file or symlink in place of the directory, unstable path,
  failed device/filesystem policy, replaced inode or failed fsync
  refuses. An unusually restrictive umask may leave a directory with
  insufficient permissions and refuse startup qualification. A
  `mkdirat` success supplies no inode descriptor, so never chmod a
  subsequently reopened name, alter process umask, or clean up a possibly
  substituted name. `Untouched` may leave an empty directory for the
  operator to inspect. No file is created in this leaf.
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
  and stock libSQLCipher VFS checks for the candidate. Widen only
  fixed Linux AMD64 receiver slot count, snapshot slot/name, exact weak
  lease ownership and necessary shared registration code. Keep
  fixture-only ProfileSlots/scalar charge, arbitrary alternate snapshot,
  Founder, interruption, recovery and one-poll native fixtures test-only.
  Construct the candidate dispatch with its `OnceLock<ReservedPaths>`
  initialized to the exact complete reservation before publishing the
  wrapper as SQLite's default. Keep fixture late binding separate. Use a
  distinct fixed candidate VFS name. Final pre-mutation checks cover Ready
  scope, qualification, original Arc, directory, topology and expected
  named stock default. No await or database open occurs in this turn.
  SQLite global initialization for the VFS opens no store.
- Serialize the default mutation with a private process-owned state
  `Unregistered | Registered(&'static Dispatch) | Indeterminate` on
  startup. A local refusal before mutation is `Untouched` only after
  proving exact stock default/managed and the stock contract. Check
  version, size, callback topology, name/pointer and compile options
  before mutation, including absence of a foreign candidate name.
  SQLite initialization failure prevents checking the stock contract and
  takes the fatal path because ordinary startup also lacks trusted
  storage. Set the state to `Indeterminate` before
  `sqlite3_vfs_register` and change it to `Registered` only after exact
  default and named-wrapper identity, own open/delete callback pointers,
  size/version and captured stock/managed identity all match. Keep the
  attempted descriptor and complete reservation for process life. This
  covers displacement between SQLite lookup and registration, which are
  separate calls. A failed or uncertain attempt and displaced or unknown
  global state are sticky `Indeterminate`, with no unregister or second
  mutation. A repeat cannot issue another witness, even for the same
  lease. An exact read-only registered check proves wrapper, lease,
  device/inode and complete topology. Refusal after a still-verified
  registered open is local unavailability. A broken default/stock
  invariant cannot authorize ordinary opening through the wrapper.
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
  exact private descriptor evidence, a different lease, same
  account/device with a different SQLCipher root, wrong
  config/CA/manifest, owner, mode, non-directory and symlink member,
  member/root replacement, and same-lease exact rechecks.
- Registration changes the process default irreversibly. Run dedicated
  isolated Linux AMD64 subprocess cases for the real registration and
  attachment core, untouched stock default after a local mismatch,
  protected main and all three sidecar names refusing unscoped opens
  and deletes, ordinary SQLite forwarding before any Peer open, foreign
  or expired weak lease, displaced default before attach, and no second
  mutation even after an uncertain result.
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
