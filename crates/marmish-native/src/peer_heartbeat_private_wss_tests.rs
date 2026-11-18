//! Prove native publication provenance before any status leaves the process.

use super::*;
use crate::{
    peer_heartbeat_send_fences::{self as fence, core_tests as fence_fixture},
    peer_status_wire::{self as status, state},
};
use cgka_traits::{
    AppMessageRetentionDecision, FanoutMlsState, FanoutTargetStatus, app_event::MarmotAppEvent,
};
use std::{
    io::{BufRead, BufReader, Write},
    num::NonZeroU64,
    process::{Child, Command, Stdio},
    sync::{Mutex as SyncMutex, mpsc},
    time::Instant as ParentClock,
};
use transport_nostr_peeler::NostrTransportEvent;

const APP_KIND: u64 = 30104;
const APP_TIMEOUT: Duration = Duration::from_secs(5);
const PROOF_TIMEOUT: Duration = Duration::from_secs(180);
const BENCHMARK_SAMPLE_TIMEOUT: Duration = Duration::from_secs(50);
const CALLBACK_BEGIN: &str = "peer-heartbeat-callback-read-begin";
const METRICS_PREFIX: &str = "peer_heartbeat_private_wss ";
const CHILD_MODE: &str = "MARMISH_TEST_PRIVATE_HEARTBEAT_CHILD";

#[derive(Clone, Copy)]
pub(in super::super) enum Run {
    Proof,
    Benchmark,
}

impl Run {
    fn timeout(self) -> Duration {
        match self {
            Self::Proof => PROOF_TIMEOUT,
            Self::Benchmark => Duration::from_secs(600),
        }
    }

    fn count(self) -> usize {
        match self {
            Self::Proof => 2,
            Self::Benchmark => 12,
        }
    }
}

#[test]
#[ignore = "requires subprocess, staged native SQLCipher and independent private WSS accounts"]
fn private_wss_heartbeat_observes_provenance_before_publish() {
    run_subprocess(
        Run::Proof,
        "private_wss_heartbeat_observes_provenance_before_publish",
    );
}

#[test]
#[ignore = "opt-in twelve-reservation private WSS heartbeat benchmark"]
fn private_wss_heartbeat_benchmark() {
    run_subprocess(Run::Benchmark, "private_wss_heartbeat_benchmark");
}

fn run_subprocess(mode: Run, test: &str) {
    if std::env::var_os(CHILD_MODE).as_deref() == Some(std::ffi::OsStr::new(test)) {
        run_native(mode.timeout(), move || prove_lost_ack(true, Some(mode)));
    } else {
        supervise(mode, test);
    }
}

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while std::time::Instant::now() < deadline {
                if self.0.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn supervise(mode: Run, test: &str) {
    let temp = private();
    let module = module_path!().split_once("::").unwrap().1;
    let selector = format!("{module}::{test}");
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &selector, "--ignored", "--nocapture"])
            .env(CHILD_MODE, test)
            .env("TMPDIR", temp.path())
            .current_dir(temp.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (trace_tx, trace) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout)
            .lines()
            .map_while(std::result::Result::ok)
        {
            if line.starts_with("peer-heartbeat-callback-") || line.starts_with(METRICS_PREFIX) {
                let _ = trace_tx.send((ParentClock::now(), line));
            }
        }
    });
    let whole = ParentClock::now() + mode.timeout();
    let mut monitor = Monitor::new();
    let status = loop {
        for (when, line) in trace.try_iter() {
            monitor.accept(when, &line).unwrap();
        }
        let now = ParentClock::now();
        monitor.check_read(now).unwrap();
        assert!(
            now < whole,
            "private heartbeat child exceeded whole deadline"
        );
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // An inherited pipe descriptor cannot extend the whole-test deadline.
    let drain = (ParentClock::now() + Duration::from_millis(500)).min(whole);
    loop {
        match trace.recv_timeout(drain.saturating_duration_since(ParentClock::now())) {
            Ok((when, line)) => monitor.accept(when, &line).unwrap(),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("private heartbeat trace did not close"),
        }
    }
    reader.join().unwrap();
    assert!(
        status.success() && monitor.complete(mode.count()),
        "private heartbeat proof did not complete at {}",
        monitor.stage
    );
}

struct Monitor {
    reading: Option<ParentClock>,
    completed: usize,
    metrics: bool,
    stage: &'static str,
}

impl Monitor {
    fn new() -> Self {
        Self {
            reading: None,
            completed: 0,
            metrics: false,
            stage: "enrollment",
        }
    }

    fn complete(&self, expected: usize) -> bool {
        self.reading.is_none() && self.completed == expected && self.metrics
    }

