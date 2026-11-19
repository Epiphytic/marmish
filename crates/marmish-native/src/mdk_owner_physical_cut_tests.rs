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
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tokio::sync::oneshot;

const PROFILE: &str = "ordinary";

struct Fixture {
    directory: tempfile::TempDir,
    lease: Arc<ProfileLease>,
    material: IdentityMaterial,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
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
        admission: &Admission,
        relay_url: &str,
        execution_client: [u8; 32],
    ) {
        let config = CommandConfig::from_bytes(
            &serde_json::to_vec(&serde_json::json!({
                "version":1, "role":"server", "relays":[relay_url], "tor":"off",
                "cwd":"/", "limits":{}, "aliases":{}, "trusted_remotes":[],
                "command_policy":{"version":1},
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
        owner
            .journal
            .command_bind_engine(
                crate::command_identity::DERIVATION_VERSION,
                &identity.root_id,
                &identity.key_id,
            )
            .unwrap();
        let worker = self.lease.root().join("test-worker");
        fs::copy(std::env::current_exe().unwrap(), &worker).unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        let policy = config
            .engine_policy(self.lease.root().join("commands"), worker, account, &[])
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
            admission.clone(),
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
    fixture.install_engine(
        &mut owner,
        &Admission::new(&unrelated, || true),
        "wss://127.0.0.1:9/",
        peer,
    );
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
    let cut = PhysicalCut::begin(owner, &fixture.lease).ok().unwrap();
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
        forwarder: Barrier::PossiblyLive,
        ..report
    };
    assert_eq!(cut.observe(deadline()).await, expected);
    drop(release);
    assert_eq!(cut.observe(deadline()).await, expected);
    assert!(expected.requires_process_hold());
    let cut = cut.finish().err().unwrap();
    assert_eq!(cut.report(), expected);
    assert_held(directory.path());
    release_fixture(cut.parked);
    assert!(ProfileLease::acquire(directory.path(), PROFILE).is_ok());
}

#[tokio::test]
async fn previously_closed_slots_leave_ownership_unconfirmed() {
    let fixture = Fixture::new();
    let (trip, fatal) = Trip::new();
    let connector = direct(&fixture, fatal);
    let owner = fixture.open(connector.clone());
    connector.cancel();
    trip.trip();
    let mut cut = PhysicalCut::begin(owner, &fixture.lease).ok().unwrap();
    let report = cut.observe(deadline()).await;
    assert_eq!(report.raw, Barrier::PossiblyLive);
    assert_eq!(cut.observe(deadline()).await, report);
    assert!(report.requires_process_hold());
    let cut = cut.finish().err().unwrap();
    release_fixture(cut.parked);
}

#[tokio::test]
async fn joined_keeper_failure_and_lost_keeper_both_hold_without_child_proof() {
    let mut returned = Task::new(Some(tokio::spawn(async {
        Err::<(), _>(Error::Storage("engine_thread_failed"))
    })));
    let lost = tokio::spawn(std::future::pending::<Result<()>>());
    lost.abort();
    let mut lost = Task::new(Some(lost));

    returned.observe(deadline(), engine_barrier).await;
    lost.observe(deadline(), engine_barrier).await;
    assert!(returned.handle.is_none());
    assert!(lost.handle.is_none());
    assert_eq!(returned.status, Barrier::PossiblyLive);
    assert_eq!(lost.status, Barrier::PossiblyLive);
    let report = Report {
        engine: returned.status,
        runners: Barrier::Complete,
        sdk: Barrier::Complete,
        forwarder: Barrier::Complete,
        raw: Barrier::Complete,
    };
    assert!(report.requires_process_hold());
}

#[tokio::test]
async fn runner_proven_not_started_permits_failed_disposition() {
    let runner = RunnerHandle::not_started_for_test().unwrap();
    let mut status = Barrier::Pending;
    observe_runners(&[runner], &mut status, deadline()).await;
    assert_eq!(status, Barrier::Incomplete);
}

#[tokio::test]
async fn finished_uncertain_runner_preserves_the_cut_and_lease() {
    let fixture = Fixture::new();
    let (trip, fatal) = Trip::new();
    let mut owner = fixture.open(direct(&fixture, fatal));
    let (trigger_panic, await_panic) = std::sync::mpsc::channel();
    let runner = RunnerHandle::panic_before_child_for_test(await_panic).unwrap();
    let mut live = LiveStream::new();
    live.runner = Some(runner.clone());
    owner.streams.live.insert("caught-panic".into(), live);
    trip.trip();

    let mut cut = PhysicalCut::begin(owner, &fixture.lease).ok().unwrap();
    let Fixture {
        directory, lease, ..
    } = fixture;
    drop(lease);
    let before = cut
        .observe(Instant::now() + Duration::from_millis(35))
        .await;
    assert_eq!(before.runners, Barrier::Pending);
    assert!(before.requires_process_hold());
    assert!(!runner.finished());
    assert_held(directory.path());
    trigger_panic.send(()).unwrap();

    let expected = Report {
        runners: Barrier::PossiblyLive,
        engine: Barrier::Complete,
        sdk: Barrier::Complete,
        forwarder: Barrier::Complete,
        raw: Barrier::Complete,
    };
    assert_eq!(cut.observe(deadline()).await, expected);
    assert!(runner.finished());
    assert!(runner.snapshot().uncertain);
    assert!(expected.requires_process_hold());
    for _ in 0..2 {
        cut = *cut.finish().err().unwrap();
        assert_held(directory.path());
        assert_eq!(cut.observe(deadline()).await, expected);
    }

    // This panic was injected before spawning a child. Only the fixture,
    // which never displaced native dispatch, can dismantle the parked owner.
    release_fixture(cut.parked);
    assert!(ProfileLease::acquire(directory.path(), PROFILE).is_ok());
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
    relay.ordinary(local);
    let peer = Keys::generate().public_key().to_bytes();
    fixture.install_engine(
        &mut owner,
        &Admission::new(&trip, || true),
        relay.url.as_str(),
        peer,
    );
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
    let mdk = fixture.lease.mdk_database().to_owned();
    let before_mdk = database_bytes(&mdk);
    assert!(before_mdk[0].is_some());
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
    assert!(!runner.snapshot().uncertain);
    assert!(!report.requires_process_hold());
    until(|| !relay.runtime_socket_active(socket)).await;
    assert_eq!(relay.runtime_socket_end(socket), Some(SocketEnd::Raw));
    assert_eq!(transcript(), before_transcript);
    assert!(database_bytes(&journal) == before);
    assert!(database_bytes(&mdk) == before_mdk);
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

fn open_healthy(fixture: &Fixture, trip: &Arc<Trip>, admission: &Admission) -> Box<MdkOwner> {
    Box::new(
        MdkOwner::open_candidate(
            &fixture.lease,
            &fixture.material,
            direct(fixture, trip.observe()),
            vec![],
            admission.clone(),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn healthy_owned_stop_binds_exact_admission_and_closes_after_native_drain() {
    use crate::mdk_adapter::healthy_stop::HealthyStop;
    let fixture = Fixture::new();
    let (trip, _) = Trip::new();
    let trip = Arc::new(trip);
    let admission = Admission::new(&trip, || true);
    let owner = open_healthy(&fixture, &trip, &admission);
    let another_admission = Admission::new(&trip, || true);
    let owner = HealthyStop::begin(owner, &fixture.lease, &another_admission)
        .err()
        .expect("separate admission with the same latch must refuse");
    let mut stopping = HealthyStop::begin(owner, &fixture.lease, &admission)
        .ok()
        .expect("exact owner");
    let report = stopping.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert!(report.graceful_complete && !report.fatal);
    let calls = std::cell::Cell::new(0);
    assert!(
        stopping
            .complete(|| {
                calls.set(calls.get() + 1);
                fixture.lease.cleanup_runtime()
            })
            .is_ok()
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(Arc::strong_count(&fixture.lease), 1);
    // A stock process can acquire/reopen the same real native profile.
    drop(fixture.lease);
    let lease = ProfileLease::acquire(fixture.directory.path(), PROFILE).unwrap();
    let connector = RelayConnector::new(
        RelayPolicy::parse(
            &["wss://127.0.0.1:9/".into()],
            RouteMode::Off,
            TransportLimits::default(),
        )
        .unwrap(),
        ArtiDirs {
            state: lease.arti_state().into(),
            cache: lease.arti_cache().into(),
        },
    )
    .unwrap();
    let mut stock = MdkOwner::open(&lease, &fixture.material, connector, vec![]).unwrap();
    stock.shutdown(deadline()).await.unwrap();
}

#[tokio::test]
async fn healthy_owned_stop_keeps_forwarder_and_refuses_effects_after_cut() {
    use crate::mdk_adapter::healthy_stop::HealthyStop;
    let fixture = Fixture::new();
    let (trip, _) = Trip::new();
    let trip = Arc::new(trip);
    let admission = Admission::new(&trip, || true);
    let mut owner = open_healthy(&fixture, &trip, &admission);
    let (release, hold) = oneshot::channel();
    owner.forwarder = Some(tokio::spawn(async { hold.await.unwrap() }));
    let mut stopping = HealthyStop::begin(owner, &fixture.lease, &admission)
        .ok()
        .unwrap();
    let report = stopping
        .observe(Instant::now() + Duration::from_millis(10))
        .await;
    assert_eq!(report.physical.forwarder, Barrier::Pending);
    assert!(report.physical.requires_process_hold());
    assert!(!report.graceful_complete && !report.fatal);
    trip.notify();
    let report = stopping
        .observe(Instant::now() + Duration::from_millis(10))
        .await;
    assert_eq!(report.physical.forwarder, Barrier::Pending);
    assert!(!report.graceful_complete && report.fatal);
    release.send(()).unwrap();
    let report = stopping.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert!(!report.graceful_complete && report.fatal);
    let retained = stopping.complete(|| panic!("fatal stop cannot clean runtime"));
    assert!(retained.is_err());
    assert!(Arc::strong_count(&fixture.lease) > 1);
}

#[tokio::test]
async fn healthy_owned_stop_reports_cut_during_final_cleanup_and_retains_lease() {
    use crate::mdk_adapter::healthy_stop::HealthyStop;
    let fixture = Fixture::new();
    let (trip, _) = Trip::new();
    let trip = Arc::new(trip);
    let admission = Admission::new(&trip, || true);
    let owner = open_healthy(&fixture, &trip, &admission);
    let mut stopping = HealthyStop::begin(owner, &fixture.lease, &admission)
        .ok()
        .unwrap();
    assert!(stopping.observe(deadline()).await.graceful_complete);
    let Err(mut retained) = stopping.complete(|| {
        trip.notify();
        Ok(())
    }) else {
        panic!("post-cleanup admission must refuse success");
    };
    assert!(Arc::strong_count(&fixture.lease) > 1);
    let report = retained.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert!(!report.graceful_complete && report.fatal);
}

async fn stop_started_engine(quiesce: bool) {
    use crate::{command_engine::CleanupState, mdk_adapter::healthy_stop::HealthyStop};
    let fixture = Fixture::new();
    let (trip, _) = Trip::new();
    let trip = Arc::new(trip);
    let admission = Admission::new(&trip, || true);
    let mut owner = open_healthy(&fixture, &trip, &admission);
    let client = Keys::generate().public_key().to_bytes();
    fixture.install_engine(&mut owner, &admission, "wss://127.0.0.1:9/", client);
    if quiesce {
        let expires = deadline();
        loop {
            match owner.command_begin_cleanup() {
                Ok(_) => break,
                Err(Error::Storage("command_cleanup_busy")) => {
                    assert!(
                        Instant::now() < expires,
                        "Command worker did not become ready"
                    );
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                Err(error) => panic!("unexpected Command cleanup preparation error: {error:?}"),
            }
        }
        while owner.command_poll_cleanup().unwrap().0 != CleanupState::Quiesced {
            assert!(Instant::now() < expires, "Command cleanup did not quiesce");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    let mut stopping = HealthyStop::begin(owner, &fixture.lease, &admission)
        .ok()
        .unwrap();
    let report = stopping.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert_eq!(report.graceful_complete, quiesce);
    assert!(!report.fatal);
    let result = stopping.complete(|| {
        assert!(
            quiesce,
            "Command worker join alone cannot authorize cleanup"
        );
        fixture.lease.cleanup_runtime()
    });
    assert_eq!(result.is_ok(), quiesce);
}

#[tokio::test]
async fn healthy_owned_stop_joins_real_started_engine_without_authorizing_cleanup() {
    stop_started_engine(false).await;
}

#[tokio::test]
async fn healthy_owned_stop_allows_cleanup_after_real_engine_quiescence() {
    stop_started_engine(true).await;
}

#[test]
fn healthy_owned_stop_refuses_invalid_owner_without_effects() {
    use crate::mdk_adapter::healthy_stop::HealthyStop;
    let fixture = Fixture::new();
    let different = Fixture::new();
    let trip = Arc::new(Trip::new().0);
    let admission = Admission::new(&trip, || true);
    let owner = open_healthy(&fixture, &trip, &admission);
    let address = &*owner as *const MdkOwner;
    let refused = |owner, lease| {
        let owner = HealthyStop::begin(owner, lease, &admission).err().unwrap();
        assert_eq!(address, &*owner as *const MdkOwner);
        assert!(!owner.publication.blocked());
        owner
    };
    let owner = refused(owner, &fixture.lease); // No current runtime.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let mut owner = refused(owner, &different.lease);
    owner.streams.stopping = true;
    let mut owner = refused(owner, &fixture.lease);
    assert!(owner.streams.stopping);
    owner.streams.stopping = false;
    let client = Keys::generate().public_key().to_bytes();
    fixture.install_engine(
        &mut owner,
        &Admission::new(&trip, || true),
        "wss://127.0.0.1:9/",
        client,
    );
    let mut owner = refused(owner, &fixture.lease);
    let mut worker = owner.take_engine_for_stop().unwrap();
    assert!(
        worker
            .shutdown(std::time::Instant::now() + Duration::from_secs(5))
            .unwrap()
            .complete
    );
    owner.startup_custody.take().unwrap().close().unwrap();
}

#[tokio::test]
async fn healthy_owned_stop_rechecks_sticky_cut_during_graceful_preparation() {
    use crate::mdk_adapter::healthy_stop::HealthyStop;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let fixture = Fixture::new();
    let trip = Arc::new(Trip::new().0);
    let checks = Arc::new(AtomicUsize::new(0));
    let admission = Admission::new(&trip, {
        let trip = trip.clone();
        let checks = checks.clone();
        move || {
            if checks.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                count.checked_sub(1)
            }) == Ok(1)
            {
                trip.mark_cut();
            }
            true
        }
    });
    let owner = open_healthy(&fixture, &trip, &admission);
    let mut stopping = HealthyStop::begin(owner, &fixture.lease, &admission)
        .ok()
        .unwrap();
    checks.store(2, Ordering::Release);
    let report = stopping.observe(deadline()).await;
    assert_eq!(checks.load(Ordering::Acquire), 0);
    assert!(report.physical.all_complete());
    assert!(report.fatal && !report.graceful_complete);
    assert!(
        stopping
            .complete(|| panic!("cut forbids cleanup effects"))
            .is_err()
    );
}

async fn wait_for_idle_engine(owner: &mut MdkOwner) {
    until(|| {
        owner
            .commands
            .as_mut()
            .unwrap()
            .engine_mut()
            .unwrap()
            .readiness()
            == Readiness::Ready
    })
    .await;
}

#[tokio::test]
async fn registered_idle_prelude_quiesces_real_engine_before_runtime_cleanup() {
    use crate::mdk_adapter::healthy_stop::RegisteredStop;
    let fixture = Fixture::new();
    let trip = Arc::new(Trip::new().0);
    let admission = Admission::new(&trip, || true);
    let mut owner = open_healthy(&fixture, &trip, &admission);
    let client = Keys::generate().public_key().to_bytes();
    fixture.install_engine(&mut owner, &admission, "wss://127.0.0.1:9/", client);
    wait_for_idle_engine(&mut owner).await;
    let binding = owner
        .terminal_authority
        .register(AuthenticatedCommandContext::test_fixture(
            fixture.material.public_key().to_bytes(),
            client,
            vec![3; 32],
            [2; 16],
            CommandDirection::Server,
        ))
        .unwrap();
    let began = Instant::now();
    let mut stop = RegisteredStop::begin(owner, &fixture.lease, &admission, began, true)
        .ok()
        .expect("registered owner");
    assert_eq!(binding.fence.state(), FenceState::Retired);
    let report = stop.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert!(report.graceful_complete && !report.fatal);
    assert!(stop.complete(|| fixture.lease.cleanup_runtime()).is_ok());
    assert_eq!(Arc::strong_count(&fixture.lease), 1);
    eprintln!(
        "idle registered prelude and physical stop: {:?}, retained_rows=0",
        began.elapsed()
    );
}

#[tokio::test]
async fn registered_idle_prelude_expired_at_entry_allocates_no_fence_and_skips_cleanup() {
    use crate::mdk_adapter::healthy_stop::RegisteredStop;
    let fixture = Fixture::new();
    let trip = Arc::new(Trip::new().0);
    let admission = Admission::new(&trip, || true);
    let mut owner = open_healthy(&fixture, &trip, &admission);
    let client = Keys::generate().public_key().to_bytes();
    fixture.install_engine(&mut owner, &admission, "wss://127.0.0.1:9/", client);
    wait_for_idle_engine(&mut owner).await;
    let before = database_bytes(fixture.lease.journal_database());
    let mut stop = RegisteredStop::begin(
        owner,
        &fixture.lease,
        &admission,
        Instant::now() - Duration::from_secs(7),
        true,
    )
    .ok()
    .expect("registered owner");
    let report = stop.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert!(!report.graceful_complete && !report.fatal);
    assert!(
        stop.complete(|| panic!("expired prelude cannot clean runtime"))
            .is_err()
    );
    assert_eq!(before, database_bytes(fixture.lease.journal_database()));
    assert_held(fixture.directory.path());
}

#[tokio::test]
async fn registered_idle_prelude_cancelled_observer_retains_owner_during_actual_cut() {
    use crate::mdk_adapter::healthy_stop::RegisteredStop;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let fixture = Fixture::new();
    let trip = Arc::new(Trip::new().0);
    let armed = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicBool::new(false));
    let (release, wait) = mpsc::channel();
    let wait = Mutex::new(wait);
    let owner_thread = std::thread::current().id();
    let admission = Admission::new(&trip, {
        let armed = armed.clone();
        let entered = entered.clone();
        move || {
            if std::thread::current().id() != owner_thread && armed.swap(false, Ordering::AcqRel) {
                entered.store(true, Ordering::Release);
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            }
            true
        }
    });
    let mut owner = open_healthy(&fixture, &trip, &admission);
    let client = Keys::generate().public_key().to_bytes();
    fixture.install_engine(&mut owner, &admission, "wss://127.0.0.1:9/", client);
    wait_for_idle_engine(&mut owner).await;
    armed.store(true, Ordering::Release);
    let before = database_bytes(fixture.lease.journal_database());
    let mut stop = RegisteredStop::begin(owner, &fixture.lease, &admission, Instant::now(), true)
        .ok()
        .expect("registered owner");
    tokio::select! {
        _ = until(|| entered.load(Ordering::Acquire)) => {}
        _ = stop.observe(deadline()) => panic!("owned turn returned before release"),
    }
    trip.notify();
    let interrupted = stop
        .observe(Instant::now() + Duration::from_millis(15))
        .await;
    assert_eq!(interrupted.physical.engine, Barrier::PossiblyLive);
    assert_eq!(interrupted.physical.sdk, Barrier::Pending);
    assert!(interrupted.physical.requires_process_hold());
    assert!(interrupted.fatal && !interrupted.graceful_complete);
    assert_held(fixture.directory.path());
    release.send(()).unwrap();
    let report = stop.observe(deadline()).await;
    assert!(report.physical.all_complete());
    assert!(report.fatal && !report.graceful_complete);
    assert!(
        stop.complete(|| panic!("fatal prelude cannot clean runtime"))
            .is_err()
    );
    assert_eq!(before, database_bytes(fixture.lease.journal_database()));
}

#[tokio::test]
async fn registered_stop_without_engine_bypasses_prelude_budget() {
    use crate::mdk_adapter::healthy_stop::RegisteredStop;
    let fixture = Fixture::new();
    let trip = Arc::new(Trip::new().0);
    let admission = Admission::new(&trip, || true);
    let owner = open_healthy(&fixture, &trip, &admission);
    let mut stop = RegisteredStop::begin(
        owner,
        &fixture.lease,
        &admission,
        Instant::now() - Duration::from_secs(7),
        true,
    )
    .ok()
    .expect("registered owner");
    assert!(stop.observe(deadline()).await.graceful_complete);
    assert!(stop.complete(|| fixture.lease.cleanup_runtime()).is_ok());
}
