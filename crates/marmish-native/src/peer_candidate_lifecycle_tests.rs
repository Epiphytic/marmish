//! Test adapter for the ordinary daemon loop. It owns no Peer store or route.

use super::{Config, FixtureTransport, ProfileLease, run_inner};
use crate::{Error, Result, ipc, peer_candidate_startup as startup, transport::RelayConnector};
use std::{path::PathBuf, sync::Arc};
use tokio::{net::UnixListener, sync::mpsc, sync::oneshot, task::JoinHandle};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ListenerMode {
    Normal,
    WrongPath,
    TransientAcceptError,
    UnexpectedExit,
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Registered,
    RegistrationRefused,
    AcceptedRetry,
    OrdinaryReady,
    WorkerReady,
    WorkerStopped,
    BeforeDrain { ready: bool },
}

pub(super) struct Fixture {
    connector: RelayConnector,
    layout: PathBuf,
    mode: ListenerMode,
    events: mpsc::UnboundedSender<Event>,
    stop: oneshot::Receiver<()>,
    release_join: Option<oneshot::Receiver<()>>,
}

impl Fixture {
    pub(super) fn transport(&self) -> FixtureTransport<'_> {
        FixtureTransport {
            connector: Some(&self.connector),
            layout: &self.layout,
        }
    }

    pub(super) fn start(self, lease: Arc<ProfileLease>) -> (Runtime, oneshot::Receiver<()>) {
        let (startup, registration, worker) = startup::pair();
        let events = self.events.clone();
        let release = self.release_join;
        let worker = tokio::spawn(async move {
            if let Some(mut ready) = worker.ready().await {
                if ready.is_current() {
                    let _ = events.send(Event::WorkerReady);
                }
                ready.stopped().await;
            }
            let _ = events.send(Event::WorkerStopped);
            if let Some(release) = release {
                let _ = release.await;
            }
            drop(lease);
        });
        let runtime = Runtime {
            startup,
            registration: Some(registration),
            events: self.events,
            mode: self.mode,
            worker,
        };
        (runtime, self.stop)
    }
}

pub(super) struct Runtime {
    startup: startup::Startup,
    registration: Option<startup::ListenerRegistration>,
    events: mpsc::UnboundedSender<Event>,
    mode: ListenerMode,
    worker: JoinHandle<()>,
}

impl Runtime {
    pub(super) fn listener(&mut self, mut socket: PathBuf) -> Listener {
        if self.mode == ListenerMode::WrongPath {
            socket.set_extension("wrong");
        }
        Listener {
            socket,
            registration: self
                .registration
                .take()
                .expect("single listener registration"),
            events: self.events.clone(),
            mode: self.mode,
        }
    }

    pub(super) fn ordinary_ready(&self) {
        let _ = self.events.send(Event::OrdinaryReady);
        self.startup.ordinary_ready();
    }

    pub(super) fn stop(&self) {
        self.startup.stop();
    }

    pub(super) async fn join(self) -> Result<()> {
        self.worker
            .await
            .map_err(|_| Error::Identity("owned_stop_incomplete"))
    }
}

pub(crate) struct Listener {
    socket: PathBuf,
    registration: startup::ListenerRegistration,
    events: mpsc::UnboundedSender<Event>,
    mode: ListenerMode,
}

impl Listener {
    pub(crate) fn register(&self, listener: &UnixListener) -> bool {
        if !listener
            .local_addr()
            .is_ok_and(|bound| bound.as_pathname() == Some(self.socket.as_path()))
        {
            let _ = self.events.send(Event::RegistrationRefused);
            return false;
        }
        let _ = self.events.send(Event::Registered);
        self.registration.registered();
        true
    }

    pub(crate) fn exits(&self) -> bool {
        self.mode == ListenerMode::UnexpectedExit
    }

    pub(crate) fn transient_accept_error(&mut self) -> bool {
        if self.mode == ListenerMode::TransientAcceptError {
            self.mode = ListenerMode::Normal;
            let _ = self.events.send(Event::AcceptedRetry);
            return true;
        }
        false
    }

    pub(crate) fn before_drain(&self) {
        let _ = self.events.send(Event::BeforeDrain {
            ready: self.registration.readiness().is_current(),
        });
    }
}

use crate::{
    identity::{BackendChoice, socket_path_for},
    mdk_adapter::terminal_relay_fixture::Relay,
    transport::{RelayPolicy, RouteMode, TransportLimits},
};
use std::{path::Path, time::Duration};
use tokio::time::timeout;