    fn check_read(&self, now: ParentClock) -> std::result::Result<(), &'static str> {
        self.reading
            .is_none_or(|end| now < end)
            .then_some(())
            .ok_or("native callback read stalled")
    }

    fn accept(&mut self, when: ParentClock, line: &str) -> std::result::Result<(), &'static str> {
        self.check_read(when)?;
        match line {
            CALLBACK_BEGIN if self.reading.is_none() && !self.metrics => {
                self.reading = Some(when + APP_TIMEOUT);
            }
            "peer-heartbeat-callback-observed" if self.reading.take().is_some() => {
                self.completed += 1;
            }
            "peer-heartbeat-callback-absent" => return Err("native callback fanout absent"),
            "peer-heartbeat-callback-storage" => return Err("native callback read failed"),
            "peer-heartbeat-callback-binding" => return Err("native callback binding mismatch"),
            "peer-heartbeat-callback-ready" => self.stage = "authority",
            "peer-heartbeat-callback-seed" => self.stage = "reservation",
            "peer-heartbeat-callback-arm" => self.stage = "arm",
            "peer-heartbeat-callback-ack" => self.stage = "relay/native publication",
            "peer-heartbeat-callback-ingress" => self.stage = "native ingress/projection",
            "peer-heartbeat-callback-cleanup" => self.stage = "cleanup",
            line if line.starts_with(METRICS_PREFIX) && !self.metrics && self.reading.is_none() => {
                self.metrics = true;
                eprintln!("{line}");
            }
            _ => return Err("private heartbeat trace did not match the expected protocol"),
        }
        Ok(())
    }
}

#[test]
fn callback_watchdog_refuses_missing_binding_and_stalled_provenance() {
    let start = ParentClock::now();
    for (marker, expected) in [
        (
            "peer-heartbeat-callback-absent",
            "native callback fanout absent",
        ),
        (
            "peer-heartbeat-callback-storage",
            "native callback read failed",
        ),
        (
            "peer-heartbeat-callback-binding",
            "native callback binding mismatch",
        ),
        (
            "peer-heartbeat-callback-unknown",
            "private heartbeat trace did not match the expected protocol",
        ),
    ] {
        let mut monitor = Monitor::new();
        monitor.accept(start, CALLBACK_BEGIN).unwrap();
        assert_eq!(monitor.accept(start, marker), Err(expected));
        assert!(!monitor.complete(1));
    }
    let mut monitor = Monitor::new();
    monitor.accept(start, CALLBACK_BEGIN).unwrap();
    assert!(
        monitor
            .check_read(start + APP_TIMEOUT - Duration::from_millis(1))
            .is_ok()
    );
    assert_eq!(
        monitor.check_read(start + APP_TIMEOUT),
        Err("native callback read stalled")
    );
    assert_eq!(
        monitor.accept(start + APP_TIMEOUT, "peer-heartbeat-callback-observed"),
        Err("native callback read stalled")
    );
    assert!(!monitor.complete(1));
}

fn trace(marker: &str) {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{marker}").unwrap();
    stdout.flush().unwrap();
}

#[derive(Clone, Eq, PartialEq)]
struct Scope {
    founder: nostr::key::PublicKey,
    recipient: nostr::key::PublicKey,
    group: GroupId,
    route: [u8; 32],
    endpoint: TransportEndpoint,
    room: status::Hex32,
    generation: NonZeroU64,
    pin: EpochPin,
    founder_leaf: u32,
    recipient_leaf: u32,
    deadline: u64,
    offer_revision: [u8; 32],
}

struct Prepared {
    scope: Scope,
    payload: Vec<u8>,
    app_id: String,
    body: status::Heartbeat,
    started: Instant,
    revision: NonZeroU64,
}

#[derive(Clone)]
struct Candidate {
    request: TransportPublishRequest,
    id: [u8; 32],
    digest: [u8; 32],
    observed: Duration,
    lookup: Duration,
}

enum Observation {
    Awaiting,
    Observed(Box<Candidate>),
    Absent,
    Storage,
    Binding,
}

struct Turn {
    socket: Arc<Mutex<Owner>>,
    candidate: Candidate,
    deadline: Instant,
}

#[derive(Default)]
struct GateState {
    observation: Option<Observation>,
    turn: Option<Turn>,
    ack: Option<Duration>,
}

#[derive(Clone)]
struct CaptureGate {
    storage: SqliteAccountStorage,
    prepared: Arc<Prepared>,
    state: Arc<SyncMutex<GateState>>,
    wall: Arc<AtomicU64>,
    current: Arc<SyncMutex<Option<Authority>>>,
}

impl CaptureGate {
    fn observe(&self, request: TransportPublishRequest) -> Observation {
        trace(CALLBACK_BEGIN);
        let started = Instant::now();
        let fanout = match self.storage.outbound_fanout(&request.message.id) {
            Ok(Some(fanout)) => fanout,
            Ok(None) => return Observation::Absent,
            Err(_) => return Observation::Storage,
        };
        let Some(application) = fanout.application_message() else {
            return Observation::Binding;
        };
        let mut frozen = fanout.request().clone();
        frozen.required_acks = 1;
        let scope = &self.prepared.scope;
        let expected = TransportPublishTarget::Group {
            group_id: scope.group.clone(),
            transport_group_id: scope.route.to_vec(),
            endpoints: vec![scope.endpoint.clone()],
        };
        let Some(signed) = NostrTransportEvent::from_transport_message(&request.message)
            .ok()
            .and_then(|event| event.to_verified_nostr_event().ok())
        else {
            return Observation::Binding;
        };
        let id = signed.id.to_bytes();
        if frozen != request
            || request.target != expected
            || request.account_id.as_slice() != scope.founder.to_bytes()
            || request.validate_envelope_matches_target().is_err()
            || request.message.id.as_slice() != id
            || signed.kind.as_u16() != 445
            || signed
                .tags
                .iter()
                .filter(|tag| {
                    tag.as_slice().len() == 2
                        && tag.as_slice()[0] == "h"
                        && tag.as_slice()[1] == hex::encode(scope.route)
                })
                .count()
                != 1
            || fanout.group_id() != Some(&scope.group)
            || fanout.mls_state() != FanoutMlsState::NotApplicable
            || fanout.pending_ref().is_some()
            || fanout.target_statuses() != [FanoutTargetStatus::Attempting]
            || fanout.target_attempt_count(0) != 1
            || application.group_id != scope.group
            || application.source_epoch != scope.pin.epoch
            || application.app_event_id != self.prepared.app_id
            || application.retention
                != AppMessageRetentionDecision::new(self.prepared.body.sampled_at, 0)
        {
            return Observation::Binding;
        }
        Observation::Observed(Box::new(Candidate {
            digest: Sha256::digest(&request.message.payload).into(),
            request,
            id,
            observed: self.prepared.started.elapsed(),
            lookup: started.elapsed(),
        }))
    }
}

