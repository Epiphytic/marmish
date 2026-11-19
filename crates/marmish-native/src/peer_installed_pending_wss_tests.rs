use std::{
    fs,
    num::NonZeroU64,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::AtomicBool,
};

use cgka_traits::app_components::NostrRoutingV1;
use nostr::key::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{
    command_limits::continuous_time,
    identity::BackendChoice,
    peer_control_version::ControlVersion,
    peer_quota_vfs::dispatch::{
        ProfileSlots, ReceiverQuota, TOTAL_ADMISSION,
        founder::{Expected, FounderQuota},
        mount_for_proof, resolve_once, welcome_counts,
    },
    peer_relay_trust::{ReceiverRouteAdmission, qualified_recipient_fixture},
    transport::{ArtiDirs, RelayPolicy, RouteMode, TransportLimits},
};

use super::*;
use crate::peer_invitation::{
    founder_ledger::{Ledger, Outbound, ledger_binding},
    receiver_inventory::v2_pending::{
        PendingReceiver, committed_custody_changes, fixture_join_result_lost,
        fixture_lose_join_result,
    },
    socket::owner::recipient::PublicationOutcome,
    store::consent_checkpoint::{
        Error as CheckpointError, Status,
        provisioning::prepared_consent::custody::CustodyState,
        wire::{Codec, MAX_OUTER_BYTES},
    },
};
use store::TrustedOutcome;
#[cfg(marmish_peer_receiver_candidate)]
use store::consent_checkpoint::provisioning::prepared_consent::custody::HeldEvidence;

const CHILD_FLAG: &str = "MARMISH_PRIVATE_V2_PENDING_STEP";
const SCENARIO: &str = "MARMISH_PRIVATE_V2_PENDING_SCENARIO";
const RECEIVER: &str = "MARMISH_PRIVATE_V2_PENDING_RECEIVER";
const SHARED: &str = "MARMISH_PRIVATE_V2_PENDING_PUBLIC";
const CHILD: &str = concat!(
    "peer_invitation::prepared_consent::installed_consent::delivery::",
    "pending_bridge::private_v2_pending_child"
);

#[derive(Serialize, Deserialize)]
struct PublicSetup {
    founder: [u8; 32],
    receiver: [u8; 32],
    origin: String,
}

#[derive(Serialize, Deserialize)]
struct HeldCoordinates {
    founder: [u8; 32],
    offer: [u8; 16],
    revision: [u8; 32],
    generation: u64,
    carrier: [u8; 32],
}

