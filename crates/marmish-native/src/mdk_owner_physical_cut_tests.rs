use super::*;
use crate::{
    command_config::CommandConfig,
    command_engine::{Admission, EngineSettings, Readiness},
    command_service::CommandService,
    dispatcher_fatal::{Fatal, Trip},
    identity::{BackendChoice, IdentityMaterial},
    mdk_adapter::{
        AuthenticatedCommandContext, CommandDirection,
        terminal_relay_fixture::{Relay, SocketEnd},
    },
    stream_runner::RunnerLimits,
    stream_service::{LiveStream, StreamRecord},
    terminal_fence::FenceState,
    transport::{ArtiDirs, RelayPolicy, RouteMode, TransportLimits},
};
use marmish_stream::{codec::Codec, wire::Binding};
use nostr::key::Keys;
use std::{fs, path::Path};
use tokio::sync::oneshot;

const PROFILE: &str = "ordinary";

struct Fixture {
    directory: tempfile::TempDir,
    lease: Arc<ProfileLease>,
    material: IdentityMaterial,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut lease = ProfileLease::acquire(directory.path(), PROFILE).unwrap();
        let material = lease
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                std::time::Instant::now() + Duration::from_secs(3),
            )
            .unwrap();
        Self {
            directory,
            lease: Arc::new(lease),
            material,
        }
    }

    fn connector(&self, mode: RouteMode) -> RelayConnector {
        let policy = RelayPolicy::parse(
            &["wss://127.0.0.1:9/".into()],
            mode,
            TransportLimits::default(),
        )
        .unwrap();
        RelayConnector::new(
            policy,
            ArtiDirs {
                state: self.lease.arti_state().to_owned(),
                cache: self.lease.arti_cache().to_owned(),
            },
        )
        .unwrap()
    }

    fn open(&self, connector: RelayConnector) -> Box<MdkOwner> {
        Box::new(MdkOwner::open(&self.lease, &self.material, connector, vec![]).unwrap())
    }

    fn install_engine(
        &self,
        owner: &mut MdkOwner,
        trip: &Arc<Trip>,
        relay_url: &str,
        execution_client: [u8; 32],
    ) {
        let config = CommandConfig::from_bytes(
            &serde_json::to_vec(&serde_json::json!({
                "version":1, "role":"server", "relays":[relay_url], "tor":"off",
                "execution_clients":[hex::encode(execution_client)],
            }))
            .unwrap(),
        )
        .unwrap();
        owner.journal.command_configure(config.limits()).unwrap();
        let account = self.material.public_key().to_bytes();
        let identity = crate::command_identity::derive(
            self.lease.profile_id(),
            account,
            self.material.sqlcipher_root(),
        )
        .unwrap();
        let policy = config
            .engine_policy(
                self.lease.root().join("commands"),
                std::env::current_exe().unwrap(),
                account,
                &[],
            )
            .unwrap()
            .unwrap();
        let engine = CommandEngine::start_candidate(
            EngineSettings {
                policy,
                binding: identity.binding,
                master: identity.master,
                canonical_native_image: None,
            },
            vec![],
            Admission::new(trip, || true),
        )
        .unwrap();
        owner.commands = Some(CommandService::new(config, Some(engine)));
    }
}