#[async_trait]
impl TransportAdapter for CaptureGate {
    async fn activate_account(
        &self,
        _: TransportAccountActivation,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn sync_account_groups(
        &self,
        _: TransportGroupSync,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn receive(
        &self,
    ) -> std::result::Result<Option<TransportDelivery>, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn deactivate_account(
        &self,
        _: &MemberId,
    ) -> std::result::Result<(), TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn publish_commit(
        &self,
        _: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        Err(TransportAdapterError::Closed)
    }

    async fn publish(
        &self,
        request: TransportPublishRequest,
    ) -> std::result::Result<TransportPublishReport, TransportAdapterError> {
        let (capture, turn) = {
            let mut state = self.state.lock().unwrap();
            let capture = matches!(state.observation.take(), Some(Observation::Awaiting));
            (capture, state.turn.take())
        };
        if capture {
            let observation = self.observe(request);
            trace(match observation {
                Observation::Observed(_) => "peer-heartbeat-callback-observed",
                Observation::Absent => "peer-heartbeat-callback-absent",
                Observation::Storage => "peer-heartbeat-callback-storage",
                Observation::Binding => "peer-heartbeat-callback-binding",
                Observation::Awaiting => unreachable!(),
            });
            self.state.lock().unwrap().observation = Some(observation);
            return Err(TransportAdapterError::Closed);
        }
        let turn = turn.ok_or(TransportAdapterError::Closed)?;
        if request != turn.candidate.request
            || <[u8; 32]>::from(Sha256::digest(&request.message.payload)) != turn.candidate.digest
        {
            return Err(TransportAdapterError::Closed);
        }
        let scope = &self.prepared.scope;
        let deadline = turn.deadline;
        let mut socket = timeout(APP_TIMEOUT, turn.socket.lock())
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        if socket.origin() != scope.endpoint.as_str()
            || Instant::now() >= deadline
            || !permit(
                &self.prepared,
                self.current.lock().unwrap().as_ref(),
                self.wall.load(Ordering::Acquire),
            )
        {
            return Err(TransportAdapterError::Closed);
        }
        let ack = socket
            .push_group_exact(
                scope.route,
                &request.message.payload,
                turn.candidate.id,
                deadline,
            )
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        if !socket.accepted(&ack, turn.candidate.id) {
            return Err(TransportAdapterError::Closed);
        }
        self.state.lock().unwrap().ack = Some(self.prepared.started.elapsed());
        socket
            .stop(deadline)
            .await
            .map_err(|_| TransportAdapterError::Closed)?;
        Ok(TransportPublishReport {
            message_id: request.message.id,
            accepted: vec![TransportEndpointReceipt {
                endpoint: scope.endpoint.clone(),
                accepted_at: None,
                ack_kind: Some(TransportEndpointAckKind::Affirmative),
            }],
            failed: Vec::new(),
            required_acks: 1,
        })
    }
}

#[derive(Clone)]
struct Authority {
    scope: Scope,
    revision: NonZeroU64,
    enabled: bool,
}

fn live(prepared: &Prepared, wall: u64) -> bool {
    (prepared.body.sampled_at..prepared.scope.deadline).contains(&wall)
        && wall - prepared.body.sampled_at < 60
}

fn permit(prepared: &Prepared, current: Option<&Authority>, wall: u64) -> bool {
    current.is_some_and(|current| {
        current.enabled
            && current.revision == prepared.revision
            && current.scope == prepared.scope
            && live(prepared, wall)
    })
}

struct Fence {
    db: rusqlite::Connection,
    namespace: fence::Namespace,
    key: fence::SendScope,
}

impl Fence {
    fn new(run: &ReceiptRun<'_, '_>, scope: &Scope, revision: NonZeroU64) -> Self {
        let path = run
            .round
            .founder_root
            .path()
            .join("fixture-heartbeat-fence.db");
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let db = fence_fixture::encrypted_connection(&path, random()).unwrap();
        fence_fixture::install_fixture(&db);
        let namespace = fence::Namespace {
            profile: fence::ProfileName::new("private-heartbeat-wss").unwrap(),
            account: scope.founder,
        };
        let key = fence::SendScope::new(
            namespace.clone(),
            scope.recipient,
            scope.group.as_slice().to_vec(),
            scope.room,
            scope.generation,
        )
        .unwrap();
        fence_fixture::authorize(&db, &key, revision, fence::State::Enabled(None));
        Self { db, namespace, key }
    }

    fn reserve(&mut self, authority: &Authority, wall: u64, host: status::HostStatus) -> Prepared {
        let started = Instant::now();
        assert!(authority.enabled && wall < authority.scope.deadline);
        let mut store = fence::Store::attach(&mut self.db, self.namespace.clone()).unwrap();
        store.synchronize(&self.key, authority.revision).unwrap();
        let stamp = NonZeroU64::new(wall).unwrap();
        let reservation = store
            .reserve_heartbeat(&self.key, authority.revision, None, stamp, stamp)
            .unwrap();
        let (reserved, revision, sequence, permission, sampled) = reservation.parts();
        assert_eq!((reserved, revision), (&self.key, authority.revision));
        let body = status::Heartbeat {
            sequence,
            sampled_at: sampled.get(),
            interval: 3600,
            host,
            permission,
        };
        let scope = authority.scope.clone();
        let record = status::Record::new(scope.room, status::Payload::Heartbeat(body.clone()))
            .encode()
            .unwrap();
        let payload = encode_app(&scope, sampled.get(), APP_KIND, Vec::new(), record);
        let app_id = MarmotAppEvent::decode(&payload).unwrap().id;
        Prepared {
            scope,
            payload,
            app_id,
            body,
            started,
            revision,
        }
    }
}

fn encode_app(
    scope: &Scope,
    authored_at: u64,
    kind: u64,
    tags: Vec<Vec<String>>,
    record: Vec<u8>,
) -> Vec<u8> {
    MarmotAppEvent::new(
        scope.founder.to_hex(),
        authored_at,
        kind,
        tags,
        String::from_utf8(record).unwrap(),
    )
    .encode()
    .unwrap()
}

fn maximal() -> status::HostStatus {
    status::HostStatus {
        username: Some("\"".repeat(32)),
        uptime: Some(status::MAX_UPTIME),
        load: Some([status::MAX_LOAD; 3]),
        disks: Some(
            (0..status::MAX_DISKS)
                .map(|index| status::DiskStatus {
                    label: format!("{index}{}", "\"".repeat(15)),
                    used: u64::MAX / 2,
                    available: (u64::MAX / 2) + 1,
                })
                .collect(),
        ),
        ips: Some(
            (0..status::MAX_IPS)
                .map(|index| {
                    std::net::IpAddr::V6(std::net::Ipv6Addr::new(
                        0x2001,
                        0xdb8,
                        0xffff,
                        0xffff,
                        0xffff,
                        0xffff,
                        0xffff,
                        index as u16,
                    ))
                })
                .collect(),
        ),
    }
}

fn verify_membership(
    rt: &AccountDeviceRuntime<
        impl TransportAdapter,
        impl TransportRoutingPolicy,
        impl KeyPackagePublisher,
    >,
    scope: &Scope,
    local: nostr::key::PublicKey,
) {
    assert_eq!(rt.self_id().as_slice(), local.to_bytes());
    assert_eq!(stable_pin(rt, &scope.group), scope.pin);
    let route = rt
        .app_component(&scope.group, NOSTR_ROUTING_COMPONENT_ID)
        .unwrap()
        .unwrap();
    let route = decode_nostr_routing_v1(&route).unwrap();
    assert_eq!(route.nostr_group_id, scope.route);
    assert_eq!(route.relays.as_slice(), [scope.endpoint.as_str()]);
    let roster = rt.canonical_group_membership(&scope.group).unwrap();
    assert!(roster.local_leaf_active);
    assert_eq!(roster.member_leaves.len(), 2);
    for (member, expected_leaf) in [
        (scope.founder, scope.founder_leaf),
        (scope.recipient, scope.recipient_leaf),
    ] {
        assert!(
            roster
                .member_leaves
                .iter()
                .any(|leaf| leaf.leaf_index == expected_leaf
                    && leaf.member.as_slice() == member.to_bytes())
        );
    }
}

#[derive(Clone, Copy)]
struct Received<'a> {
    group: &'a GroupId,
    author: &'a MemberId,
    leaf: Option<u32>,
    epoch: cgka_traits::EpochId,
}

fn checked_payload(
    accepted: &Scope,
    received: Received<'_>,
    bytes: &[u8],
    wall: u64,
) -> Option<MarmotAppEvent> {
    if received.group != &accepted.group
        || received.author.as_slice() != accepted.founder.to_bytes()
        || received.leaf != Some(accepted.founder_leaf)
        || received.epoch != accepted.pin.epoch
        || wall >= accepted.deadline
    {
        return None;
    }
    let app = MarmotAppEvent::decode(bytes).ok()?;
    if app.kind != APP_KIND
        || !app.tags.is_empty()
        || app.validate_sender(&accepted.founder.to_hex()).is_err()
        || app.encode().ok()?.as_slice() != bytes
        || app.created_at > wall
        || wall.saturating_sub(app.created_at) >= 60
    {
        return None;
    }
    let record = status::Record::decode(app.content.as_bytes()).ok()?;
    let status::Payload::Heartbeat(heartbeat) = &record.payload else {
        return None;
    };
    if record.room != accepted.room
        || heartbeat.sampled_at != app.created_at
        || record.encode().ok()?.as_slice() != app.content.as_bytes()
    {
        return None;
    }
    Some(app)
}

pub(super) async fn prove(
    run: &ReceiptRun<'_, '_>,
    projection: &FounderProjection,
    accepted_at: u64,
    mode: Run,
) {
    trace("peer-heartbeat-callback-ready");
    let mut db = projection.checkpoint(run);
    let source = db.bind_source(run.round.relay.url.as_str()).unwrap();
    let acceptance = db
        .read_founder_acceptance(&source, run.round.signer, run.wall())
        .unwrap();
    let saved = acceptance.source();
    let prior = saved.decoded.receipt;
    let prior_app = accept_receipt_wire::prepare(&prior, saved.decoded.created_at).unwrap();
    assert_eq!(acceptance.accepted_at(), accepted_at);
    assert_eq!(saved.decoded.event_id, run.inner);
    assert_eq!(saved.group, run.stage.group.as_slice());
    assert_eq!(saved.route, run.stage.routing.nostr_group_id);
    assert_eq!(prior.founder, run.round.founder.public_key());
    assert_eq!(prior.recipient, run.round.signer);
    let mut owner = (run.reopen)();
    let (row, published) = run.read(&mut owner, Step::Published { number: 2 });
    let mut recipient = run.open(&owner, &row, ReceiptGate::closed(run.round.wall.clone()));
    outbound::acknowledge_published(&recipient, run.image, &published, run.wall()).unwrap();
    let closed = run.founder.closed();
    let durable = DurableExpectation::read(run, &mut db, &source);
    assert!(durable.expected().receipt == &prior);
    let roster = closed
        .runtime()
        .canonical_group_membership(&run.stage.group)
        .unwrap();
    let leaf = roster
        .member_leaves
        .iter()
        .find(|leaf| leaf.member == closed.runtime().self_id())
        .unwrap()
        .leaf_index;
    let scope = Scope {
        founder: prior.founder,
        recipient: prior.recipient,
        group: run.stage.group.clone(),
        route: saved.route,
        endpoint: TransportEndpoint(run.round.relay.url.to_string()),
        room: status::Hex32(
            Sha256::digest(
                [
                    b"marmish/fixture/heartbeat-room/v1\0".as_slice(),
                    &prior.room_id,
                ]
                .concat(),
            )
            .into(),
        ),
        generation: NonZeroU64::new(run.stage.generation).unwrap(),
        pin: EpochPin {
            epoch: cgka_traits::EpochId(prior.epoch),
            identity: prior.epoch_identity,
        },
        founder_leaf: leaf,
        recipient_leaf: saved.sender_leaf,
        deadline: saved.deadline,
        offer_revision: prior.offer_revision,
    };
    assert!(run.wall() < scope.deadline);
    verify_membership(closed.runtime(), &scope, scope.founder);
    verify_membership(&recipient, &scope, scope.recipient);
    drop((closed, db));
    let recipient_socket = run.open_sender().await;
    let mut recipient_socket = recipient_socket.lock().await;
    let page = recipient_socket
        .group_history(scope.route, Instant::now() + APP_TIMEOUT)
        .await
        .unwrap();
    assert!(recipient_socket.accepts_group_page(&page));

    let bound = state::ClientBinding {
        room: scope.room,
        local_client: status::Hex32(scope.recipient.to_bytes()),
        pinned_server: status::Hex32(scope.founder.to_bytes()),
        local_generation: scope.generation,
    };
    let mut receiver = state::Receiver::new(bound);
    let authority = Authority {
        scope,
        revision: NonZeroU64::MIN,
        enabled: true,
    };
    let mut fence = Fence::new(run, &authority.scope, authority.revision);
    let run_clock = Instant::now();
    let mut samples = Vec::with_capacity(mode.count());
    let baseline = run.round.relay.group_writes().len();
    let peer_writes = run.round.relay.peer_transcript();
    let public_attempts = run.round.relay.publication_attempts(APP_KIND as u16);
    for index in 0..mode.count() {
        let host = if matches!(mode, Run::Proof) && index == 1 {
            maximal()
        } else {
            status::HostStatus {
                username: Some("fixture-user".to_owned()),
                ..Default::default()
            }
        };
        trace("peer-heartbeat-callback-seed");
        let prepared = fence.reserve(&authority, run.wall(), host);
        assert_eq!(prepared.body.sequence.get(), (index + 1) as u64);
        let check_replay = matches!(mode, Run::Proof);
        if check_replay && index == 0 {
            refusal_cases(&prepared, prior_app.payload(), run.wall());
            assert_eq!(run.round.relay.group_writes().len(), baseline);
        }
        let turn = SampleTurn {
            run,
            recipient: &mut recipient,
            recipient_socket: &mut recipient_socket,
            receiver: &mut receiver,
            authority: &authority,
            bound,
            run_clock,
            check_replay,
        };
        let send = turn.publish(prepared);
        let sample = match mode {
            Run::Proof => send.await,
            Run::Benchmark => timeout(BENCHMARK_SAMPLE_TIMEOUT, send).await.unwrap(),
        };
        samples.push(sample);
        run.round.wall.fetch_add(1, Ordering::AcqRel);
    }
    recipient_socket
        .stop(Instant::now() + APP_TIMEOUT)
        .await
        .unwrap();
    let writes = run.round.relay.group_writes().len() - baseline;
    assert_eq!(writes, samples.len());
    assert!(run.round.relay.peer_transcript() == peer_writes);
    assert_eq!(
        run.round.relay.publication_attempts(APP_KIND as u16),
        public_attempts
    );
    assert!(run_clock.elapsed() >= Duration::from_secs(mode.count() as u64));
    report(&samples, writes, run_clock.elapsed());
}

type HeartbeatRecipient = AccountDeviceRuntime<ReceiptGate, StaticTransportRouting, Gate>;

struct SampleTurn<'a, 'b, 'profile> {
    run: &'a ReceiptRun<'b, 'profile>,
    recipient: &'a mut HeartbeatRecipient,
    recipient_socket: &'a mut Owner,
    receiver: &'a mut state::Receiver,
    authority: &'a Authority,
    bound: state::ClientBinding,
    run_clock: Instant,
    check_replay: bool,
}

