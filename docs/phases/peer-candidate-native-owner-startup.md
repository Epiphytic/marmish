# Guard the native owner during candidate startup

This is the installed account-owner construction slice of the
[daemon mount](peer-candidate-daemon-mount.md) and
[dispatcher shutdown](peer-candidate-dispatcher-shutdown.md) designs.
It builds on the [account custody qualification](peer-candidate-mdk-owner.md)
and the reviewed guarded host preflight and Journal. Keep the existing
public `MdkOwner::open` entry point and stock behavior. This slice supplies
an internal production constructor and tests its composed startup and
early error paths with real profile contents. The daemon starts using it
with the reviewed physical stop integration.

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
Use a startup state that owns the account as soon as it is returned and
owns the Journal as soon as it is returned. On panic or abandonment its
drop path seals and parks a live account before dropping the Journal.
On a healthy error, close any acquired Journal first and any acquired
account second, with separate fresh admissions. Preserve the original
error if cleanup passes. An observed fatal or failed admitted close
takes precedence and the account is parked. An opener error before
returning any root leaves no Marmish-owned account to park.

For ordinary close consume the inactive owner. After retiring its local
authority and checking that no runner or relay task was started, close
its Journal first and its account second. Reuse the mutable close
operation of the guarded connection via a narrowly scoped in-place
Journal close. The owner is consumed so no code reads the empty
connection slot. Clear account custody only on successful upstream
close. On an unexpected drop, `MdkOwner::drop` seals the live token
before normal fields drop, including the Journal. It reports custody
failure through its original Admission and parks the upstream root and
its exact lease. Setting the upstream inert close flag keeps surviving
clones from taking a new ordinary storage guard. The last-field token
is also a fallback for partial construction. Neither generic Drop path
is a physical stop for an active owner. The unmounted constructor
cannot start a socket, producer or command runner. Later mounting has
to order physical abort and reap before the explicit custody decision.

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
  during the last release. Inject a close error at the disposition
  boundary to qualify parking. This injection does not require a
  reproducible upstream busy-close error. Assert the exact original lease stays held until process
  exit on the parked branch. No clock measurement of an already admitted
  upstream call is used as a cancellation guarantee.
- Strict Linux AMD64 stock and candidate builds and focused tests pass.
  Confirm default and package checks still select stock behavior. Leave
  the production caller unmounted until the physical stop and final
  composed review. No private upstream dependency or replacement
  SQLite default is introduced.
