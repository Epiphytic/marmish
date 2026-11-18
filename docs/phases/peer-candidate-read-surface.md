# Private recipient candidate build and local reads

This design refines steps 3–4 and the List/Show contract in
[Installed recipient daemon](peer-installed-recipient-daemon.md) against
`2c18a7d`. It lets compilation, packaging and the read protocol be reviewed
while the charged owner is being composed. The phase exits with the existing
installed-daemon Pending milestone. A linked binary, packaged candidate or
socket framing check alone does not satisfy it.

## Build and artifact boundary

- Declare `marmish_peer_receiver_candidate` for Cargo check-cfg in
  `crates/marmish-native/build.rs`. Only the explicit private Rust cfg selects
  it. Reject it on targets other than `x86_64-unknown-linux-gnu`, together with
  `installed-checks`, or with an absent, zero or malformed
  `MARMISH_BUILD_COMMIT`. Reuse the existing stamp, require an explicitly
  supplied value with no Git/zero fallback in the candidate, and have the
  private harness check clean source and exact HEAD/stamp before building.
  Check `CARGO_CFG_MARMISH_PEER_RECEIVER_CANDIDATE` at build time with the
  explicit Cargo target and use an isolated target directory. Cargo features,
  ordinary runtime flags and environment at program execution cannot select
  this build.
- Ordinary `release-info` and manifest bytes and schema remain unchanged. Add
  one fixed candidate discriminator to `compiled_release_metadata`, serialized
  only by the candidate. Use the exact existing `native_release.rs` bounded
  probe of all three executable inputs (`marmish`, `marmishd`, worker), and
  require matching metadata including candidate class and exact stamp. The
  private packager invocation explicitly selects the candidate, requires a
  candidate-built packager and refuses mixed inputs before output mutation.
  Ordinary packaging refuses candidate inputs, including when accidentally
  invoked through a candidate-built packager.
- Keep the manifest record set, lengths, provenance and hashes. For candidate
  only use a distinct exact first line, `marmish-peer-receiver-candidate 1`,
  recognized only by candidate-compiled native parsing. Enforce the class in
  `Manifest::parse` for all package, installer, installed receipt/adoption and
  layout reads, and compare it in the package probe with the other provenance.
  Only the appropriate header parses in each build. The class supplies no
  trust or route authority. Existing public bootstrap and strict release
  header and JSON metadata checks remain stock-only, including workflow,
  release-candidate, diagnostic, local qualification and update/HTTPS tools.
  The private recipient harness under the parent phase calls the existing
  native `marmish install` directly with a verified manifest digest, isolated
  HOME, prefix, launcher, state and socket paths, and disposable explicit
  credentials. Reuse owned temporary-directory, asset matching and native
  layout checks. Existing stock-only harness verification stays unchanged.

## One listener and a read-only projection

- At `cli.rs::parse_frontend`, add candidate-only client Clap subcommands
  `marmish servers list` and
  `marmish servers show --server NPUB --offer-id HEX --revision HEX --generation N`.
  Use existing local profile/socket selection and request framing. Reuse
  `peer_pending_ipc.rs::Selection` validation for canonical npub, lowercase
  fixed-length hex and nonzero generation. Give these local forms precedence
  on the candidate's unqualified `servers` token. Preserve explicit remote
  separators and the unambiguous `npub@relay` form, and test the current
  legacy parsing and rsync paths. Follow the same local/remote invocation
  restrictions as `peer relay`; reject malformed local forms and disallowed
  options locally. Only candidate `marmish` advertises the subcommands.
- Reuse `ipc.rs::serve_with_peer` on the ordinary Unix listener and its exact
  peer-credential check, short exchange deadline, client capacity and
  four-request Peer queue. Do not create a socket, a second listener or a
  generic API. Share a single credential, accepted timestamp and framing
  path on the real listener in unit tests and candidate runtime. Expose only
  a closed read request type in the candidate `ipc::Request::PeerRead`, with
  the shared selector and bounded read queue. The separately test-only
  fixture request/reply and its `Approve`, `Decline`, `Manual` and
  `Witness` have no non-test mount and are not the candidate wire type,
  even under candidate-cfg unit tests. The candidate wire operation is
  `peer_read`. A raw write variant, extra field or malformed selector is
  rejected before peer work is queued.
- The reply exposes at most 16 current Pending entries. Fields are the four
  selector coordinates, Pending, held time and expiry. Project at the Peer
  reply boundary from current checked worker results. Show requires exact
  selection; stale, expired, denied, unbound, non-Pending and failed checks
  produce `Unavailable`. List omits ineligible rows; an unavailable owner
  produces `Unavailable`, including absent and Off. Use a separate closed
  read entry and reply with no decision outcomes. Keep typed malformed-frame
  and overloaded-exchange failures bounded. Never copy internal provenance,
  qualified/activating time, native coordinates, event bytes, relay,
  filesystem path or debug reason.
- Separate listener registration and the first ordinary active status in
  `daemon.rs::serve_owner`. The existing fixture worker waits only for the
  former. Distinguish Starting, Registered, Ready and Stopped. The candidate
  opens no Peer database or route until Ready. Signal readiness once, after
  registration, the usual active status and successful
  `StartupReporter::ready` when present; use the same event for foreground.
  Pair the two events without waiting on Peer work from ordinary startup.
  Before Ready, reads fail unavailable before queueing. A Peer registration
  error stops only Peer; a transient accept error follows the existing
  ordinary diagnostic and 50 ms retry, even when a pair exists. A genuine
  listener task exit retains the ordinary fatal behavior.
- Arm the existing listener drain in the candidate when a pair is supplied,
  independent of the fixture stop trigger. Track the shared listener task
  while running. On shutdown close Peer intake and initiate route cancellation
  and listener drain, then join the listener and worker and release the
  profile lease only after owned native handles close. Use the existing
  ordinary shutdown path when no pair was created. A failed Peer bootstrap
  does not fail ordinary startup; the parent's post-VFS registration
  invariant failures retain their specified fail-closed semantics.

## Composed exit and verification

Build, packaging, read framing and lifecycle changes can be reviewed before
Delivery or the charged owner lands. Keep the route/startup mount closed
during that work. No fixture bootstrap, clock, CA or test authority moves into
installed code. The same final candidate mounts only after exact preflight
ordering, post-open stock binding verification, recovered quota, authenticated
Delivery and charged native Pending readback from the parent phase. Bind
readiness to that owner without promoting the existing fixture worker's
bootstrap or decisions.

Check default, installed-checks and private candidate contracts. Cover forbidden
cfg/feature/target and stamp combinations, mixed input and packager classes,
both manifest directions, digest tamper rejection before mutation, and ordinary
release checks rejecting the candidate. Use the existing bounded, disposable
install primitives in the private recipient harness. Include a non-test
candidate compile-time absence check for the write types and request variants.
Run the read-boundary tests under the candidate cfg and ordinary fixture tests
under the default test build. Verify help/parse isolation in both binaries,
exact List/Show projection, same-UID and foreign-UID refusal, a transient
accept fault followed by successful ordinary Status and Command dispatch,
registration failure, pre-ready refusal and stop without a Peer work queue
or task escaping. Include a black-box check of malformed/raw writes and
unavailable reads on an installed candidate's actual socket. Until the mount
lands, positive Pending projection evidence is adapter-level only. Reuse the
existing installed private WSS test matrix for actual Pending/List/Show,
ordinary Command/Terminal coexistence and restart once the charged owner is
present. Obtain independent Claude Auto code review of that exact composed
source before calling the phase complete.
