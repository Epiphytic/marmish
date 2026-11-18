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

Retain custody across failure and panic before the normal `MdkOwner` can
be returned. The candidate constructor closes the smoke account
explicitly, reopens it independently, acquires the guarded Journal,
closes and reopens it with separate checks, then initializes the
account-device runtime from `SessionConfig::with_storage`. The runtime
and pre-existing MdkOwner storage field receive only clones of the
custodied connection. Confirm the guard survives any fallible operation
between the first account clone and the final owning value. Keep field
ownership compatible with the later physical stop design. Binding
checks, imported group retirement, recovery, reconciliation and native
inventory are separate synchronous startup effects. Admit each before
calling it and suppress a successful returned owner if the final
admission fails. The initially constructed connector is still inert,
and construction must not start a relay socket, producer, or child
runner. Preserve original errors when admission remains healthy. An
observed fatal wins over errors from a preceding admitted body.

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
  anonymous clone, and registered fatal or custody failure during the
  last release. Assert the exact original lease stays held until process
  exit on the parked branch. No clock measurement of an already admitted
  upstream call is used as a cancellation guarantee.
- Strict Linux AMD64 stock and candidate builds and focused tests pass.
  Confirm default and package checks still select stock behavior. Leave
  the production caller unmounted until the physical stop and final
  composed review. No private upstream dependency or replacement
  SQLite default is introduced.