struct Sample {
    callback: Duration,
    lookup: Duration,
    ack: Duration,
    projection: Duration,
    app_bytes: usize,
    wire_bytes: usize,
}

impl SampleTurn<'_, '_, '_> {
    async fn publish(self, prepared: Prepared) -> Sample {
        let Self {
            run,
            recipient,
            recipient_socket,
            receiver,
            authority,
            bound,
            run_clock,
            check_replay,
        } = self;
        let prepared = Arc::new(prepared);
        let scope = &prepared.scope;
        let state = Arc::new(SyncMutex::new(GateState {
            observation: Some(Observation::Awaiting),
            ..Default::default()
        }));
        let current = Arc::new(SyncMutex::new(Some(authority.clone())));
        let mut probe = None;
        let mut founder = run.founder.open_with_adapter(|storage| {
            let gate = CaptureGate {
                storage,
                prepared: prepared.clone(),
                state: state.clone(),
                wall: run.round.wall.clone(),
                current: current.clone(),
            };
            if check_replay && prepared.body.sequence.get() == 1 {
                probe = Some(gate.clone());
            }
            gate
        });
        verify_membership(founder.runtime(), scope, scope.founder);
        let baseline = run.round.relay.group_writes();
        let effects = timeout(
            APP_TIMEOUT,
            founder.send_fixture_app(scope.group.clone(), prepared.payload.clone(), scope.pin),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(effects.published_app_messages.is_empty());
        assert_eq!(run.round.relay.group_writes(), baseline);
        let candidate = match state.lock().unwrap().observation.take() {
            Some(Observation::Observed(candidate)) => *candidate,
            _ => panic!("no exact durable callback provenance"),
        };
        trace("peer-heartbeat-callback-arm");
        let origin = QualifiedOrigin::for_daemon_peer_fixture(
            run.round.relay.peer_mode().unwrap(),
            run.round.founder_root.path(),
            scope.founder,
            scope.recipient,
        )
        .unwrap();
        let sender = Owner::open(
            &origin.connector,
            scope.endpoint.as_str(),
            scope.founder,
            &run.round.founder,
            Instant::now() + APP_TIMEOUT,
        )
        .await
        .unwrap();

        let sender = Arc::new(Mutex::new(sender));
        if let Some(gate) = probe {
            prove_armed_refusals(run, &gate, authority, &candidate, &sender).await;
        }

        let delay = founder
            .runtime()
            .outbound_fanout_retry_delay_ms(&scope.group)
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(delay.max(1000))).await;
        verify_membership(founder.runtime(), scope, scope.founder);
        assert!(permit(&prepared, Some(authority), run.wall()));
        let (id, digest, observed, lookup) = (
            candidate.id,
            candidate.digest,
            candidate.observed,
            candidate.lookup,
        );
        let wire_bytes = candidate.request.message.payload.len();
        state.lock().unwrap().turn = Some(Turn {
            socket: sender,
            candidate,
            deadline: Instant::now() + APP_TIMEOUT,
        });
        let (finished, record) = tokio::join!(
            timeout(APP_TIMEOUT, founder.resume_fixture_app()),
            recipient_socket.next_group_record(Instant::now() + APP_TIMEOUT),
        );
        trace("peer-heartbeat-callback-ack");
        let effects = finished.unwrap().unwrap();
        let [publication] = effects.published_app_messages.as_slice() else {
            panic!("one published identity after one native retry")
        };
        assert_eq!(publication.app_event_id, prepared.app_id);
        assert_eq!(publication.group_id, scope.group);
        assert_eq!(publication.source_epoch, scope.pin.epoch);
        assert_eq!(publication.message_id.as_slice(), id);
        founder
            .runtime()
            .group_authority_view()
            .with_group_authority_snapshot(&scope.group, |storage, _| {
                let accepted = storage.outbound_fanout(&publication.message_id)?.unwrap();
                assert_eq!(accepted.group_id(), Some(&scope.group));
                assert_eq!(accepted.target_statuses(), [FanoutTargetStatus::Accepted]);
                assert_eq!(accepted.target_attempt_count(0), 2);
                assert_eq!(accepted.request().message.id.as_slice(), id);
                assert_eq!(
                    <[u8; 32]>::from(Sha256::digest(&accepted.request().message.payload)),
                    digest
                );
                assert_eq!(
                    accepted.application_message().unwrap().app_event_id,
                    prepared.app_id
                );
                assert_eq!(
                    accepted.first_acceptance_observation(),
                    publication.publication_observation
                );
                assert!(publication.publication_observation.is_some());
                Ok::<_, marmot_session::SessionError>(())
            })
            .unwrap();

        trace("peer-heartbeat-callback-ingress");
        let record = record.unwrap();
        assert!(recipient_socket.accepts_group_record(&record));
        assert_eq!(
            (record.fingerprint().id, record.fingerprint().bytes_sha256),
            (id, digest)
        );
        let delivery = record.delivery(&scope.group, run.wall());
        let ingress = timeout(APP_TIMEOUT, recipient.ingest_delivery(&delivery))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ingress.outcome, IngestOutcome::Processed);
        assert!(!ingress.left_object_unpersisted);
        let mut events = ingress
            .effects
            .events
            .iter()
            .filter(|event| matches!(event, GroupEvent::MessageReceived { .. }));
        let [
            Some(GroupEvent::MessageReceived {
                group_id,
                message_id,
                sender,
                sender_leaf_index,
                epoch,
                payload,
                ..
            }),
            None,
        ] = [events.next(), events.next()]
        else {
            panic!("one native heartbeat source");
        };
        let coordinates = Received {
            group: group_id,
            author: sender,
            leaf: *sender_leaf_index,
            epoch: *epoch,
        };
        assert_eq!(payload, &prepared.payload);
        verify_membership(recipient, scope, scope.recipient);
        recipient
            .group_authority_view()
            .with_group_authority_snapshot(&scope.group, |storage, _| {
                let stored = storage.get_message(message_id)?;
                assert_eq!(stored.group_id, scope.group);
                assert_eq!(stored.id, *message_id);
                assert_eq!(stored.epoch, scope.pin.epoch);
                assert_eq!(stored.state, cgka_traits::MessageState::Processed);
                Ok::<_, marmot_session::SessionError>(())
            })
            .unwrap();
        let app = checked_payload(scope, coordinates, payload, run.wall()).unwrap();
        assert_eq!(
            status::Record::decode(app.content.as_bytes())
                .unwrap()
                .payload,
            status::Payload::Heartbeat(prepared.body.clone())
        );
        let clock = state::Clock {
            wall: run.wall(),
            monotonic: run_clock.elapsed(),
        };
        assert_eq!(
            receiver.receive(bound, app.created_at, app.content.as_bytes(), clock, false),
            Ok(state::Applied::Heartbeat)
        );
        let elapsed = prepared.started.elapsed();
        if check_replay {
            let replay = timeout(APP_TIMEOUT, recipient.ingest_delivery(&delivery))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                replay.outcome,
                IngestOutcome::Ignored {
                    category: cgka_traits::InputRejectionCategory::Duplicate,
                }
            );
            let mut replayed_events = replay
                .effects
                .events
                .iter()
                .filter(|event| matches!(event, GroupEvent::MessageReceived { .. }));
            if let Some(GroupEvent::MessageReceived {
                group_id,
                message_id: replay_id,
                sender,
                sender_leaf_index,
                epoch,
                payload,
                ..
            }) = replayed_events.next()
            {
                assert_eq!(replay_id, message_id);
                let replayed = checked_payload(
                    scope,
                    Received {
                        group: group_id,
                        author: sender,
                        leaf: *sender_leaf_index,
                        epoch: *epoch,
                    },
                    payload,
                    run.wall(),
                )
                .unwrap();
                assert_eq!(
                    receiver.receive(
                        bound,
                        replayed.created_at,
                        replayed.content.as_bytes(),
                        state::Clock {
                            monotonic: clock.monotonic + Duration::from_secs(1),
                            ..clock
                        },
                        false
                    ),
                    Ok(state::Applied::Ignored)
                );
            }
            assert!(replayed_events.next().is_none());
        }
        let state::HeartbeatView::Fresh(view) = receiver.heartbeat(clock) else {
            panic!("authenticated heartbeat readback")
        };
        assert_eq!(view.sample, &prepared.body);
        assert_eq!(view.received_at, clock.wall);
        assert_eq!(
            receiver.permission(clock),
            state::PermissionView::Unavailable
        );
        let writes = run.round.relay.group_writes();
        assert_eq!(&writes[..baseline.len()], baseline.as_slice());
        let [write] = &writes[baseline.len()..] else {
            panic!("one physical status write");
        };
        assert_eq!(write.record, record.fingerprint());
        assert!(write.acknowledged && !write.duplicate);
        let ack = state.lock().unwrap().ack.unwrap();
        assert!(observed <= ack && ack <= elapsed);
        assert_eq!(write.sender, scope.founder);
        trace("peer-heartbeat-callback-cleanup");
        founder
            .acknowledge_fixture_app(&effects.published_app_messages)
            .unwrap();
        Sample {
            callback: observed,
            lookup,
            ack,
            projection: elapsed,
            app_bytes: prepared.payload.len(),
            wire_bytes,
        }
    }
}

