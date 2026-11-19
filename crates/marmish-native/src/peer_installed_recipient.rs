//! Owned startup and teardown for the private installed Manual recipient.

use super::{receiver_inventory::RecoveredOwner, socket::owner::recipient::PeerRecipientPort};
use crate::{
    dispatcher_fatal::Fatal,
    identity::{IdentityMaterial, ProfileLease},
    ipc::peer_read::pending_view::Producer,
    peer_candidate_startup::{Ready, WorkerStart},
    peer_relay_trust::{
        ReceiverRouteAdmission,
        candidate_registration::{ForwardingVerifier, ReceiverRouteControl, RegisteredReceiver},
    },
    transport::PeerSocketDrain,
};
use std::{
    io,
    panic::AssertUnwindSafe,
    sync::Arc,
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    runtime::Builder,
    time::{Instant, sleep, sleep_until},
};
use tokio_util::sync::CancellationToken;

const PORT_PROBE: Duration = Duration::from_millis(500);
const DRAIN_WAIT: Duration = Duration::from_secs(1);
const JOIN_POLL: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Exit {
    Stopped,
    LocalUnavailable,
    GlobalFault,
    WorkerFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PhysicalEnd {
    NoStart,
    Drained,
    Unproved,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Completion {
    pub(crate) exit: Exit,
    pub(crate) physical: PhysicalEnd,
}

impl Completion {
    fn new(exit: Exit, physical: PhysicalEnd) -> Self {
        Self { exit, physical }
    }

    fn finalize(self, forwarding: &ForwardingVerifier) -> Self {
        Self::new(final_exit(forwarding, self.exit), self.physical)
    }
}

#[must_use]
pub(crate) struct Actor {
    stop: CancellationToken,
    worker: Worker,
}

pub(crate) enum Service {
    Passive,
    Manual(Producer),
}

enum Worker {
    Owned(JoinHandle<Completion>),
    Joined(Completion),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Incomplete;

impl Actor {
    pub(crate) fn spawn(
        worker: WorkerStart,
        lease: Arc<ProfileLease>,
        material: IdentityMaterial,
        registration: RegisteredReceiver,
        service: Service,
    ) -> io::Result<Self> {
        let stop = CancellationToken::new();
        let worker_stop = stop.clone();
        let thread = thread::Builder::new()
            .name("marmish-peer-recipient".into())
            .spawn(move || {
                let forwarding = registration.forwarding_verifier();
                let Ok(runtime) = Builder::new_current_thread().enable_all().build() else {
                    return Completion::new(Exit::WorkerFailed, PhysicalEnd::NoStart)
                        .finalize(&forwarding);
                };
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    runtime.block_on(run(
                        worker,
                        &lease,
                        &material,
                        registration,
                        &worker_stop,
                        service,
                    ))
                }));
                result
                    .unwrap_or(Completion::new(Exit::WorkerFailed, PhysicalEnd::Unproved))
                    .finalize(&forwarding)
            })?;
        Ok(Self {
            stop,
            worker: Worker::Owned(thread),
        })
    }

    pub(crate) fn stop(&self) {
        self.stop.cancel();
    }

    /// The owned handle survives an overdue or canceled join. A request to
    /// stop is insufficient to release the ordinary owner's lease.
    pub(crate) async fn join_before(
        &mut self,
        deadline: Instant,
    ) -> Result<Completion, Incomplete> {
        loop {
            match &self.worker {
                Worker::Joined(exit) => return Ok(*exit),
                Worker::Owned(thread) if thread.is_finished() => break,
                Worker::Owned(_) => {}
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Incomplete);
            }
            sleep_until((now + JOIN_POLL).min(deadline)).await;
        }
        let unproved = Completion::new(Exit::WorkerFailed, PhysicalEnd::Unproved);
        let Worker::Owned(thread) = std::mem::replace(&mut self.worker, Worker::Joined(unproved))
        else {
            unreachable!("only a finished owned thread reaches join");
        };
        let completion = thread.join().unwrap_or(unproved);
        self.worker = Worker::Joined(completion);
        Ok(completion)
    }
}

impl Drop for Actor {
    fn drop(&mut self) {
        self.stop();
    }
}

struct StartupFence<'a> {
    ready: Ready,
    stop: &'a CancellationToken,
    forwarding: &'a ForwardingVerifier,
    fatal: &'a Fatal,
}