const PROFILE: &str = "inert";
const WAIT: Duration = Duration::from_secs(30);

fn inputs(
    root: &Path,
    relay: &Relay,
    mode: ListenerMode,
    release_join: Option<oneshot::Receiver<()>>,
) -> (
    Config,
    Fixture,
    oneshot::Sender<()>,
    mpsc::UnboundedReceiver<Event>,
) {
    let executable = std::env::current_exe().unwrap();
    let (stop, stop_receiver) = oneshot::channel();
    let (events, receive) = mpsc::unbounded_channel();
    let fixture = Fixture {
        connector: relay.connector(&root.join(PROFILE), true, None),
        layout: executable.clone(),
        mode,
        events,
        stop: stop_receiver,
        release_join,
    };
    let config = Config {
        state_dir: root.to_owned(),
        identity: PROFILE.into(),
        backend: BackendChoice::File,
        helper: executable,
        relay_policy: RelayPolicy::parse(
            &[relay.url.to_string()],
            RouteMode::Off,
            TransportLimits::default(),
        )
        .unwrap(),
        allowed_peers: vec![],
        retention: Duration::from_secs(86_400),
        max_commands: None,
        max_uptime: None,
    };
    (config, fixture, stop, receive)
}

async fn until(events: &mut mpsc::UnboundedReceiver<Event>, expected: Event) -> Vec<Event> {
    timeout(WAIT, async {
        let mut observed = Vec::new();
        loop {
            let event = events.recv().await.expect("lifecycle event channel closed");
            if event == expected {
                return observed;
            }
            observed.push(event);
        }
    })
    .await
    .expect("expected lifecycle event")
}