// MDK records its target attempt before entering publish. Exercise the actual
// armed adapter with the frozen native request without spending another native
// attempt for each denial. The real native resume remains the positive path.
async fn prove_armed_refusals(
    run: &ReceiptRun<'_, '_>,
    gate: &CaptureGate,
    authority: &Authority,
    candidate: &Candidate,
    sender: &Arc<Mutex<Owner>>,
) {
    let wall = gate.wall.load(Ordering::Acquire);
    let group = run.round.relay.group_writes();
    let peer = run.round.relay.peer_transcript();
    let attempts = run.round.relay.publication_attempts(445);
    let mut lost = authority.clone();
    lost.enabled = false;
    let mut replaced = authority.clone();
    replaced.revision = NonZeroU64::new(authority.revision.get() + 1).unwrap();
    let mut changed = authority.clone();
    changed.scope.pin.identity[0] ^= 1;
    changed.scope.founder_leaf ^= 1;
    let mut wrong_endpoint = authority.clone();
    wrong_endpoint.scope.endpoint = TransportEndpoint("wss://example.invalid/".into());
    let mut mismatched = candidate.request.clone();
    mismatched.message.payload[0] ^= 1;
    let denied = [
        (None, wall, candidate.request.clone()),
        (Some(lost), wall, candidate.request.clone()),
        (Some(replaced), wall, candidate.request.clone()),
        (Some(changed), wall, candidate.request.clone()),
        (Some(wrong_endpoint), wall, candidate.request.clone()),
        (
            Some(authority.clone()),
            authority.scope.deadline,
            candidate.request.clone(),
        ),
        (Some(authority.clone()), wall, mismatched),
    ];
    for (current, sampled, offered) in denied {
        let turn = Turn {
            socket: sender.clone(),
            candidate: candidate.clone(),
            deadline: Instant::now() + APP_TIMEOUT,
        };
        *gate.current.lock().unwrap() = current;
        gate.wall.store(sampled, Ordering::Release);
        assert!(gate.state.lock().unwrap().turn.replace(turn).is_none());
        assert!(matches!(
            timeout(APP_TIMEOUT, gate.publish(offered)).await.unwrap(),
            Err(TransportAdapterError::Closed)
        ));
        assert!(gate.state.lock().unwrap().turn.is_none());
        assert!(gate.state.lock().unwrap().ack.is_none());
        assert_eq!(run.round.relay.group_writes(), group);
        assert!(run.round.relay.peer_transcript() == peer);
        assert_eq!(run.round.relay.publication_attempts(445), attempts);
        let fanout = gate
            .storage
            .outbound_fanout(&candidate.request.message.id)
            .unwrap()
            .unwrap();
        assert_eq!(fanout.target_attempt_count(0), 1);
    }
    gate.wall.store(wall, Ordering::Release);
    *gate.current.lock().unwrap() = Some(authority.clone());
}