fn assert_held(directory: &Path) {
    assert!(matches!(
        ProfileLease::acquire(directory, PROFILE),
        Err(Error::Identity("profile_in_use"))
    ));
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn direct(fixture: &Fixture, fatal: Fatal) -> RelayConnector {
    fixture.connector(RouteMode::Off).with_fatal(fatal).unwrap()
}

fn rejected(owner: Box<MdkOwner>, lease: &Arc<ProfileLease>) -> Box<MdkOwner> {
    let address = &*owner as *const MdkOwner;
    let active = owner.active;
    let stopping = owner.streams.stopping;
    let paused = owner.publication.blocked();
    let owner = PhysicalCut::begin(owner, lease).err().unwrap();
    assert_eq!(&*owner as *const MdkOwner, address);
    assert_eq!(owner.active, active);
    assert_eq!(owner.streams.stopping, stopping);
    assert_eq!(owner.publication.blocked(), paused);
    owner
}

// This disposable process has never lost native dispatch. Only this fixture
// dismantles the deliberately parked storage and lease after all tasks end.
fn release_fixture(parked: ParkedOrdinary) {
    drop(ManuallyDrop::into_inner(parked.owner));
    drop(ManuallyDrop::into_inner(parked.lease));
    assert!(ManuallyDrop::into_inner(parked.refused_engine).is_none());
}

#[test]
fn rejects_without_cut_authority_and_preserves_the_owner() {
    let fixture = Fixture::new();
    let other = Fixture::new();
    let (trip, fatal) = Trip::new();
    assert!(matches!(
        fixture
            .connector(RouteMode::Always)
            .with_fatal(fatal.clone()),
        Err(Error::Transport("dispatcher_fatal_direct_only"))
    ));
    let owner = fixture.open(direct(&fixture, fatal.clone()));
    trip.trip();
    let mut owner = rejected(owner, &fixture.lease);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    owner = rejected(owner, &other.lease);
    owner.streams.stopping = true;
    drop(rejected(owner, &fixture.lease));
    let owner = fixture.open(fixture.connector(RouteMode::Off));
    drop(rejected(owner, &fixture.lease));

    let (trip, fatal) = Trip::new();
    let mut owner = rejected(fixture.open(direct(&fixture, fatal)), &fixture.lease);
    let unrelated = Arc::new(Trip::new().0);
    let peer = Keys::generate().public_key().to_bytes();
    fixture.install_engine(&mut owner, &unrelated, "wss://127.0.0.1:9/", peer);
    trip.trip();
    unrelated.trip();
    let mut owner = rejected(owner, &fixture.lease);
    let engine = owner
        .commands
        .as_mut()
        .unwrap()
        .take_engine_for_stop()
        .unwrap();
    engine.into_fatal_join().ok().unwrap().wait().unwrap();
}

#[tokio::test]
async fn cached_cut_handles_survive_cancelled_observation_and_failed_join() {
    let fixture = Fixture::new();
    let (trip, fatal) = Trip::new();
    let connector = direct(&fixture, fatal);
    let mut owner = fixture.open(connector.clone());
    let (release, delayed) = oneshot::channel();
    let forwarder = tokio::spawn(async move { delayed.await.unwrap() });
    let abort = forwarder.abort_handle();
    owner.forwarder = Some(forwarder);
    trip.trip();
    let mut cut = PhysicalCut::begin(owner, &fixture.lease).ok().unwrap();
    let mut cut = cut.finish().err().unwrap();
    let Fixture {
        directory, lease, ..
    } = fixture;
    drop(lease);
    {
        let observation = cut.observe(deadline());
        tokio::pin!(observation);
        std::future::poll_fn(|cx| {
            use std::future::Future as _;
            assert!(observation.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }
    let report = cut
        .observe(Instant::now() + Duration::from_millis(40))
        .await;
    assert_eq!(
        report,
        Report {
            engine: Barrier::Complete,
            runners: Barrier::Complete,
            sdk: Barrier::Complete,
            raw: Barrier::Complete,
            forwarder: Barrier::Pending,
        }
    );
    assert_held(directory.path());
    assert!(connector.reap_fatal_sockets(deadline()).await.is_err());

    abort.abort();
    let expected = Report {
        forwarder: Barrier::Incomplete,
        ..report
    };
    assert_eq!(cut.observe(deadline()).await, expected);
    drop(release);
    assert_eq!(cut.observe(deadline()).await, expected);
    let (parked, final_report) = cut.finish().ok().unwrap();
    assert_eq!(final_report, expected);
    assert_held(directory.path());
    release_fixture(parked);
    assert!(ProfileLease::acquire(directory.path(), PROFILE).is_ok());
}

#[tokio::test]
async fn previously_closed_slots_remain_incomplete() {
    let fixture = Fixture::new();
    let (trip, fatal) = Trip::new();
    let connector = direct(&fixture, fatal);
    connector.cancel();
    let owner = fixture.open(connector);
    trip.trip();
    let mut cut = PhysicalCut::begin(owner, &fixture.lease).ok().unwrap();
    let report = cut.observe(deadline()).await;
    assert_eq!(report.raw, Barrier::Incomplete);
    assert_eq!(cut.observe(deadline()).await, report);
    release_fixture(cut.finish().ok().unwrap().0);
}

fn database_bytes(path: &Path) -> [Option<Vec<u8>>; 4] {
    ["", "-wal", "-shm", "-journal"].map(|suffix| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::read(name) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("database snapshot: {}", error.kind()),
        }
    })
}

fn install_live_runner(
    owner: &mut MdkOwner,
    client: [u8; 32],
    server: [u8; 32],
) -> (tempfile::TempDir, RunnerHandle, String) {
    let record = StreamRecord {
        binding: Binding {
            stream: [1; 16],
            session: [2; 16],
            client,
            server,
        },
        group: vec![3; 32],
        server_side: true,
        hello: true,
        copy_hello: false,
        open_digest: Some([4; 32]),
        launch: Default::default(),
        codec: Codec::None,
        state: "active".into(),
        cancel: false,
        active: true,
        terminal: None,
        identity: None,
        child_disposition: None,
        revision: 1,
        sequence: 0,
        stdin_accepted: 0,
        stdin_delivered: 0,
        stdout_eof: None,
        stderr_eof: None,
        completed_at: None,
        last_exchange_digest: None,
        payload_drained: false,
        payload_expired: false,
        projection_invalidated: false,
    };
    owner.journal.stream_insert(&record).unwrap();
    let (runner_root, prepared) =
        crate::stream_runner::tests::prepare("printf ready; exec /bin/cat");
    let runner = RunnerHandle::start(
        prepared,
        RunnerLimits {
            drain_grace: Duration::from_millis(80),
            cancel_grace: Duration::from_millis(40),
            poll_interval: Duration::from_millis(2),
        },
    )
    .unwrap();
    let mut live = LiveStream::new();
    live.runner = Some(runner.clone());
    let key = record.key();
    owner.streams.live.insert(key.clone(), live);
    (runner_root, runner, key)
}

async fn until(mut condition: impl FnMut() -> bool) {
    let end = deadline();
    while !condition() {
        assert!(Instant::now() < end);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[test]
#[ignore = "opens a disposable private WSS, MDK owner, Command engine, and live child"]
fn ordinary_owner_physical_cut_over_private_wss() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(private_wss());
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn private_wss() {
    let fixture = Fixture::new();
    let mut relay = Relay::start().await;
    relay.record_terminal_socket_ends();
    let (trip, fatal) = Trip::new();
    let trip = Arc::new(trip);
    let connector = relay
        .connector(fixture.lease.root(), true, None)
        .with_fatal(fatal)
        .unwrap();
    let mut owner = fixture.open(connector);
    let local = fixture.material.public_key();
    let peer = Keys::generate().public_key().to_bytes();
    fixture.install_engine(&mut owner, &trip, relay.url.as_str(), peer);
    until(|| {
        matches!(
            owner
                .commands
                .as_mut()
                .unwrap()
                .engine_mut()
                .unwrap()
                .readiness(),
            Readiness::Ready
        )
    })
    .await;
    owner
        .activate(Instant::now() + Duration::from_secs(25))
        .await
        .unwrap();
    assert!(relay.authenticated(local));
    until(|| relay.recipient_requests(local) > 0).await;
    let socket = relay.terminal_connection_for(local);
    assert!(relay.runtime_socket_active(socket));
    assert!(
        owner
            .forwarder
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    );

    let (_runner_root, runner, key) = install_live_runner(&mut owner, peer, local.to_bytes());
    until(|| {
        runner
            .snapshot()
            .stdout
            .as_ref()
            .is_some_and(|out| out.data == b"ready")
    })
    .await;
    assert!(!runner.finished());
    assert_eq!(owner.journal.stream_get(&key).unwrap().unwrap().revision, 1);
    let binding = owner
        .terminal_authority
        .register(AuthenticatedCommandContext::test_fixture(
            local.to_bytes(),
            peer,
            vec![3; 32],
            [2; 16],
            CommandDirection::Server,
        ))
        .unwrap();
    assert_eq!(binding.fence.state(), FenceState::Live);

    let journal = fixture.lease.journal_database().to_owned();
    let before = database_bytes(&journal);
    assert!(before[0].is_some());
    let transcript = || {
        (
            relay.recipient_requests(local),
            relay.recipient_closes(local),
            relay.all_publication_attempts(),
        )
    };
    let before_transcript = transcript();

    trip.trip();
    let mut cut = PhysicalCut::begin(owner, &fixture.lease).ok().unwrap();
    let Fixture {
        directory, lease, ..
    } = fixture;
    drop(lease);
    assert_held(directory.path());
    assert_eq!(binding.fence.state(), FenceState::Retired);
    assert!(runner.snapshot().cancel_requested);
    let report = cut.observe(deadline()).await;
    assert_eq!(
        report,
        Report {
            engine: Barrier::Complete,
            runners: Barrier::Complete,
            sdk: Barrier::Complete,
            forwarder: Barrier::Complete,
            raw: Barrier::Complete,
        }
    );
    assert!(runner.finished());
    until(|| !relay.runtime_socket_active(socket)).await;
    assert_eq!(relay.runtime_socket_end(socket), Some(SocketEnd::Raw));
    assert_eq!(transcript(), before_transcript);
    assert!(database_bytes(&journal) == before);
    assert_eq!(cut.observe(deadline()).await, report);
    let (parked, _) = cut.finish().ok().unwrap();
    assert_held(directory.path());

    // The readback is after all proof snapshots. Audit that the oracle would
    // observe the graceful branch rewriting this particular active record.
    let mut owner = ManuallyDrop::into_inner(parked.owner);
    owner.streams.stopping = false;
    owner.stream_begin_stop().unwrap();
    assert_eq!(owner.journal.stream_get(&key).unwrap().unwrap().revision, 2);
    assert!(database_bytes(&journal) != before);
    drop(owner);
    drop(ManuallyDrop::into_inner(parked.lease));
    assert!(ManuallyDrop::into_inner(parked.refused_engine).is_none());
    relay.stop().await;
    assert!(ProfileLease::acquire(directory.path(), PROFILE).is_ok());
}
