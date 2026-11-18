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
    identity::BackendChoice,
    peer_control_version::ControlVersion,
    peer_quota_vfs::dispatch::{
        ProfileSlots, ReceiverQuota, TOTAL_ADMISSION,
        founder::{Expected, FounderQuota},
        mount_for_proof, resolve_once,
    },
    peer_relay_trust::{ReceiverRouteAdmission, qualified_recipient_fixture},
    transport::{ArtiDirs, RelayPolicy, RouteMode, TransportLimits},
};

use super::*;
use crate::peer_invitation::{
    founder_ledger::{Ledger, Outbound, ledger_binding},
    receiver_inventory::v2_pending::PendingReceiver,
    socket::owner::recipient::PublicationOutcome,
    store::consent_checkpoint::{
        Status,
        provisioning::prepared_consent::custody::CustodyState,
        wire::{Codec, MAX_OUTER_BYTES},
    },
};
use store::TrustedOutcome;

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
        let deadline = Instant::now() + Duration::from_secs(45);
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
        let public = tempfile::tempdir().unwrap();
        let founder_root = public.path().join("founder-profile");
        fs::create_dir(&founder_root).unwrap();
        fs::set_permissions(&founder_root, fs::Permissions::from_mode(0o700)).unwrap();
        let mut lease = ProfileLease::acquire(&founder_root, "test").unwrap();
        let founder = prepare(&mut lease).public_key();
        drop(lease);
        let unrelated = Keys::generate().public_key();
        let (mut relay, home, lease, material) =
            qualified_recipient_fixture(founder, unrelated, Duration::from_secs(150)).await;
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
        assert_eq!(relay.all_publication_attempts(), before);
        relay.stop().await;
        println!("private-v2-pending scenario={scenario} bridge_publications=0");
    }
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
    Instant::now() + Duration::from_secs(10)
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

    let live = remote.wait_live(deadline()).await.unwrap().unwrap();
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
    let mut port = PeerRecipientPort::open(
        ReceiverRouteAdmission::fixture(lease.clone(), &material).unwrap(),
        deadline(),
    )
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
    let started = std::time::Instant::now();
    match step {
        "start" => {
            assert_eq!(
                receiver
                    .custody_state(&mut port, selector, exact.generation)
                    .unwrap(),
                (CustodyState::Held, exact.carrier)
            );
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
            let armed = receiver
                .arm(&mut port, selector, exact.generation, &stop)
                .unwrap();
            let prepare_ms = started.elapsed().as_millis();
            assert!(
                receiver
                    .arm(&mut port, selector, exact.generation, &stop)
                    .is_err()
            );
            let ingest = std::time::Instant::now();
            let polls = if scenario == "capacity" {
                let original = home.join("peer/provisional-unknown.db");
                let known = home.join("peer/provisional-known.db");
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
                let (result, metrics) = receiver.ingest(&mut port, armed, &stop).unwrap();
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
                metrics.polls
            } else if scenario == "joined" {
                let (result, metrics) = receiver.ingest(&mut port, armed, &stop).unwrap();
                result.unwrap();
                assert_eq!(metrics.polls, 1);
                assert!(metrics.main_opens > 0);
                assert_eq!(metrics.rejected, [0; 2]);
                assert!(metrics.peak > 0);
                metrics.polls
            } else {
                assert_eq!(scenario, "before-native");
                drop(armed);
                0
            };
            assert_eq!(
                receiver
                    .custody_state(&mut port, selector, exact.generation)
                    .unwrap(),
                (CustodyState::JoinStarted, exact.carrier)
            );
            println!(
                "private-v2-pending prepare_ms={prepare_ms} ingest_ms={} polls={polls} start=1 native_commit={}",
                ingest.elapsed().as_millis(),
                scenario == "joined"
            );
        }
        "qualify" => {
            assert_eq!(
                receiver
                    .custody_state(&mut port, selector, exact.generation)
                    .unwrap(),
                (CustodyState::JoinStarted, exact.carrier)
            );
            if scenario == "joined" {
                let joined = receiver
                    .qualify(&mut port, selector, exact.generation, &stop)
                    .unwrap();
                native.check(joined.coordinates());
                assert_eq!(
                    receiver
                        .custody_state(&mut port, selector, exact.generation)
                        .unwrap(),
                    (CustodyState::Pending, exact.carrier)
                );
                println!(
                    "private-v2-pending cold_readback_ms={} pending=1",
                    started.elapsed().as_millis()
                );
            } else {
                assert!(
                    receiver
                        .qualify(&mut port, selector, exact.generation, &stop)
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
            }
        }
        "replay" => {
            if scenario == "joined" {
                native.check(
                    receiver
                        .qualify(&mut port, selector, exact.generation, &stop)
                        .unwrap()
                        .coordinates(),
                );
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
                println!(
                    "private-v2-pending replay_ms={} native_ingest=0 custody_transition=0",
                    started.elapsed().as_millis()
                );
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
            }
        }
        _ => panic!("unknown Pending child step"),
    }
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
