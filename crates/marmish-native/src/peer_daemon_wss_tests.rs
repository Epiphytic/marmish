//! Opt-in custody qualification through the daemon's same-UID Unix socket.

use super::{
    daemon_peer::{Admission, Bootstrap},
    *,
};
use crate::{
    command_local::LocalResult,
    daemon::{self, Config, PeerFixture},
    identity::{BackendChoice, socket_path_for},
    ipc::{self, Request, peer},
    mdk_adapter::{
        CommandPeer, peer_staged_proof_tests::peer_enrollment_welcome,
        terminal_relay_fixture::Relay,
    },
    transport::{RelayPolicy, RouteMode, TransportLimits},
};
use cgka_traits::app_components::NostrRoutingV1;
use consent_checkpoint::{
    Mode,
    provisioning::prepared_consent::custody::CustodyState,
    wire::{Codec, Delivery, Flight, MAX_OUTER_BYTES},
};
use nostr::key::PublicKey;
use prepared_consent::PreparedConsentOwner;
use serde_json::json;
use std::{
    fs::{OpenOptions, Permissions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};

const PROFILE: &str = "recipient";
const STEP: Duration = Duration::from_secs(5);
const WAIT: Duration = Duration::from_secs(35);

fn private() -> TempDir {
    tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap()
}

fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

fn layout() -> PathBuf {
    let libtest = std::env::current_exe().unwrap();
    let bin = libtest.parent().unwrap().parent().unwrap().join("marmishd");
    crate::native_layout::resolve(&bin).expect("build marmish-native --bins before qualification");
    bin
}

fn material(lease: &mut ProfileLease, daemon: &Path) -> IdentityMaterial {
    lease
        .prepare(
            BackendChoice::File,
            &crate::native_layout::resolve(daemon).unwrap().helper,
            std::time::Instant::now() + STEP,
        )
        .unwrap()
}

struct Running {
    stop: Option<oneshot::Sender<()>>,
    task: JoinHandle<crate::Result<()>>,
}

impl Running {
    async fn stop(mut self) -> Duration {
        let start = Instant::now();
        self.stop.take().unwrap().send(()).expect("running daemon");
        timeout(WAIT, &mut self.task)
            .await
            .expect("bounded owned daemon stop")
            .expect("daemon task joined")
            .expect("clean owned daemon stop");
        start.elapsed()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

struct Round {
    ordinary: Relay,
    relay: Relay,
    command: CommandPeer,
    signer: PublicKey,
    root: TempDir,
    founder: Arc<Keys>,
    founder_root: TempDir,
    wall: Arc<AtomicU64>,
    daemon: PathBuf,
}

impl Round {
    async fn initialize() -> Self {
        let daemon = layout();
        let root = private();
        let founder = Arc::new(Keys::generate());
        let founder_root = private();
        let command = CommandPeer::new(root.path(), "command");
        let ordinary = Relay::start().await;
        let mut setup = ProfileLease::acquire(root.path(), PROFILE).unwrap();
        let signer = material(&mut setup, &daemon).public_key();
        let config = json!({
            "version": 1, "role": "client", "relays": [ordinary.url.as_str()],
            "tor": "off", "cwd": "/", "limits": {}, "aliases": {},
            "trusted_remotes": [command.public().to_hex()], "execution_clients": [],
            "command_policy": {"version": 1, "defaults": {"allow_exec": []}}
        });
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(setup.root().join("config.json"))
            .unwrap()
            .write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        drop(setup);
        ordinary.ordinary(signer);
        ordinary.ordinary(command.public());
        let mut relay = Relay::start_peer([signer, founder.public_key()]).await;
        assert!(ordinary.peer_mode().is_none());
        assert!(
            QualifiedOrigin::for_daemon_peer_fixture(
                relay.peer_mode().unwrap(),
                founder_root.path(),
                signer,
                command.public(),
            )
            .is_err()
        );
        let bound = std::mem::replace(&mut relay.url, ordinary.url.clone());
        assert!(
            QualifiedOrigin::for_daemon_peer_fixture(
                relay.peer_mode().unwrap(),
                founder_root.path(),
                signer,
                founder.public_key(),
            )
            .is_err()
        );
        relay.url = bound;
        let wall = Arc::new(AtomicU64::new(Wall::System.sample().unwrap().as_secs()));
        Self {
            ordinary,
            relay,
            command,
            signer,
            root,
            founder,
            founder_root,
            wall,
            daemon,
        }
    }

    fn socket(&self) -> PathBuf {
        socket_path_for(self.root.path(), PROFILE).unwrap()
    }

    async fn start(&self, admission: Admission, uncertain: bool) -> Running {
        let profile_root = self.root.path().join(PROFILE);
        let bootstrap = Bootstrap::for_fixture(
            self.relay.peer_mode().unwrap(),
            &profile_root,
            self.signer,
            self.founder.public_key(),
            admission,
            Some(self.wall.clone()),
        )
        .unwrap();
        let bootstrap = if uncertain {
            bootstrap.with_uncertain_approval_once()
        } else {
            bootstrap
        };
        let (stop, stop_receiver) = oneshot::channel();
        let config = Config {
            state_dir: self.root.path().to_owned(),
            identity: PROFILE.into(),
            backend: BackendChoice::File,
            helper: crate::native_layout::resolve(&self.daemon).unwrap().helper,
            relay_policy: RelayPolicy::parse(
                &[self.ordinary.url.to_string()],
                RouteMode::Off,
                TransportLimits::default(),
            )
            .unwrap(),
            allowed_peers: vec![],
            retention: Duration::from_secs(86_400),
            max_commands: None,
            max_uptime: None,
        };
        let fixture = PeerFixture {
            bootstrap: Some(bootstrap),
            connector: Some(self.ordinary.connector(&profile_root, true, None)),
            stop: stop_receiver,
            layout: self.daemon.clone(),
        };
        let running = Running {
            stop: Some(stop),
            task: tokio::task::spawn_local(daemon::run_peer_fixture(config, None, fixture)),
        };
        timeout(WAIT, async {
            loop {
                assert!(
                    !running.task.is_finished(),
                    "daemon exited before ordinary readiness"
                );
                let Ok(socket) = socket_path_for(self.root.path(), PROFILE) else {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    continue;
                };
                if active(&socket).await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("ordinary relay ready on same daemon socket");
        assert!(matches!(
            ProfileLease::acquire(self.root.path(), PROFILE),
            Err(crate::Error::Identity("profile_in_use"))
        ));
        running
    }

    async fn send_delivery(&self) -> (Selector, Delivery) {
        let deadline = || Instant::now() + WAIT;
        let origin = self.relay.url.to_string();
        let qualified = QualifiedOrigin::for_daemon_peer_fixture(
            self.relay.peer_mode().unwrap(),
            self.founder_root.path(),
            self.signer,
            self.founder.public_key(),
        )
        .unwrap();
        let mut founder_connection = Owner::open(
            &qualified.connector,
            &origin,
            self.founder.public_key(),
            &self.founder,
            deadline(),
        )
        .await
        .unwrap();
        let wall = self.wall.load(Ordering::Relaxed);
        let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
        let routing = NostrRoutingV1::new(random(), vec![origin]).unwrap();
        let offer = codec
            .prepare_offer(&self.founder, self.signer, random(), &routing, wall)
            .unwrap();
        let selector = Selector::from(&offer.flight);
        assert!(
            founder_connection
                .publish(&offer, offer.flight.flight(), deadline())
                .await
                .unwrap()
        );
        let receipt = founder_connection
            .wait_live(deadline())
            .await
            .unwrap()
            .expect("actual recipient Consent carrier");
        let consent = codec
            .open_consent(
                &self.founder,
                receipt.arrival().body.get().as_bytes(),
                &offer.flight,
                wall,
            )
            .unwrap();
        assert_eq!(consent.flight().author, self.signer);
        let welcome = peer_enrollment_welcome(
            self.founder_root.path(),
            self.founder.clone(),
            self.signer,
            consent.offer().routing(),
            consent.offer().room_id(),
            consent.key_package().clone(),
        )
        .await;
        let delivery = codec
            .prepare_delivery(&self.founder, &consent, &welcome, wall, wall + 360)
            .unwrap();
        assert!(
            founder_connection
                .publish(&delivery, delivery.flight.flight(), deadline())
                .await
                .unwrap()
        );
        founder_connection.stop(deadline()).await.unwrap();
        self.assert_relays(&[
            offer.flight.flight(),
            consent.flight(),
            delivery.flight.flight(),
        ]);
        (selector, delivery.flight)
    }

    fn assert_relays(&self, expected: &[Flight]) {
        assert!(self.relay.authenticated(self.signer));
        assert!(self.relay.authenticated(self.founder.public_key()));
        assert!(self.relay.peer_only_publications());
        assert_eq!(self.ordinary.publication_attempts(7435), 0);
        let writes = self.relay.peer_transcript();
        assert_eq!(writes.len(), expected.len());
        for (entry, flight) in writes.iter().zip(expected) {
            assert_eq!(entry.sender, flight.author);
            assert_eq!(entry.recipient, flight.recipient);
            assert_eq!(entry.event, flight.carrier_id);
        }
    }

    fn closed_stage(
        &self,
        selection: &peer::Selection,
        delivery: &Delivery,
        expected: CustodyState,
        mode: Mode,
    ) -> [u8; 32] {
        let mut lease =
            ProfileLease::acquire(self.root.path(), PROFILE).expect("daemon released sole lease");
        let material = material(&mut lease, &self.daemon);
        let route = Route::configured(self.relay.url.as_str()).unwrap();
        let mut owner = PreparedConsentOwner::open_custody(
            &lease,
            &material,
            crate::command_config::Role::Client,
            Policy {
                mode,
                ..Policy::default()
            },
            &route,
            Wall::Fixture(self.wall.clone()),
        )
        .unwrap();
        let wall = self.wall.load(Ordering::Relaxed);
        let selector = Selector {
            server: selection.server,
            id: selection.offer_id,
            revision: selection.revision,
        };
        let held = owner
            .state
            .checkpoint
            .read_custody(&owner.state.source, selector, selection.generation, wall)
            .unwrap()
            .expect("durable selected custody row");
        assert_eq!(held.state(), expected);
        let joined = owner
            .inspect_joined(held.stage(), delivery, held.package_digest())
            .expect("exact native closed-room readback and consumed package");
        assert!(held.matches_pending_readback(&joined));
        let (binding, witness) = held.stage();
        Paths::new(&lease, &material)
            .unwrap()
            .reopen_provisional(binding, witness)
            .unwrap()
            .digest()
            .unwrap()
    }
}

async fn active(socket: &Path) -> bool {
    matches!(
        timeout(STEP, ipc::request(socket, &Request::Status {})).await,
        Ok(Ok(response)) if response.ok && response.result.as_ref().is_some_and(|status| status["state"] == "active")
    )
}

async fn assert_no_command(socket: &Path) {
    let status = ipc::request(socket, &Request::Status {})
        .await
        .unwrap()
        .result
        .unwrap();
    assert_eq!(status["state"], "active");
    assert_eq!(status["conversations"], 0);
    assert_eq!(status["received"], 0);
    let LocalResult::Invocations { entries, .. } = ipc::command_request(
        socket,
        &Request::CommandInvocations {
            after: None,
            limit: 1,
        },
    )
    .await
    .unwrap() else {
        panic!("ordinary invocation inventory available")
    };
    assert!(entries.is_empty());
}

async fn entry(socket: &Path, selector: Selector, target: peer::State) -> peer::Entry {
    timeout(WAIT, async {
        loop {
            if let Ok(peer::Reply::List { entries }) =
                peer::client_request(socket, peer::Request::List {}).await
                && let Some(entry) = entries.into_iter().find(|entry| {
                    entry.selection.server == selector.server
                        && entry.selection.offer_id == selector.id
                        && entry.selection.revision == selector.revision
                        && entry.state == target
                })
            {
                break entry;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("bounded exact durable inventory")
}

async fn show(socket: &Path, selection: peer::Selection, state: peer::State) -> peer::Entry {
    let peer::Reply::Show { entry } =
        peer::client_request(socket, peer::Request::Show { selection })
            .await
            .unwrap()
    else {
        panic!("typed Peer status")
    };
    assert_eq!(entry.selection, selection);
    assert_eq!(entry.state, state);
    entry
}

async fn manual() {
    let round = Round::initialize().await;
    let daemon = round.start(Admission::Manual, false).await;
    let socket = round.socket();
    let began = Instant::now();
    let (selector, delivery) = round.send_delivery().await;
    let pending = entry(&socket, selector, peer::State::Pending).await;
    let offer_to_pending = began.elapsed();
    show(&socket, pending.selection, peer::State::Pending).await;
    assert_no_command(&socket).await;
    let stop = daemon.stop().await;
    assert!(!socket.exists());
    let joined = round.closed_stage(
        &pending.selection,
        &delivery,
        CustodyState::Pending,
        Mode::Manual,
    );

    let daemon = round.start(Admission::Automatic, true).await;
    show(&socket, pending.selection, peer::State::Pending).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    show(&socket, pending.selection, peer::State::Pending).await;
    let began = Instant::now();
    let request = || peer::Request::Approve {
        selection: pending.selection,
    };
    assert!(matches!(
        peer::client_request(&socket, request()).await,
        Err(peer::Failure::Uncertain)
    ));
    let decision = began.elapsed();
    let committed = show(&socket, pending.selection, peer::State::Activating).await;
    assert!(committed.activating_at.is_some());
    assert_no_command(&socket).await;
    daemon.stop().await;
    assert!(!socket.exists());
    assert_eq!(
        round.closed_stage(
            &pending.selection,
            &delivery,
            CustodyState::Activating,
            Mode::Automatic
        ),
        joined
    );

    let daemon = round.start(Admission::Automatic, false).await;
    round.wall.fetch_add(1, Ordering::Relaxed);
    let began = Instant::now();
    let peer::Reply::Approve { entry } = peer::client_request(&socket, request()).await.unwrap()
    else {
        panic!("exact committed reply after restart")
    };
    assert_eq!(entry.selection, pending.selection);
    assert_eq!(entry.state, peer::State::Activating);
    assert_eq!(entry.activating_at, committed.activating_at);
    let retry = began.elapsed();
    round.wall.store(pending.expires_at, Ordering::Relaxed);
    assert_eq!(
        peer::client_request(
            &socket,
            peer::Request::Show {
                selection: pending.selection
            }
        )
        .await,
        Err(peer::Failure::Expired),
    );
    assert_eq!(
        peer::client_request(&socket, request()).await,
        Err(peer::Failure::Expired),
    );
    daemon.stop().await;
    assert!(!socket.exists());
    assert!(ProfileLease::acquire(round.root.path(), PROFILE).is_ok());
    assert!(round.relay.peer_only_publications());
    assert_eq!(round.relay.peer_transcript().len(), 3);
    assert_eq!(round.ordinary.publication_attempts(7435), 0);
    eprintln!(
        "peer_daemon_wss manual offer_to_pending_ms={} decision_ms={} retry_ms={} first_stop_ms={}",
        offer_to_pending.as_millis(),
        decision.as_millis(),
        retry.as_millis(),
        stop.as_millis()
    );
}

async fn automatic() {
    let round = Round::initialize().await;
    let daemon = round.start(Admission::Automatic, false).await;
    let socket = round.socket();
    let began = Instant::now();
    let (selector, delivery) = round.send_delivery().await;
    let admitted = entry(&socket, selector, peer::State::Activating).await;
    let offer_to_activating = began.elapsed();
    show(&socket, admitted.selection, peer::State::Activating).await;
    assert_no_command(&socket).await;
    let stop = daemon.stop().await;
    assert!(!socket.exists());
    round.closed_stage(
        &admitted.selection,
        &delivery,
        CustodyState::Activating,
        Mode::Automatic,
    );
    assert!(round.relay.peer_only_publications());
    assert_eq!(round.relay.peer_transcript().len(), 3);
    eprintln!(
        "peer_daemon_wss automatic offer_to_activating_ms={} stop_ms={}",
        offer_to_activating.as_millis(),
        stop.as_millis()
    );
}

#[tokio::test]
#[ignore = "requires same-build native bins and two independent private TLS relays"]
async fn peer_daemon_wss_manual_and_automatic_custody() {
    tokio::task::LocalSet::new()
        .run_until(timeout(Duration::from_secs(240), async {
            manual().await;
            automatic().await;
        }))
        .await
        .expect("bounded same-UID/native WSS daemon qualification");
}

async fn ordinary_command(socket: &Path, remote: PublicKey, relay: &Relay, cat: &Path) {
    use crate::{
        command_client::CommandHandle,
        command_local::Cursors,
        command_wire::{self as wire, ResponseResult, Submit},
    };
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    const MESSAGE: &[u8] = b"\x00two words; * % \" \' \\ \xff\r\n";

    async fn request(socket: &Path, request: &Request) -> LocalResult {
        let result = ipc::command_request(socket, request).await.unwrap();
        if let LocalResult::Failure { category, .. } = &result {
            panic!("ordinary Command IPC failed: {category}");
        }
        result
    }

    async fn answered(
        socket: &Path,
        handle: &CommandHandle,
        request_id: &str,
        mut result: LocalResult,
    ) -> (std::result::Result<ResponseResult, wire::WireError>, String) {
        loop {
            match result {
                LocalResult::Answer {
                    handle: found,
                    request_id: id,
                    response_base64,
                    response_sha256,
                } => {
                    assert!(found == *handle);
                    assert_eq!(id, request_id);
                    let bytes = STANDARD.decode(response_base64).unwrap();
                    assert_eq!(response_sha256, hex::encode(wire::request_digest(&bytes)));
                    let response = wire::decode_response(&bytes).unwrap();
                    assert_eq!(hex::encode(response.id), request_id);
                    assert_eq!(response.binding.session, handle.session());
                    assert_eq!(response.binding.server, handle.remote());
                    return (response.result, response_sha256);
                }
                LocalResult::RemotePending {
                    handle: found,
                    request_id: id,
                } => {
                    assert!(found == *handle && id == request_id);
                }
                LocalResult::LocalBusy => {}
                _ => panic!("unexpected ordinary Command response"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
            result = request(
                socket,
                &Request::CommandResponse {
                    handle: handle.clone(),
                    request_id: request_id.to_owned(),
                },
            )
            .await;
        }
    }

    async fn consume(
        socket: &Path,
        handle: &CommandHandle,
        id: String,
        digest: String,
        cursors: Option<Cursors>,
    ) {
        assert!(matches!(
            request(
                socket,
                &Request::CommandConsume {
                    handle: handle.clone(),
                    request_id: id,
                    response_sha256: digest,
                    expected_cursors: cursors,
                }
            )
            .await,
            LocalResult::Consumed { .. }
        ));
    }

    let relays = vec![relay.url.to_string()];
    let remote = remote.to_hex();
    let prepare = Request::CommandPrepare {
        remote: remote.clone(),
        relays: relays.clone(),
        fresh: None,
    };
    let session = loop {
        match request(socket, &prepare).await {
            LocalResult::Ready { session, .. } => break session,
            LocalResult::LocalBusy => tokio::time::sleep(Duration::from_millis(50)).await,
            _ => panic!("expected ready Command room"),
        }
    };
    let fresh = || crate::foundation::fresh_id(Wall::System.sample().unwrap().as_secs()).unwrap();
    let submit_id = fresh();
    let submit = Request::CommandSubmit {
        remote,
        relays,
        session,
        request_id: submit_id.clone(),
        submit: Submit {
            invocation: hex::decode(fresh()).unwrap().try_into().unwrap(),
            argv: vec![cat.as_os_str().as_encoded_bytes().to_vec()],
            stdin: MESSAGE.to_vec(),
        },
    };
    let LocalResult::LocalQueued { handle } = request(socket, &submit).await else {
        panic!("expected queued Command");
    };
    let response = request(
        socket,
        &Request::CommandResponse {
            handle: handle.clone(),
            request_id: submit_id.clone(),
        },
    )
    .await;
    let (reply, digest) = answered(socket, &handle, &submit_id, response).await;
    assert!(matches!(reply, Ok(ResponseResult::Submit { .. })));
    consume(socket, &handle, submit_id, digest, None).await;

    let mut stdout = Vec::new();
    let mut cursors = Cursors {
        stdout: 0,
        stderr: 0,
    };
    loop {
        let poll_id = fresh();
        let poll = Request::CommandPoll {
            handle: handle.clone(),
            request_id: poll_id.clone(),
            stdout_cursor: cursors.stdout,
            stderr_cursor: cursors.stderr,
            max_bytes: 1024,
        };
        let initial = loop {
            match ipc::command_request(socket, &poll).await.unwrap() {
                LocalResult::Failure { category, .. } if category == "command_poll_busy" => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                LocalResult::Failure { category, .. } => {
                    panic!("ordinary Poll IPC failed: {category}")
                }
                other => break other,
            }
        };
        let (reply, digest) = answered(socket, &handle, &poll_id, initial).await;
        let (status, out, err) = match reply {
            Ok(ResponseResult::Poll {
                status,
                stdout,
                stderr,
            }) => (status, stdout, stderr),
            Err(error) if error.code == wire::ErrorCode::Busy => {
                consume(socket, &handle, poll_id, digest, Some(cursors)).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            Err(error) => panic!("ordinary Poll failed: {}", error.diagnostic_category()),
            _ => panic!("expected ordinary Command Poll"),
        };
        consume(socket, &handle, poll_id, digest, Some(cursors)).await;
        assert!(err.bytes.is_empty());
        assert_eq!(err.next_cursor, 0);
        stdout.extend_from_slice(&out.bytes);
        assert!(stdout.len() <= MESSAGE.len());
        cursors.stdout = out.next_cursor;
        if let Some(outcome) = status
            .outcome
            .filter(|_| status.stdout.closed && status.stderr.closed)
        {
            assert!(outcome.kind == wire::OutcomeKind::Code);
            assert_eq!(outcome.value, Some(0));
            assert_eq!(stdout, MESSAGE);
            break;
        }
    }

    let LocalResult::Invocations { entries, .. } = request(
        socket,
        &Request::CommandInvocations {
            after: None,
            limit: 2,
        },
    )
    .await
    else {
        panic!("ordinary invocation inventory");
    };
    assert_eq!(entries.len(), 1);
    assert!(entries[0].handle == handle);
    assert!(entries[0].authority_valid);
}

async fn outage() {
    let mut round = Round::initialize().await;
    let daemon = round.start(Admission::Manual, false).await;
    let socket = round.socket();
    let (selector, _) = round.send_delivery().await;
    let pending = entry(&socket, selector, peer::State::Pending).await;
    show(&socket, pending.selection, peer::State::Pending).await;
    round.relay.stop().await;
    assert_no_command(&socket).await;
    let began = Instant::now();
    let peer::Reply::Approve { entry } = peer::client_request(
        &socket,
        peer::Request::Approve {
            selection: pending.selection,
        },
    )
    .await
    .unwrap() else {
        panic!("local approval while only the Peer WSS transport is down");
    };
    assert_eq!(entry.state, peer::State::Activating);
    let decision = began.elapsed();

    let cat = std::fs::canonicalize("/bin/cat").unwrap();
    let mut server = round
        .command
        .open_fixture_server(&round.ordinary, round.signer, &cat)
        .await;
    let remote = round.command.public();
    let began = Instant::now();
    let work = ordinary_command(&socket, remote, &round.ordinary, &cat);
    tokio::pin!(work);
    timeout(Duration::from_secs(55), async {
        loop {
            tokio::select! {
                () = &mut work => break,
                result = server.tick(Instant::now() + STEP) => result.unwrap(),
            }
        }
    })
    .await
    .expect("ordinary Command must finish despite unavailable Peer WSS");
    let command = began.elapsed();
    assert!(round.ordinary.publication_attempts(445) > 0);
    assert_eq!(round.ordinary.publication_attempts(7435), 0);
    assert!(round.relay.peer_only_publications());
    assert_eq!(round.relay.peer_transcript().len(), 3);
    round.command.stop_fixture_server(server).await;
    let stop = daemon.stop().await;
    assert!(!socket.exists());
    assert!(ProfileLease::acquire(round.root.path(), PROFILE).is_ok());
    eprintln!(
        "peer_daemon_wss outage decision_ms={} command_ms={} stop_ms={}",
        decision.as_millis(),
        command.as_millis(),
        stop.as_millis()
    );
}

#[tokio::test]
#[ignore = "requires same-build native bins and two independent private TLS relays"]
async fn peer_daemon_wss_peer_outage_preserves_ordinary_commands() {
    tokio::task::LocalSet::new()
        .run_until(timeout(Duration::from_secs(160), outage()))
        .await
        .expect("bounded native Command during Peer-only outage");
}
