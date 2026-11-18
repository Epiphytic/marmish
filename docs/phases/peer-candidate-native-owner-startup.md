# Guard the native owner during candidate startup

This is the installed account-owner construction slice of the
[daemon mount](peer-candidate-daemon-mount.md) and
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md) designs.
It builds on the [account custody qualification](peer-candidate-mdk-owner.md)
and the reviewed guarded host preflight and Journal. Keep the existing
public `MdkOwner::open` entry point and stock behavior. This slice supplies
an internal production constructor and tests its composed startup and
early error paths with real profile contents. The daemon starts using it
with the reviewed physical stop integration. A single private constructor
uses a mode carrying either stock behavior or the exact registered lease
and Admission. The existing public `open` and the new internal constructor
are wrappers. Share validation, derivation, the owner literal, connector
construction and reconciliations. Stock mode preserves existing connection
opens, drop behavior, error categories and ordering.

## One checked owner construction

Use one exact `Arc<ProfileLease>` and registered `Admission` supplied by the
already-reviewed qualifying branch. The registered candidate constructor
requires both. It has no stock fallback. The test follows the same
lease and registration sequence and passes the same capabilities. Use the
process default verifier immediately before a synchronous MDK opener,
then place every successful account root in custody before the next
admission. Recheck independently for explicit close and cold reopen.
The registrar holds no mutex across native work. Use the existing
`native_preflight::check_with_fence` before account construction, using
that same Admission, and preserve its failure precedence.

The custody token retains an exact strong account root, lease and
Admission. When it cannot admit a native close, set the upstream inert
`begin_close` flag, keep the account root and lease through process exit,
and report the fixed fatal error on the explicit path. Clones for the
runtime, engine and maintenance never become the last native owner.
Ordinary close uses the admitted upstream `close()`. Release its token
only after success. If upstream reports an error, trip the registered
custody-failure edge and park the root using the same rule. Its internal
checkpoint and possible cleanup retry belong to the one admitted close
body. Treat the release marker as observation only.

Keep the custody token as the last field of a candidate `MdkOwner`.
A partial-startup state holds each live account or Journal as soon as
it is returned. It seals and parks a live account before its Journal
drops on panic or abandonment. Transfer the token and Journal into the
owner literal together before the first post-construction reconciliation.
Run subsequent fallible calls in a closure. A healthy `Err` consumes the
local owner through explicit cleanup. A panic follows `MdkOwner::drop`,
which seals before any fields drop. A final admission check gates the
result. Release of both slots records an ordinary disposition. There
is no fallible action between moving the storage slots into the literal
and having the owner that holds them.

On an ordinary finish or a healthy startup error, retire terminal
authority, cancel the inert connector and verify that no forwarder,
runner or active owner was started. Admit cleanup of an acquired
Journal first and account second. Preserve the original error if
cleanup succeeds. A dispatcher admission refusal or failed admitted
close takes precedence. An opener error before a root is returned
leaves no Marmish-owned root to park.

Add `ExistingConnection::close_in_place(&mut self)` and
`Journal::close_in_place(&mut self)`, both using the existing checked
close body. Consuming Journal close delegates to that same body. The
consumed owner never reads the empty connection slot. Supply the
Journal with `Admission::connection_fence()` from the same registry
record. Clear account custody only after a successful upstream close.
After an upstream close returns an error the marker alone gives no
descriptor guarantee, since the upstream operation may already have
taken and discarded the connection. The failure still trips and holds
the original lease.

Drop sealing in the partial startup state, owner, and fallback token
uses only `begin_close`, the lock-free custody-failure notification
(`CustodyFault::report`, via Admission), and forgetting the root and
lease. It performs no new admission, verification or close itself.
The fatal edge fires before the guarded Journal's existing Drop checks
admission. The guard then parks its connection. Sealing must not panic.
Surviving account clones cannot take a new ordinary guard after sealing
and cannot be the last reference while a root is parked. Neither generic
Drop path is a physical stop for an active owner. Later mounting has to
order physical abort and reap before the explicit custody decision.

Admission is checked immediately before and after each independent
synchronous effect. This includes SQLCipher initialization, account
smoke open via `open_encrypted_observed`, explicit smoke close,
independent account reopen, guarded Journal open/close/reopen,
`key_package_lifecycle` read, and conditional
`put_key_package_lifecycle` write. The upstream observed-open callback
is a no-op in production. `AccountDeviceRuntime::open` receives
`SessionConfig::with_storage` and clones of the custodied root. Its
engine construction, imported legacy retirement, and hydration are
inside that admitted call. The existing inert transport factory is the
test seam after the session opens and before the post-call check.
Admit the binding check, revoked Journal conversations read,
each group retirement, orphan reconciliation, maintenance, route
rebuild and invalidation reconciliation separately. Each admitted
body can complete its synchronous nested effects. After a fatal
return, suppress its result and refuse the next independent effect.
Only transfer custody to the returned owner after the final check.

The ready profile's manifest remains read-only in this slice. Normal
setup, `daemon::open`, doctor and all unregistered storage behavior
continue through the existing entry point. The ordinary live daemon
mount must separately guard future owner turns and async effects, stop
all tasks, runners and raw sockets before parking, and route graceful
and fatal close through the same ownership. A guard alone cannot make
an active MdkOwner safe to drop while physical tasks are live.

## Acceptance

- A fresh subprocess can acquire the exact candidate registration after
  preparing a Ready profile, open its real MDK and Journal through the
  new constructor, inspect account and Journal metadata, and explicitly
  close after a fresh admission. A subsequent independent stock
  process can reopen the same profile.
- Controlled displacement before each smoke/open transition, an
  upstream error before ownership, a cut during runtime initialization,
  and a cut immediately before final return distinguish already admitted
  work from the next independent effect. Independently prove the
  succeeding Journal remains unchanged at earlier cuts, that an
  acquired account retains its descriptor after a fatal return, and
  that no caller receives a partially authorized owner. Use fresh
  subprocesses, fixed diagnostic categories and disposable identities.
- Test early failure with a healthy default, the admitted close with an
  anonymous clone, plain abandoned owner drop, and registered fatal or custody failure
  during the last release. Inject panic at the pre-literal ownership
  transitions and each post-literal reconciliation. Assert that a plain
  abandoned owner retains both main-file descriptors. Check that the
  suppressed runtime can drop, that the inert runtime construction
  opens no sockets and starts no thread, and that dropping a consumed
  owner after successful or fence-refused Journal close tolerates its
  empty connection slot. Inject a close error at the disposition
  boundary to qualify parking. This injection does not require a
  reproducible upstream busy-close error. Assert the exact original lease stays held until process
  exit on the parked branch. No clock measurement of an already admitted
  upstream call is used as a cancellation guarantee.
- Strict Linux AMD64 stock and candidate builds and focused tests pass.
  Confirm default and package checks still select stock behavior. Leave
  the production caller unmounted until the physical stop and final
  composed review. No private upstream dependency or replacement
  SQLite default is introduced.