impl StartupFence<'_> {
    fn deadline(&self) -> Result<Instant, Exit> {
        check_stop(self.stop, self.forwarding)?;
        // The remainder is sampled after `now`, so the Tokio deadline can
        // only shorten the readiness budget recorded at the second edge.
        let now = Instant::now();
        self.ready
            .startup_remaining()
            .map(|remaining| now + remaining)
            .ok_or_else(|| {
                if self.ready.is_current() {
                    Exit::LocalUnavailable
                } else {
                    Exit::Stopped
                }
            })
    }

    fn step<T, E>(&self, action: impl FnOnce() -> Result<T, E>) -> Result<T, Exit> {
        self.deadline()?;
        let result = action();
        self.deadline()?;
        result.map_err(|_| Exit::LocalUnavailable)
    }
}

async fn run(
    worker: WorkerStart,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    registration: RegisteredReceiver,
    stop: &CancellationToken,
    service: Service,
) -> Completion {
    let forwarding = registration.forwarding_verifier();
    let fatal = forwarding.fatal();
    let ready = tokio::select! {
        biased;
        () = fatal.tripped() => return Completion::new(Exit::GlobalFault, PhysicalEnd::NoStart),
        () = stop.cancelled() => return Completion::new(Exit::Stopped, PhysicalEnd::NoStart),
        ready = worker.ready() => ready,
    };
    let Some(ready) = ready else {
        return Completion::new(Exit::Stopped, PhysicalEnd::NoStart);
    };
    let fence = StartupFence {
        ready,
        stop,
        forwarding: &forwarding,
        fatal: &fatal,
    };
    attach(lease, material, registration, fence, service)
        .await
        .unwrap_or_else(|exit| Completion::new(exit, PhysicalEnd::NoStart))
}

async fn attach(
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    registration: RegisteredReceiver,
    mut fence: StartupFence<'_>,
    service: Service,
) -> Result<Completion, Exit> {
    #[cfg(test)]
    tests::panic_at("panic_before_attach");
    let mut attached = fence.step(|| registration.attach(lease, material))?;
    let (quota, mut route) = attached.actor_parts();
    let mut owner = fence.step(|| {
        RecoveredOwner::open_for_actor(lease, material, quota, &mut route, || {
            fence.deadline().is_ok()
        })
    })?;
    let wall = fence.step(|| SystemTime::now().duration_since(UNIX_EPOCH))?;
    fence.step(|| owner.reconcile_startup(lease, quota, wall.as_secs()))?;

    let token = match fence.step(|| route.take_route(lease, material)) {
        Ok(token) => token,
        Err(exit) => {
            route.revoke();
            return Err(exit);
        }
    };
    let result = match ReceiverRouteAdmission::registered(token) {
        Ok((admission, drain)) => {
            let result = futures_util::FutureExt::catch_unwind(AssertUnwindSafe(hold_port(
                admission, &mut owner, &mut fence, service,
            )))
            .await;
            revoke_and_reap(fence.ready, &route, &drain).await;
            Completion::new(result.unwrap_or(Exit::WorkerFailed), PhysicalEnd::Drained)
        }
        Err(_) => {
            route.revoke();
            Completion::new(Exit::LocalUnavailable, PhysicalEnd::NoStart)
        }
    };
    Ok(result)
}

async fn hold_port(
    admission: ReceiverRouteAdmission,
    _owner: &mut RecoveredOwner<'_>,
    fence: &mut StartupFence<'_>,
    service: Service,
) -> Exit {
    // Keep the sole producer inside this future, including while opening the
    // port. Its drop closes the view before the caller reaps the route.
    let _producer = match service {
        Service::Passive => None,
        Service::Manual(producer) => Some(producer),
    };
    #[cfg(test)]
    tests::assert_manual_view_admitted();
    let deadline = match fence.deadline() {
        Ok(deadline) => deadline,
        Err(exit) => return exit,
    };
    let port = tokio::select! {
        biased;
        () = fence.fatal.tripped() => return Exit::GlobalFault,
        () = fence.stop.cancelled() => return Exit::Stopped,
        () = fence.ready.stopped() => return Exit::Stopped,
        result = PeerRecipientPort::open(admission, deadline) => result,
    };
    if let Err(exit) = fence.deadline() {
        return exit;
    }
    let Ok(mut port) = port else {
        return Exit::LocalUnavailable;
    };
    #[cfg(test)]
    {
        tests::assert_manual_view_admitted();
        tests::panic_at("panic_in_hold");
    }
    // Until the durable service mounts, detect the physical end only. An
    // incoming record stops this idle owner for recovery by a later service.
    loop {
        let outcome = tokio::select! {
            biased;
            () = fence.fatal.tripped() => return Exit::GlobalFault,
            () = fence.stop.cancelled() => return Exit::Stopped,
            () = fence.ready.stopped() => return Exit::Stopped,
            result = port.wait_live(Instant::now() + PORT_PROBE) => result,
        };
        if let Err(exit) = check_stop(fence.stop, fence.forwarding) {
            return exit;
        }
        if !matches!(outcome, Ok(None)) {
            return Exit::LocalUnavailable;
        }
    }
}