fn report(samples: &[Sample], writes: usize, elapsed: Duration) {
    let pct = |q: usize, get: fn(&Sample) -> Duration| {
        let mut sorted: Vec<_> = samples.iter().map(get).collect();
        sorted.sort_unstable();
        sorted[(sorted.len() * q).div_ceil(100) - 1].as_millis()
    };
    let app: usize = samples.iter().map(|sample| sample.app_bytes).sum();
    let wire: usize = samples.iter().map(|sample| sample.wire_bytes).sum();
    let max_app = samples.iter().map(|sample| sample.app_bytes).max().unwrap();
    trace(&format!(
        "{METRICS_PREFIX}samples={} callback_p50_ms={} callback_p95_ms={} lookup_p50_ms={} lookup_p95_ms={} ack_p50_ms={} ack_p95_ms={} projection_p50_ms={} projection_p95_ms={} elapsed_ms={} app_bytes_total={} max_app_bytes={} wire_bytes_total={} writes={writes} projections={}",
        samples.len(),
        pct(50, |s| s.callback),
        pct(95, |s| s.callback),
        pct(50, |s| s.lookup),
        pct(95, |s| s.lookup),
        pct(50, |s| s.ack),
        pct(95, |s| s.ack),
        pct(50, |s| s.projection),
        pct(95, |s| s.projection),
        elapsed.as_millis(),
        app,
        max_app,
        wire,
        samples.len(),
    ));
}