async fn ordinary_round_trip(root: &Path) {
    let socket = socket_path_for(root, PROFILE).unwrap();
    let status = ipc::request(&socket, &ipc::Request::Status {})
        .await
        .unwrap();
    assert_eq!(status.result.unwrap()["state"], "active");
    let read = ipc::request(
        &socket,
        &ipc::Request::Received {
            after: None,
            limit: 1,
        },
    )
    .await
    .unwrap();
    assert_eq!(read.result.unwrap().as_array().unwrap().len(), 0);
    #[cfg(marmish_peer_receiver_candidate)]
    {
        let unavailable = ipc::request(
            &socket,
            &ipc::Request::PeerRead {
                request: ipc::peer_read::Request::List {},
            },
        )
        .await
        .unwrap();
        assert_eq!(unavailable.error.as_deref(), Some("peer_unavailable"));
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in private relay lifecycle proof"]
async fn inert_daemon_readiness_retry_and_owned_drain() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let mut relay = Relay::start().await;
            let (release, blocked) = oneshot::channel();
            let (config, fixture, stop, mut events) = inputs(
                root.path(),
                &relay,
                ListenerMode::TransientAcceptError,
                Some(blocked),
            );
            let task = tokio::task::spawn_local(run_inner(config, None, None, Some(fixture)));
            let events_before = until(&mut events, Event::WorkerReady).await;
            assert!(events_before.contains(&Event::Registered));
            assert!(events_before.contains(&Event::OrdinaryReady));
            assert!(events_before.contains(&Event::AcceptedRetry));
            ordinary_round_trip(root.path()).await;
            assert!(!root.path().join(PROFILE).join("peer").exists());
            stop.send(()).unwrap();
            let observed = until(&mut events, Event::BeforeDrain { ready: false }).await;
            if !observed.contains(&Event::WorkerStopped) {
                until(&mut events, Event::WorkerStopped).await;
            }
            assert!(!task.is_finished());
            assert!(matches!(
                ProfileLease::acquire(root.path(), PROFILE),
                Err(Error::Identity("profile_in_use"))
            ));
            let socket = socket_path_for(root.path(), PROFILE).unwrap();
            timeout(WAIT, async {
                while tokio::net::UnixStream::connect(&socket).await.is_ok() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            release.send(()).unwrap();
            timeout(WAIT, task).await.unwrap().unwrap().unwrap();
            assert!(ProfileLease::acquire(root.path(), PROFILE).is_ok());
            relay.stop().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in private relay lifecycle proof"]
async fn inert_daemon_registration_refusal_keeps_ordinary_socket() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let mut relay = Relay::start().await;
            let (config, fixture, stop, mut events) =
                inputs(root.path(), &relay, ListenerMode::WrongPath, None);
            let task = tokio::task::spawn_local(run_inner(config, None, None, Some(fixture)));
            let mut observed = until(&mut events, Event::OrdinaryReady).await;
            if !observed.contains(&Event::WorkerStopped) {
                observed.extend(until(&mut events, Event::WorkerStopped).await);
            }
            assert!(observed.contains(&Event::RegistrationRefused));
            assert!(!observed.contains(&Event::WorkerReady));
            ordinary_round_trip(root.path()).await;
            stop.send(()).unwrap();
            timeout(WAIT, task).await.unwrap().unwrap().unwrap();
            relay.stop().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in private relay lifecycle proof"]
async fn inert_daemon_bind_refusal_stops_waiting_worker() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let mut relay = Relay::start().await;
            let (config, fixture, _stop, mut events) =
                inputs(root.path(), &relay, ListenerMode::Normal, None);
            let base = root.path().to_owned();
            let helper = config.helper.clone();
            let socket = tokio::task::spawn_blocking(move || {
                let mut lease = ProfileLease::acquire(&base, PROFILE).unwrap();
                lease
                    .prepare(
                        BackendChoice::File,
                        &helper,
                        std::time::Instant::now() + WAIT,
                    )
                    .unwrap();
                let path = lease.socket_path();
                std::fs::write(&path, b"obstruction").unwrap();
                path
            })
            .await
            .unwrap();
            let result = timeout(WAIT, run_inner(config, None, None, Some(fixture)))
                .await
                .unwrap();
            assert!(matches!(result, Err(Error::Invalid("unsafe_socket_path"))));
            assert!(until(&mut events, Event::WorkerStopped).await.is_empty());
            assert_eq!(std::fs::read(&socket).unwrap(), b"obstruction");
            let lease = ProfileLease::acquire(root.path(), PROFILE).unwrap();
            assert_eq!(lease.socket_path(), socket);
            std::fs::remove_file(socket).unwrap();
            lease.cleanup_runtime().unwrap();
            relay.stop().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in private relay lifecycle proof"]
async fn inert_daemon_observes_actual_listener_exit() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let mut relay = Relay::start().await;
            let (config, fixture, _stop, mut events) =
                inputs(root.path(), &relay, ListenerMode::UnexpectedExit, None);
            let result = timeout(WAIT, run_inner(config, None, None, Some(fixture)))
                .await
                .unwrap();
            assert!(matches!(
                result,
                Err(Error::Invalid("candidate_fixture_listener_exit"))
            ));
            let observed = until(&mut events, Event::WorkerStopped).await;
            assert!(observed.contains(&Event::Registered));
            assert!(!observed.contains(&Event::WorkerReady));
            assert!(ProfileLease::acquire(root.path(), PROFILE).is_ok());
            relay.stop().await;
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "opt-in private relay lifecycle proof"]
async fn inert_daemon_reporter_wait_prevents_release_on_failure_or_stop() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    tokio::task::LocalSet::new()
        .run_until(async {
            let mut relay = Relay::start().await;
            for stop_during_ack in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let (config, fixture, stop, mut events) =
                    inputs(root.path(), &relay, ListenerMode::Normal, None);
                let (reporter, mut parent) =
                    crate::daemon_launch::StartupReporter::test_channel().unwrap();
                let task = tokio::task::spawn_local(run_inner(
                    config,
                    Some(reporter),
                    None,
                    Some(fixture),
                ));
                let mut message = [0];
                timeout(WAIT, parent.read_exact(&mut message))
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(&message, b"R");
                let observed = until(&mut events, Event::Registered).await;
                assert!(observed.is_empty());
                assert!(matches!(
                    events.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ));
                ordinary_round_trip(root.path()).await;
                if stop_during_ack {
                    stop.send(()).unwrap();
                    parent.write_all(b"A").await.unwrap();
                } else {
                    parent.write_all(b"X").await.unwrap();
                }
                let result = timeout(WAIT, task).await.unwrap().unwrap();
                if stop_during_ack {
                    result.unwrap();
                } else {
                    assert!(matches!(result, Err(Error::DaemonStartup(_))));
                }
                let observed: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
                assert!(observed.contains(&Event::WorkerStopped));
                assert!(observed.contains(&Event::BeforeDrain { ready: false }));
                assert!(!observed.contains(&Event::OrdinaryReady));
                assert!(!observed.contains(&Event::WorkerReady));
                assert!(ProfileLease::acquire(root.path(), PROFILE).is_ok());
            }
            relay.stop().await;
        })
        .await;
}