async fn revoke_and_reap(
    ready: Ready,
    route: &ReceiverRouteControl<'_, '_>,
    drain: &PeerSocketDrain,
) {
    drop(ready);
    route.revoke();
    #[cfg(test)]
    {
        tests::panic_at("panic_in_reap");
        tests::observe_view_before_reap().await;
    }
    while drain.reap(Instant::now() + DRAIN_WAIT).await.is_err() {
        // A physical reap which cannot be proved leaves this worker alive,
        // with its attached quota, keyed cursor and original profile intact.
        sleep(DRAIN_WAIT).await;
    }
}

fn check_stop(stop: &CancellationToken, forwarding: &ForwardingVerifier) -> Result<(), Exit> {
    match final_exit(forwarding, Exit::Stopped) {
        Exit::GlobalFault => Err(Exit::GlobalFault),
        _ if stop.is_cancelled() => Err(Exit::Stopped),
        _ => Ok(()),
    }
}

fn final_exit(forwarding: &ForwardingVerifier, otherwise: Exit) -> Exit {
    if forwarding.recheck_default().is_err() || forwarding.fatal().is_tripped() {
        Exit::GlobalFault
    } else {
        otherwise
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command_config::CommandConfig,
        mdk_adapter::terminal_relay_fixture::{RecipientFault, SocketEnd},
        peer_candidate_startup,
        peer_relay_trust::{
            candidate_registration::ReceiverPreflight, qualified_manual_recipient_fixture,
        },
    };
    use nostr::key::Keys;
    use std::{
        fs::{self, OpenOptions},
        num::NonZeroU64,
        os::unix::fs::{OpenOptionsExt, symlink},
        path::Path,
        process::{Command, Stdio},
        sync::{
            OnceLock,
            atomic::{AtomicBool, AtomicU8, Ordering},
        },
    };
    use tokio::time::timeout;

    const CASE: &str = "MARMISH_TEST_ACTOR_START_CASE";
    const ROOT: &str = "MARMISH_TEST_ACTOR_START_ROOT";
    const RUNTIME_SOCKET: usize = 5;
    static AT_VIEW_REAP: AtomicBool = AtomicBool::new(false);
    static RELEASE_VIEW_REAP: AtomicBool = AtomicBool::new(false);
    static ADMITTED_EDGES: AtomicU8 = AtomicU8::new(0);
    static MANUAL_READER: OnceLock<crate::ipc::peer_read::pending_view::Reader> = OnceLock::new();

    pub(super) fn assert_manual_view_admitted() {
        if let Some(reader) = MANUAL_READER.get() {
            assert!(reader.admit(NonZeroU64::MIN).is_some());
            ADMITTED_EDGES.fetch_add(1, Ordering::Release);
        }
    }

    fn observes_manual_view(case: &str) -> bool {
        matches!(case, "open_refused" | "preexisting_empty" | "panic_in_hold")
    }

    pub(super) async fn observe_view_before_reap() {
        if std::env::var(CASE).is_ok_and(|case| observes_manual_view(&case)) {
            AT_VIEW_REAP.store(true, Ordering::Release);
            eventually(|| RELEASE_VIEW_REAP.load(Ordering::Acquire)).await;
        }
    }

    pub(super) fn panic_at(stage: &str) {
        assert!(
            std::env::var(CASE).as_deref() != Ok(stage),
            "intentional recipient panic at {stage}"
        );
    }

    #[tokio::test]
    async fn uncaught_thread_panic_cannot_prove_physical_end() {
        let mut actor = Actor {
            stop: CancellationToken::new(),
            worker: Worker::Owned(thread::spawn(|| panic!("intentional out-of-band panic"))),
        };
        let expected = Ok(Completion::new(Exit::WorkerFailed, PhysicalEnd::Unproved));
        assert_eq!(
            actor
                .join_before(Instant::now() + Duration::from_secs(1))
                .await,
            expected
        );
        assert_eq!(actor.join_before(Instant::now()).await, expected);
    }

    #[test]
    #[ignore = "uses fresh subprocesses, private WSS relay and SQLCipher"]
    fn peer_installed_recipient_owns_start_and_stop() {
        let parent = tempfile::tempdir().unwrap();
        let child = concat!(module_path!(), "::actor_start_subprocess");
        let (_, child) = child.split_once("::").unwrap();
        for case in [
            "before_ready",
            "stop_during_auth",
            "open_refused",
            "fatal_after_open",
            "preexisting_empty",
            "sidecar_only",
            "malformed",
            "replaced_while_preparing",
            "drain_pending",
            "panic_before_attach",
            "panic_in_hold",
            "panic_in_reap",
        ] {
            let mut process = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", child, "--nocapture"])
                .env_clear()
                .env(CASE, case)
                .env(ROOT, parent.path())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let until = std::time::Instant::now() + Duration::from_secs(30);
            while process.try_wait().unwrap().is_none() {
                if std::time::Instant::now() >= until {
                    let _ = process.kill();
                    let _ = process.wait();
                    panic!("recipient actor subprocess exceeded its deadline: {case}");
                }
                thread::sleep(Duration::from_millis(10));
            }
            let result = process.wait_with_output().unwrap();
            let stdout = String::from_utf8_lossy(&result.stdout);
            assert!(
                result.status.success() && stdout.contains("1 passed"),
                "{case} {stdout} {}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }

    #[tokio::test]
    #[ignore = "run through peer_installed_recipient_owns_start_and_stop"]
    async fn actor_start_subprocess() {
        let Some(parent) = std::env::var_os(ROOT) else {
            return;
        };
        let case = std::env::var(CASE).unwrap();
        let (mut relay, _temp, lease, material) = qualified_manual_recipient_fixture(
            Keys::generate().public_key(),
            Keys::generate().public_key(),
            Path::new(&parent),
            None,
        )
        .await;
        let account = material.public_key();
        let config = CommandConfig::load(lease.root()).unwrap();
        let mut registration = ReceiverPreflight::prepare(&lease, &material, &config)
            .unwrap()
            .unwrap()
            .register(&lease, &material)
            .unwrap();
        let (_, _, custody) = registration.take_command_fence().unwrap().into_parts();
        let cursor = lease.root().join("peer/cursor.db");
        assert!(!cursor.try_exists().unwrap());
        let (startup, listener, worker) = peer_candidate_startup::pair();
        let readiness = listener.readiness();

        if matches!(case.as_str(), "replaced_while_preparing" | "drain_pending") {
            let mut attached = registration.attach(&lease, &material).unwrap();
            let (quota, mut route) = attached.actor_parts();
            startup.ordinary_ready();
            listener.registered();
            let ready = worker.ready().await.unwrap();
            if case == "replaced_while_preparing" {
                let original = lease.root().join("cursor-original-test");
                let result =
                    RecoveredOwner::open_for_actor(&lease, &material, quota, &mut route, || {
                        fs::rename(&cursor, &original).unwrap();
                        create_private(&cursor);
                        true
                    });
                assert!(result.is_err());
                assert_eq!(fs::metadata(&cursor).unwrap().len(), 0);
                assert_eq!(fs::metadata(original).unwrap().len(), 0);
            } else {
                let token = route.take_route(&lease, &material).unwrap();
                let (admission, drain) = ReceiverRouteAdmission::registered(token).unwrap();
                assert!(readiness.is_current());
                let mut pending = std::pin::pin!(revoke_and_reap(ready, &route, &drain));
                assert!(
                    timeout(Duration::from_millis(30), pending.as_mut())
                        .await
                        .is_err()
                );
                assert!(!readiness.is_current());
                drop(admission);
                timeout(Duration::from_secs(2), pending).await.unwrap();
            }
            assert!(!relay.runtime_socket_active(RUNTIME_SOCKET));
            assert_eq!(relay.all_publication_attempts(), 0);
            relay.stop().await;
            return;
        }

        let sidecar = cursor.with_file_name("cursor.db-wal");
        const MALFORMED: &[u8] = b"private malformed test cursor";
        match case.as_str() {
            "preexisting_empty" => create_private(&cursor),
            "sidecar_only" => symlink("missing-target", &sidecar).unwrap(),
            "malformed" => {
                create_private(&cursor);
                fs::write(&cursor, MALFORMED).unwrap();
            }
            _ => {}
        }
        let (service, reader) = if observes_manual_view(&case) {
            let (producer, reader) = crate::ipc::peer_read::pending_view::pair(NonZeroU64::MIN)
                .expect("one private projection");
            assert!(reader.admit(NonZeroU64::MIN).is_some());
            assert!(MANUAL_READER.set(reader.clone()).is_ok());
            (Service::Manual(producer), Some(reader))
        } else {
            (Service::Passive, None)
        };
        let mut actor = Actor::spawn(worker, lease, material, registration, service).unwrap();
        assert_eq!(actor.join_before(Instant::now()).await, Err(Incomplete));

        let expected = match case.as_str() {
            "before_ready" => {
                startup.ordinary_ready();
                assert_eq!(
                    actor
                        .join_before(Instant::now() + Duration::from_millis(50))
                        .await,
                    Err(Incomplete)
                );
                assert!(!cursor.try_exists().unwrap());
                assert_eq!(relay.qualification_connections(), RUNTIME_SOCKET - 1);
                actor.stop();
                Exit::Stopped
            }
            "stop_during_auth" => {
                relay.recipient_fault(RecipientFault::MissingAuthAck);
                listener.registered();
                startup.ordinary_ready();
                eventually(|| relay.authenticated(account)).await;
                assert_eq!(relay.recipient_requests(account), 0);
                actor.stop();
                Exit::Stopped
            }
            "open_refused" => {
                relay.recipient_fault(RecipientFault::NegativeAuthAck);
                listener.registered();
                startup.ordinary_ready();
                Exit::LocalUnavailable
            }
            "panic_before_attach" | "panic_in_hold" => {
                startup.ordinary_ready();
                listener.registered();
                Exit::WorkerFailed
            }
            "fatal_after_open" | "preexisting_empty" | "panic_in_reap" => {
                startup.ordinary_ready();
                listener.registered();
                eventually(|| relay.recipient_requests(account) == 1).await;
                assert!(relay.runtime_socket_active(RUNTIME_SOCKET));
                assert_ne!(fs::metadata(&cursor).unwrap().len(), 0);
                match case.as_str() {
                    "fatal_after_open" => {
                        custody.report();
                        Exit::GlobalFault
                    }
                    "panic_in_reap" => {
                        actor.stop();
                        Exit::WorkerFailed
                    }
                    _ => {
                        actor.stop();
                        Exit::Stopped
                    }
                }
            }
            "sidecar_only" | "malformed" => {
                startup.ordinary_ready();
                listener.registered();
                Exit::LocalUnavailable
            }
            _ => panic!("unknown actor case"),
        };
        if let Some(reader) = reader {
            eventually(|| AT_VIEW_REAP.load(Ordering::Acquire)).await;
            // Listener registration stays alive and holds no view permit.
            // Inspect the Reader directly, without its readiness or fatal fence.
            assert!(reader.admit(NonZeroU64::MIN).is_none());
            let expected_admissions = if case == "open_refused" { 1 } else { 2 };
            assert_eq!(ADMITTED_EDGES.load(Ordering::Acquire), expected_admissions);
            assert_eq!(actor.join_before(Instant::now()).await, Err(Incomplete));
            RELEASE_VIEW_REAP.store(true, Ordering::Release);
        }
        let physical = match case.as_str() {
            "before_ready" | "sidecar_only" | "malformed" => PhysicalEnd::NoStart,
            "panic_before_attach" | "panic_in_reap" => PhysicalEnd::Unproved,
            _ => PhysicalEnd::Drained,
        };
        let expected = Completion::new(expected, physical);
        assert_eq!(
            actor
                .join_before(Instant::now() + Duration::from_secs(5))
                .await,
            Ok(expected)
        );
        assert_eq!(actor.join_before(Instant::now()).await, Ok(expected));
        assert!(!readiness.is_current());
        match case.as_str() {
            "fatal_after_open" | "stop_during_auth" | "preexisting_empty" | "panic_in_hold" => {
                eventually(|| relay.runtime_socket_end(RUNTIME_SOCKET) == Some(SocketEnd::Raw))
                    .await;
                assert_eq!(relay.recipient_closes(account), 0);
            }
            "open_refused" => {
                assert_eq!(relay.qualification_connections(), RUNTIME_SOCKET);
                assert_eq!(relay.recipient_requests(account), 0);
                assert!(!relay.runtime_socket_active(RUNTIME_SOCKET));
            }
            "panic_before_attach" => {
                assert!(!cursor.try_exists().unwrap());
                assert_eq!(relay.qualification_connections(), RUNTIME_SOCKET - 1);
            }
            "sidecar_only" => {
                assert!(!cursor.try_exists().unwrap());
                assert_eq!(fs::read_link(sidecar).unwrap(), Path::new("missing-target"));
            }
            "malformed" => assert_eq!(fs::read(cursor).unwrap(), MALFORMED),
            _ => {}
        }
        assert!(!relay.runtime_socket_active(RUNTIME_SOCKET + 1));
        assert_eq!(relay.all_publication_attempts(), 0);
        relay.stop().await;
    }

    fn create_private(path: &Path) {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
    }

    async fn eventually(mut condition: impl FnMut() -> bool) {
        timeout(Duration::from_secs(8), async {
            while !condition() {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
}