impl HeldCoordinates {
    fn selector(&self) -> Selector {
        Selector {
            server: PublicKey::from_slice(&self.founder).unwrap(),
            id: self.offer,
            revision: self.revision,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct NativeCoordinates {
    group: Vec<u8>,
    epoch: u64,
    identity: [u8; 32],
    own_leaf: u32,
}

impl NativeCoordinates {
    fn check(&self, joined: (&[u8], u64, [u8; 32], u32)) {
        assert!(
            joined
                == (
                    self.group.as_slice(),
                    self.epoch,
                    self.identity,
                    self.own_leaf
                ),
            "cold readback differs from the independent founder"
        );
    }
}

struct Running(Child);

impl Running {
    fn spawn(step: &str, scenario: &str, public: &Path, receiver: &Path) -> Self {
        Self(
            Command::new("/proc/self/exe")
                .args(["--ignored", "--exact", CHILD, "--nocapture"])
                .env_clear()
                .env(CHILD_FLAG, step)
                .env(SCENARIO, scenario)
                .env(SHARED, public)
                .env(RECEIVER, receiver)
                .stdin(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }

    async fn wait(mut self, step: &str) {
        let deadline = Instant::now() + Duration::from_secs(65);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "{step} subprocess returned {status}");
                return;
            }
            assert!(Instant::now() < deadline, "{step} subprocess timeout");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "private real NIP-42 WSS, native Welcome and repeated process exits"]
async fn private_v2_pending_survives_cold_cuts() {
    for scenario in ["joined", "before-native", "capacity"] {
        run_pending_scenario(scenario).await;
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "private real NIP-42 WSS with withheld native completion"]
async fn private_v2_pending_recovers_uncertain_join() {
    run_pending_scenario("uncertain").await;
}

async fn run_pending_scenario(scenario: &str) {
    let public = tempfile::tempdir().unwrap();
    let founder_root = public.path().join("founder-profile");
    fs::create_dir(&founder_root).unwrap();
    fs::set_permissions(&founder_root, fs::Permissions::from_mode(0o700)).unwrap();
    let mut lease = ProfileLease::acquire(&founder_root, "test").unwrap();
    let founder = prepare(&mut lease).public_key();
    drop(lease);
    let unrelated = Keys::generate().public_key();
    let (mut relay, home, lease, material) =
        qualified_recipient_fixture(founder, unrelated, Duration::from_secs(240)).await;
    let account = material.public_key();
    assert_ne!(account, founder);
    drop(material);
    drop(lease);
    let setup = PublicSetup {
        founder: founder.to_bytes(),
        receiver: account.to_bytes(),
        origin: relay.url.to_string(),
    };
    write_public(public.path(), "setup", &setup);
    fs::write(public.path().join("relay-ca"), relay.qualification_ca()).unwrap();
    let founder_job = Running::spawn("founder", scenario, public.path(), home.path());
    let ingress_job = Running::spawn("held", scenario, public.path(), home.path());
    founder_job.wait("founder").await;
    ingress_job.wait("Held").await;
    assert!(relay.authenticated(founder));
    assert!(relay.authenticated(account));
    assert_eq!(relay.peer_transcript().len(), 3);
    let before = relay.all_publication_attempts();
    for step in ["start", "qualify", "replay"] {
        Running::spawn(step, scenario, public.path(), home.path())
            .wait(step)
            .await;
    }
    assert_eq!(relay.peer_transcript().len(), 3);
    let bridge_publications = relay.all_publication_attempts() - before;
    assert_eq!(bridge_publications, 0);
    relay.stop().await;
    println!("private-v2-pending scenario={scenario} bridge_publications={bridge_publications}");
}

#[test]
#[ignore = "child of the bounded installed Pending proof"]
fn private_v2_pending_child() {
    let Ok(step) = std::env::var(CHILD_FLAG) else {
        return;
    };
    let public = PathBuf::from(std::env::var_os(SHARED).unwrap());
    let receiver = PathBuf::from(std::env::var_os(RECEIVER).unwrap());
    let scenario = std::env::var(SCENARIO).unwrap();
    mount_for_proof().unwrap();
    std::thread::Builder::new()
        .stack_size(32 << 20)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(Box::pin(async {
                let setup: PublicSetup = read_public(&public, "setup");
                if step == "founder" {
                    run_founder(&public, &setup).await;
                } else {
                    run_recipient(&public, &receiver, &setup, &step, &scenario).await;
                }
            }));
        })
        .unwrap()
        .join()
        .unwrap();
}

fn read_public<T: serde::de::DeserializeOwned>(dir: &Path, name: &str) -> T {
    let encoded = fs::read(dir.join(name)).unwrap();
    assert!(encoded.len() <= 4096);
    serde_json::from_slice(&encoded).unwrap()
}

fn write_public(dir: &Path, name: &str, value: &impl Serialize) {
    let staged = dir.join(format!("{name}.new"));
    fs::write(&staged, serde_json::to_vec(value).unwrap()).unwrap();
    fs::rename(staged, dir.join(name)).unwrap();
}

fn prepare(lease: &mut ProfileLease) -> IdentityMaterial {
    lease
        .prepare(
            BackendChoice::File,
            Path::new("/unused"),
            std::time::Instant::now() + Duration::from_secs(5),
        )
        .unwrap()
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(25)
}

fn sample() -> u64 {
    Wall::System.sample().unwrap().as_secs()
}

async fn run_founder(public: &Path, setup: &PublicSetup) {
    let receiver = PublicKey::from_slice(&setup.receiver).unwrap();
    let mut lease = ProfileLease::acquire(&public.join("founder-profile"), "test").unwrap();
    let material = prepare(&mut lease);
    assert_eq!(material.public_key().to_bytes(), setup.founder);
    let lease = Arc::new(lease);
    let signer = material.signer();
    let paths = Paths::new(&lease, &material).unwrap();
    paths.initialize_founder_directory().unwrap();
    let mut quota = FounderQuota::mount(&lease).unwrap();

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(
            fs::read(public.join("relay-ca")).unwrap(),
        ))
        .unwrap();
    let endpoint = nostr::types::Url::parse(&setup.origin).unwrap();
    let policy = RelayPolicy::parse(
        std::slice::from_ref(&setup.origin),
        RouteMode::Off,
        TransportLimits::default(),
    )
    .unwrap();
    let connector = RelayConnector::for_loopback_fixture(
        policy,
        ArtiDirs {
            state: lease.arti_state().into(),
            cache: lease.arti_cache().into(),
        },
        endpoint,
        roots,
    )
    .unwrap();
    let mut remote = Owner::open(
        &connector,
        &setup.origin,
        signer.public_key(),
        &signer,
        deadline(),
    )
    .await
    .unwrap();

    let until = deadline();
    while !public.join("recipient-ready").exists() {
        assert!(Instant::now() < until, "recipient did not subscribe");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let generation = NonZeroU64::new(1).unwrap();
    let ledger_file = quota.ledger();
    paths.create_founder_file(&ledger_file).unwrap();
    let mut ledger = Ledger::open(
        &ledger_file,
        *paths.founder_ledger_key(),
        ledger_binding(signer.public_key(), receiver, &setup.origin),
        generation,
        true,
    )
    .unwrap();
    let room = [0xe1; 16];
    let routing = NostrRoutingV1::new([0xe2; 32], vec![setup.origin.clone()]).unwrap();
    let codec = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2).unwrap();
    let offer = codec
        .prepare_offer(&signer, receiver, room, &routing, sample())
        .unwrap();
    let sent = offer.flight.flight();
    ledger.claim(&offer).unwrap();
    ledger
        .arm(Outbound::Offer, sent, offer.frame(), sample())
        .unwrap();
    assert!(remote.publish(&offer, sent, deadline()).await.unwrap());
    ledger
        .terminal(Outbound::Offer, Outbound::Offer.accepted())
        .unwrap();

    let live = remote
        .wait_live(deadline())
        .await
        .unwrap()
        .expect("founder did not receive Consent within the fixture window");
    let frame = live.arrival().body.get().as_bytes();
    let consent = codec
        .open_consent(&signer, frame, &offer.flight, sample())
        .unwrap();
    ledger.receive_consent(&consent, frame).unwrap();
    ledger.reserve(sample()).unwrap();
    let file = paths.create_founder_file(&quota.main(generation)).unwrap();
    let started = ledger.start(file).unwrap();
    let expected = Expected {
        signer: signer.clone(),
        recipient: receiver,
        routing: &routing,
        room,
        reference: &consent.binding().reference,
    };
    let (created, writes) = quota
        .create(
            started,
            &paths.founder_image_key(generation),
            &expected,
            consent.key_package().clone(),
            || Ok(()),
        )
        .unwrap();
    assert!(writes > 0);
    let delivery = codec
        .prepare_delivery(
            &signer,
            &consent,
            &created.welcome,
            sample(),
            consent.flight().expires_at - 1,
        )
        .unwrap();
    ledger
        .freeze(&delivery, &created.image, room, expected.reference)
        .unwrap();
    let sent = delivery.flight.flight();
    ledger
        .arm(Outbound::Delivery, sent, delivery.frame(), sample())
        .unwrap();
    assert!(remote.publish(&delivery, sent, deadline()).await.unwrap());
    ledger
        .terminal(Outbound::Delivery, Outbound::Delivery.accepted())
        .unwrap();
    write_public(
        public,
        "native",
        &NativeCoordinates {
            group: created.image.group.as_slice().to_vec(),
            epoch: created.image.pin.epoch.0,
            identity: created.image.pin.identity,
            own_leaf: created.image.recipient_leaf,
        },
    );
}

#[derive(Clone, Copy)]
struct Effects {
    polls: usize,
    closed_joins: usize,
    custody_changes: usize,
}

impl Effects {
    fn read() -> Self {
        let (polls, closed_joins) = welcome_counts();
        Self {
            polls,
            closed_joins,
            custody_changes: committed_custody_changes(),
        }
    }

    fn assert_since(self, window: &str, expected: (usize, usize, usize)) {
        let current = Self::read();
        let polls = current.polls - self.polls;
        let closed_joins = current.closed_joins - self.closed_joins;
        let custody_changes = current.custody_changes - self.custody_changes;
        assert_eq!((polls, closed_joins, custody_changes), expected, "{window}");
        println!(
            "private-v2-pending window={window} welcome_polls={polls} closed_native_joins={closed_joins} committed_custody_rows={custody_changes}"
        );
    }
}

fn observe_turn<T>(window: &str, signed_end: u64, run: impl FnOnce() -> T) -> T {
    let wall = Wall::System;
    let began = wall.sample().unwrap().as_secs();
    let ticks = continuous_time().unwrap();
    let output = run();
    let elapsed = continuous_time().unwrap().checked_sub(ticks).unwrap();
    let ended = wall.sample().unwrap().as_secs();
    assert!(
        began <= ended && ended < signed_end,
        "{window} exceeded signed custody bounds"
    );
    assert!(
        elapsed < Duration::from_secs((signed_end - began).min(5)),
        "{window} exceeded its continuous time bound"
    );
    println!(
        "private-v2-pending window={window} continuous_ms={}",
        elapsed.as_millis()
    );
    output
}

#[cfg(marmish_peer_receiver_candidate)]
struct ObservationFixture {
    _startup: crate::peer_candidate_startup::Startup,
    _listener: crate::peer_candidate_startup::ListenerRegistration,
    _worker: crate::peer_candidate_startup::Ready,
    readiness: crate::peer_candidate_startup::Readiness,
    stop: Arc<AtomicBool>,
    producer: crate::ipc::peer_read::pending_view::Producer,
    reader: crate::ipc::peer_read::pending_view::Reader,
}

#[cfg(marmish_peer_receiver_candidate)]
impl ObservationFixture {
    async fn new() -> Self {
        let (startup, listener, worker) = crate::peer_candidate_startup::pair();
        startup.ordinary_ready();
        listener.registered();
        let worker = worker.ready().await.unwrap();
        let readiness = worker.readiness();
        let (producer, reader) =
            crate::ipc::peer_read::pending_view::pair(NonZeroU64::MIN).unwrap();
        Self {
            _startup: startup,
            _listener: listener,
            _worker: worker,
            readiness,
            stop: Arc::new(AtomicBool::new(false)),
            producer,
            reader,
        }
    }

    fn refresh(
        &self,
        receiver: &mut PendingReceiver<'_>,
        port: &mut PeerRecipientPort,
        selector: Selector,
        generation: u64,
    ) -> Result<()> {
        use crate::peer_invitation::receiver_inventory::v2_pending::PendingClock;

        receiver.observe_pending(
            port,
            selector,
            generation,
            PendingClock::Candidate {
                readiness: &self.readiness,
                stop: &self.stop,
            },
            &self.producer,
        )
    }

    fn read(
        &self,
        request: crate::ipc::peer_read::Request,
    ) -> Option<crate::ipc::peer_read::Reply> {
        let generation = NonZeroU64::MIN;
        self.reader.admit(generation)?.reply_before_write(
            request,
            generation,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
    }
}

#[cfg(marmish_peer_receiver_candidate)]
async fn prove_pending_observation(
    receiver: &mut PendingReceiver<'_>,
    port: &mut PeerRecipientPort,
    exact: &HeldCoordinates,
    held: &HeldEvidence,
) {
    use crate::ipc::peer_read::{Entry, Reply, Request, Selection, State};

    let fixture = ObservationFixture::new().await;
    let selector = exact.selector();
    let entry = Entry {
        selection: Selection {
            server: selector.server,
            offer_id: selector.id,
            revision: selector.revision,
            generation: exact.generation,
        },
        state: State::Pending,
        held_at: held.held_at(),
        expires_at: held.pending_deadline().unwrap(),
    };
    let effects = Effects::read();
    let list = Request::List {};
    assert_eq!(fixture.read(list), None);
    fixture
        .refresh(receiver, port, selector, exact.generation)
        .unwrap();
    assert_eq!(
        fixture.read(list),
        Some(Reply::List {
            entries: vec![entry]
        })
    );
    assert_eq!(
        fixture.read(Request::Show {
            selection: entry.selection
        }),
        Some(Reply::Show { entry })
    );
    assert!(
        fixture
            .refresh(receiver, port, selector, exact.generation + 1)
            .is_err()
    );
    assert_eq!(fixture.read(list), None);
    fixture
        .refresh(receiver, port, selector, exact.generation)
        .unwrap();
    PendingReceiver::stop_next_observation_at_final_fence(fixture.stop.clone());
    assert!(
        fixture
            .refresh(receiver, port, selector, exact.generation)
            .is_err()
    );
    assert!(fixture.stop.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(fixture.read(list), None);
    effects.assert_since("sealed-pending-observation", (0, 0, 0));
}

async fn run_recipient(
    public: &Path,
    home: &Path,
    setup: &PublicSetup,
    step: &str,
    scenario: &str,
) {
    let mut lease = ProfileLease::acquire_ready(home, "test").unwrap();
    let material = prepare(&mut lease);
    assert_eq!(material.public_key().to_bytes(), setup.receiver);
    let lease = Arc::new(lease);
    let route = Route::configured(&setup.origin).unwrap();
    let admission = ReceiverRouteAdmission::fixture(lease.clone(), &material).unwrap();
    #[cfg(marmish_peer_receiver_candidate)]
    let (admission, fixture_cut) = if scenario == "joined" && matches!(step, "qualify" | "replay") {
        let (trip, fatal) = crate::dispatcher_fatal::Trip::new();
        let (guarded, drain) = admission.fixture_guarded(fatal).unwrap();
        (guarded, Some((trip, drain)))
    } else {
        (admission, None)
    };
    let mut port = PeerRecipientPort::open(admission, deadline())
        .await
        .unwrap();
    if step == "held" {
        drive_held(public, &lease, &material, &route, &mut port).await;
        return;
    }
    let exact: HeldCoordinates = read_public(public, "held");
    let native: NativeCoordinates = read_public(public, "native");
    let selector = exact.selector();
    let quota = ReceiverQuota::bind_receiver(&lease).unwrap();
    let mut receiver =
        PendingReceiver::open(&lease, &quota, &material, &setup.origin, &mut port).unwrap();
    let stop = AtomicBool::new(false);
    let effects = Effects::read();
    match step {
        "start" => {
            let held = receiver
                .saved_custody(&mut port, selector, exact.generation)
                .unwrap();
            assert_eq!(
                (held.state(), held.delivery_carrier()),
                (CustodyState::Held, exact.carrier)
            );
            let signed_end = held.held_deadline();
            assert!(
                receiver
                    .arm(&mut port, selector, exact.generation + 1, &stop)
                    .is_err()
            );
            assert!(
                receiver
                    .arm(
                        &mut port,
                        selector,
                        exact.generation,
                        &AtomicBool::new(true)
                    )
                    .is_err()
            );
            effects.assert_since("start-refusals", (0, 0, 0));
            if scenario == "before-native" {
                assert_eq!(
                    receiver.fixture_legacy_start(&held),
                    Err(Error::Checkpoint(CheckpointError::Layout))
                );
                effects.assert_since("legacy-start-refusal", (0, 0, 0));
                let other_lease = ProfileLease::acquire(home, "foreign").unwrap();
                assert_eq!(
                    PendingReceiver::open(
                        &other_lease,
                        &quota,
                        &material,
                        &setup.origin,
                        &mut port,
                    )
                    .err(),
                    Some(Error::Path)
                );
                effects.assert_since("foreign-lease-refusal", (0, 0, 0));
                refuse_another_stage_inode(&lease, || {
                    receiver
                        .arm(&mut port, selector, exact.generation, &stop)
                        .err()
                });
                effects.assert_since("stage-inode-refusal", (0, 0, 0));
                let unchanged = receiver
                    .saved_custody(&mut port, selector, exact.generation)
                    .unwrap();
                assert_eq!(unchanged.state(), CustodyState::Held);
                assert_eq!(unchanged.stage(), held.stage());
                assert_eq!(unchanged.signed_delivery(), held.signed_delivery());
            }
            let armed = observe_turn("preflight-and-start", signed_end, || {
                receiver
                    .arm(&mut port, selector, exact.generation, &stop)
                    .unwrap()
            });
            effects.assert_since("preflight-and-start", (0, 0, 1));
            let native_effects = Effects::read();
            assert!(
                receiver
                    .arm(&mut port, selector, exact.generation, &stop)
                    .is_err()
            );
            native_effects.assert_since("repeat-start-refusal", (0, 0, 0));
            if scenario == "capacity" {
                let original = lease.root().join("peer/provisional-unknown.db");
                let known = lease.root().join("peer/provisional-known.db");
                let bytes = fs::read(&original).unwrap();
                let stage = fs::metadata(&original).unwrap();
                let charge = stage.len().max(stage.blocks() * 512);
                // A real retained, fenced physical family contributes to the
                // inventory charge. Leave exactly the current main length
                // available so the first journal growth is refused in SQLite.
                let budget = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&known)
                    .unwrap();
                budget
                    .set_len(TOTAL_ADMISSION - store::installed::RESERVED_BYTES - charge - 1)
                    .unwrap();
                let (result, metrics) = observe_turn("journal-refusal", signed_end, || {
                    receiver.ingest(&mut port, armed, &stop).unwrap()
                });
                assert!(result.is_err());
                assert_eq!(metrics.polls, 1);
                assert!(metrics.main_opens > 0);
                assert!(metrics.rejected[1] > 0);
                assert_eq!(metrics.peak, stage.len());
                assert!(
                    fs::read(&original).unwrap() == bytes,
                    "refused join changed stage"
                );
                drop(budget);
                fs::remove_file(known).unwrap();
                println!(
                    "private-v2-pending quota_peak={} journal_refused={} main_opens={}",
                    metrics.peak, metrics.rejected[1], metrics.main_opens
                );
                native_effects.assert_since("journal-refusal", (1, 0, 0));
            } else if matches!(scenario, "joined" | "uncertain") {
                let uncertain = scenario == "uncertain";
                if uncertain {
                    fixture_lose_join_result();
                }
                let window = if uncertain {
                    "uncertain-welcome-ingest"
                } else {
                    "welcome-ingest"
                };
                let (result, metrics) = observe_turn(window, signed_end, || {
                    receiver.ingest(&mut port, armed, &stop).unwrap()
                });
                if uncertain {
                    assert_eq!(result, Err(Error::Checkpoint(CheckpointError::Source)));
                    assert!(fixture_join_result_lost());
                    let retry = Effects::read();
                    assert_eq!(
                        receiver
                            .arm(&mut port, selector, exact.generation, &stop)
                            .err(),
                        Some(Error::Binding)
                    );
                    retry.assert_since("uncertain-retry-refusal", (0, 0, 0));
                } else {
                    result.unwrap();
                }
                assert_eq!(metrics.polls, 1);
                assert!(metrics.main_opens > 0);
                assert_eq!(metrics.rejected, [0; 2]);
                assert!(metrics.peak > 0);
                native_effects.assert_since(window, (1, 1, 0));
            } else {
                assert_eq!(scenario, "before-native");
                drop(receiver);
                receiver =
                    PendingReceiver::open(&lease, &quota, &material, &setup.origin, &mut port)
                        .unwrap();
                assert_eq!(
                    receiver.ingest(&mut port, armed, &stop).err(),
                    Some(Error::Binding)
                );
                native_effects.assert_since("replacement-owner-pre-effect-cut", (0, 0, 0));
            }
            assert_eq!(
                receiver
                    .custody_state(&mut port, selector, exact.generation)
                    .unwrap(),
                (CustodyState::JoinStarted, exact.carrier)
            );
        }
        "qualify" => {
            let held = receiver
                .saved_custody(&mut port, selector, exact.generation)
                .unwrap();
            assert_eq!(
                (held.state(), held.delivery_carrier()),
                (CustodyState::JoinStarted, exact.carrier)
            );
            #[cfg(marmish_peer_receiver_candidate)]
            if scenario == "joined" {
                let fixture = ObservationFixture::new().await;
                assert!(
                    fixture
                        .refresh(&mut receiver, &mut port, selector, exact.generation)
                        .is_err()
                );
                assert_eq!(fixture.read(crate::ipc::peer_read::Request::List {}), None);
                effects.assert_since("observation-before-pending", (0, 0, 0));
            }
            if matches!(scenario, "joined" | "uncertain") {
                let joined =
                    observe_turn("joined-readback-and-pending", held.held_deadline(), || {
                        receiver
                            .qualify(&mut port, selector, exact.generation, &stop)
                            .unwrap()
                    });
                native.check(joined.coordinates());
                assert_eq!(
                    receiver
                        .custody_state(&mut port, selector, exact.generation)
                        .unwrap(),
                    (CustodyState::Pending, exact.carrier)
                );
                effects.assert_since("joined-readback-and-pending", (0, 0, 1));
            } else {
                assert!(
                    observe_turn("unjoined-readback", held.held_deadline(), || {
                        receiver.qualify(&mut port, selector, exact.generation, &stop)
                    })
                    .is_err()
                );
                assert!(
                    receiver
                        .arm(&mut port, selector, exact.generation, &stop)
                        .is_err()
                );
                assert_eq!(
                    receiver
                        .custody_state(&mut port, selector, exact.generation)
                        .unwrap(),
                    (CustodyState::JoinStarted, exact.carrier)
                );
                effects.assert_since("unjoined-readback", (0, 0, 0));
            }
        }
        "replay" => {
            if matches!(scenario, "joined" | "uncertain") {
                let pending = receiver
                    .saved_custody(&mut port, selector, exact.generation)
                    .unwrap();
                let readback = observe_turn(
                    "pending-readback",
                    pending.pending_deadline().unwrap(),
                    || {
                        receiver
                            .qualify(&mut port, selector, exact.generation, &stop)
                            .unwrap()
                    },
                );
                native.check(readback.coordinates());
                effects.assert_since("pending-readback", (0, 0, 0));
                drop(receiver);
                let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
                let cursor = fixture
                    .scan_delivery(&mut port, ScanStart::Recovery)
                    .unwrap()
                    .unwrap();
                let page = port
                    .page([cursor.lower, cursor.until], deadline())
                    .await
                    .unwrap();
                let index = page
                    .records()
                    .iter()
                    .position(|record| record.id == exact.carrier)
                    .unwrap();
                assert_eq!(
                    fixture
                        .delivery_input(&mut port, Input::History(&page, index))
                        .unwrap()
                        .status,
                    Status::Accepted(TrustedOutcome::AlreadyApplied)
                );
                assert_eq!(fixture.package_reads.get(), 0);
                drop(fixture);
                let mut receiver =
                    PendingReceiver::open(&lease, &quota, &material, &setup.origin, &mut port)
                        .unwrap();
                let after = receiver
                    .saved_custody(&mut port, selector, exact.generation)
                    .unwrap();
                assert!(
                    pending.same_saved_pending(&after),
                    "retained Pending changed during replay"
                );
                effects.assert_since("pending-and-delivery-replay", (0, 0, 0));
                if scenario == "joined" {
                    #[cfg(marmish_peer_receiver_candidate)]
                    prove_pending_observation(&mut receiver, &mut port, &exact, &after).await;
                    assert_ne!(native.identity, [0; 32]);
                    receiver
                        .fixture_change_pending_epoch(exact.carrier)
                        .unwrap();
                    let changed = receiver
                        .saved_custody(&mut port, selector, exact.generation)
                        .unwrap();
                    assert!(!pending.same_saved_pending(&changed));
                    let comparison = Effects::read();
                    assert_eq!(
                        receiver
                            .qualify(&mut port, selector, exact.generation, &stop)
                            .err(),
                        Some(Error::Checkpoint(CheckpointError::Cursor(
                            store::Error::Stale
                        )))
                    );
                    #[cfg(marmish_peer_receiver_candidate)]
                    {
                        let fixture = ObservationFixture::new().await;
                        assert_eq!(
                            fixture.refresh(&mut receiver, &mut port, selector, exact.generation),
                            Err(Error::Checkpoint(CheckpointError::Cursor(
                                store::Error::Stale
                            ))),
                        );
                        assert_eq!(fixture.read(crate::ipc::peer_read::Request::List {}), None);
                    }
                    comparison.assert_since("retained-epoch-mismatch", (0, 0, 0));
                    let still_changed = receiver
                        .saved_custody(&mut port, selector, exact.generation)
                        .unwrap();
                    assert!(changed.same_saved_pending(&still_changed));
                }
            } else {
                assert_eq!(
                    receiver
                        .custody_state(&mut port, selector, exact.generation)
                        .unwrap(),
                    (CustodyState::JoinStarted, exact.carrier)
                );
                assert!(
                    receiver
                        .qualify(&mut port, selector, exact.generation, &stop)
                        .is_err()
                );
                effects.assert_since("unjoined-replay", (0, 0, 0));
                if scenario == "before-native" {
                    port.stop(deadline()).await.unwrap();
                    let stopped = Effects::read();
                    assert_eq!(
                        receiver
                            .arm(&mut port, selector, exact.generation, &stop)
                            .err(),
                        Some(Error::Checkpoint(CheckpointError::Source))
                    );
                    stopped.assert_since("retired-port-refusal", (0, 0, 0));
                }
            }
        }
        _ => panic!("unknown Pending child step"),
    }
    #[cfg(marmish_peer_receiver_candidate)]
    if let Some((trip, drain)) = fixture_cut {
        trip.trip();
        drop(port);
        drain.reap(deadline()).await.unwrap();
    }
}

fn refuse_another_stage_inode(lease: &ProfileLease, arm: impl FnOnce() -> Option<Error>) {
    let stage = lease.root().join("peer/provisional-unknown.db");
    // Rename within the same directory to preserve the real owned image,
    // and put a byte-identical file with a different inode at its path.
    let saved = stage.with_extension("fixture-backup");
    fs::rename(&stage, &saved).unwrap();
    let bytes = fs::read(&saved).unwrap();
    let mut decoy = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&stage)
        .unwrap();
    std::io::Write::write_all(&mut decoy, &bytes).unwrap();
    drop(decoy);
    assert_ne!(
        fs::metadata(&stage).unwrap().ino(),
        fs::metadata(&saved).unwrap().ino()
    );
    let refusal = arm();
    fs::remove_file(&stage).unwrap();
    fs::rename(&saved, &stage).unwrap();
    assert_eq!(refusal, Some(Error::StageIdentityMismatch));
}

async fn drive_held(
    public: &Path,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    route: &Route,
    port: &mut PeerRecipientPort,
) {
    use consent_checkpoint::wire::ClientCarrier;
    Paths::new(lease, material).unwrap().initialize().unwrap();
    let mut fixture = Fixture::open(lease, material, route).unwrap();
    fs::write(public.join("recipient-ready"), []).unwrap();
    let live = port.wait_live(deadline()).await.unwrap().unwrap();
    let codec = fixture.owner.state.checkpoint.custody_codec();
    let raw = live.arrival().body.get().as_bytes();
    let ClientCarrier::Offer(offer) = codec.classify_client(&material.signer(), raw).unwrap()
    else {
        panic!("expected authenticated v2 Offer");
    };
    let offer = offer.qualify(&codec, &route.urls, sample()).unwrap();
    let selector = Selector::from(&offer);
    assert_eq!(
        fixture
            .delivery_input(port, Input::Live(&live))
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::Applied)
    );
    let state = &mut fixture.owner.state;
    let reservation = state
        .checkpoint
        .reserve_provisioning(&state.source, selector, sample())
        .unwrap();
    let (_, generation, ..) = reservation.stage_binding().coordinates();
    let quota = ProfileSlots::reserve(lease, store::installed::RESERVED_BYTES).unwrap();
    resolve_once(state.prepare_provisional_with_quota(&reservation, |_| Ok(()), Some(&quota)))
        .unwrap()
        .unwrap();
    fixture.queue(&reservation, sample()).unwrap();
    assert_eq!(
        port.publish_prepared_consent(&mut fixture, selector, deadline())
            .await,
        PublicationOutcome::Settled
    );
    let live = port.wait_live(deadline()).await.unwrap().unwrap();
    assert_eq!(
        fixture
            .delivery_input(port, Input::Live(&live))
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::Held)
    );
    let state = &mut fixture.owner.state;
    let held = state
        .checkpoint
        .read_custody(&state.source, selector, generation, sample())
        .unwrap()
        .unwrap();
    assert_eq!(held.state(), CustodyState::Held);
    write_public(
        public,
        "held",
        &HeldCoordinates {
            founder: selector.server.to_bytes(),
            offer: selector.id,
            revision: selector.revision,
            generation,
            carrier: held.delivery_carrier(),
        },
    );
}