fn refusal_cases(prepared: &Prepared, prior_payload: &[u8], wall: u64) {
    let scope = &prepared.scope;
    let founder = MemberId::new(scope.founder.to_bytes().to_vec());
    let wrong_founder = MemberId::new(scope.recipient.to_bytes().to_vec());
    let received = Received {
        group: &scope.group,
        author: &founder,
        leaf: Some(scope.founder_leaf),
        epoch: scope.pin.epoch,
    };
    for mismatch in [
        Received {
            author: &wrong_founder,
            ..received
        },
        Received {
            epoch: cgka_traits::EpochId(scope.pin.epoch.0 + 1),
            ..received
        },
        Received {
            leaf: Some(scope.recipient_leaf),
            ..received
        },
    ] {
        assert!(checked_payload(scope, mismatch, &prepared.payload, wall).is_none());
    }
    let original = MarmotAppEvent::decode(&prepared.payload).unwrap();
    let wrong_room = status::Record::new(
        status::Hex32([0; 32]),
        status::Payload::Heartbeat(prepared.body.clone()),
    )
    .encode()
    .unwrap();
    let mut noncanonical = original.content.as_bytes().to_vec();
    noncanonical.push(b' ');
    let fixtures = [
        encode_app(scope, wall, APP_KIND, vec![], wrong_room),
        encode_app(
            scope,
            wall,
            APP_KIND,
            vec![vec!["fixture".into(), "tag".into()]],
            original.content.as_bytes().to_vec(),
        ),
        encode_app(scope, wall, APP_KIND, vec![], noncanonical),
        encode_app(
            scope,
            wall + 1,
            APP_KIND,
            vec![],
            original.content.as_bytes().to_vec(),
        ),
    ];
    for malformed in &fixtures {
        assert!(checked_payload(scope, received, malformed, wall).is_none());
    }
    assert!(checked_payload(scope, received, &prepared.payload, wall + 60).is_none());
    assert!(accept_receipt_wire::decode(&prepared.payload).is_err());
    assert!(accept_receipt_wire::decode(prior_payload).is_ok());
    assert!(checked_payload(scope, received, prior_payload, wall).is_none());
    let wrong_marker = original.content.replace("\"heartbeat\"", "\"unknown\"");
    let wrong_marker = encode_app(scope, wall, APP_KIND, vec![], wrong_marker.into_bytes());
    assert!(checked_payload(scope, received, &wrong_marker, wall).is_none());
}
