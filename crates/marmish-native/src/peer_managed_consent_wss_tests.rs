use std::os::unix::fs::FileExt as _;

use super::*;
use crate::{
    command_config::CommandConfig,
    peer_invitation::socket::owner::{
        Owner,
        recipient::{ControlOutcome, PeerRecipientPort},
    },
    peer_relay_trust::{
        ReceiverRouteAdmission, candidate_registration::ReceiverPreflight,
        qualified_manual_recipient_fixture,
    },
};

const ACK_CASE: &str = "MARMISH_MANAGED_CONSENT_ACK_CASE";

#[test]
#[ignore = "opt-in registered SQLCipher and genuine checked private WSS ACK"]
fn managed_consent_checked_ack_is_exact_and_single_use() {
    for case in ["settle", "replay", "stale", "guards", "stop"] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--nocapture",
                "--exact",
                concat!(
                    "peer_invitation::receiver_inventory::package::tests::checked_ack",
                    "::managed_consent_ack_child"
                ),
            ])
            .env_clear()
            .env("RUST_MIN_STACK", "33554432")
            .env(ACK_CASE, case)
            .spawn()
            .unwrap();
        let limit = Instant::now() + Duration::from_secs(40);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= limit {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("managed Consent ACK subprocess timed out: {case}");
            }
            std::thread::sleep(Duration::from_millis(15));
        };
        assert!(status.success(), "{case}");
    }
}

#[tokio::test]
#[ignore = "child of managed_consent_checked_ack_is_exact_and_single_use"]
async fn managed_consent_ack_child() {
    let Ok(case) = std::env::var(ACK_CASE) else {
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let founder = Keys::generate();
    let (mut relay, _temp, lease, material) = qualified_manual_recipient_fixture(
        founder.public_key(),
        Keys::generate().public_key(),
        root.path(),
    )
    .await;
    let account = material.public_key();
    let url = relay.url.as_str();
    let config = CommandConfig::load(lease.root()).unwrap();
    let registered = ReceiverPreflight::prepare(&lease, &material, &config)
        .unwrap()
        .unwrap()
        .register(&lease, &material)
        .unwrap();
    create_empty(&lease.root().join("peer/cursor.db"));
    let mut registered = registered.attach(&lease, &material).unwrap();
    let (quota, mut route) = registered.actor_parts();
    let mut owner = RecoveredOwner::open_registered(&lease, &material, quota, &mut route).unwrap();
    let mut foreign = (case == "guards")
        .then(|| RecoveredOwner::open_registered(&lease, &material, quota, &mut route).unwrap());
    let token = route.take_route(&lease, &material).unwrap();
    let (admission, _) = ReceiverRouteAdmission::registered(token).unwrap();
    let deadline = || tokio::time::Instant::now() + Duration::from_secs(15);
    let mut port = PeerRecipientPort::open(admission, deadline())
        .await
        .unwrap();

    let connector = relay.connector(lease.root(), true, None);
    let mut sender = Owner::open(&connector, url, founder.public_key(), &founder, deadline())
        .await
        .unwrap();
    let offer = wire::Codec::for_version(wire::MAX_OUTER_BYTES, ControlVersion::V2)
        .unwrap()
        .prepare_offer(
            &founder,
            account,
            [9; 16],
            &NostrRoutingV1::new([8; 32], vec![url.to_owned()]).unwrap(),
            Wall::System.sample().unwrap().as_secs(),
        )
        .unwrap();
    assert!(
        sender
            .publish(&offer, offer.flight.flight(), deadline())
            .await
            .unwrap()
    );
    let live = port.wait_live(deadline()).await.unwrap().unwrap();
    port.use_live_carrier(&live, |checked| {
        owner
            .checkpoint
            .admit_installed_custody_offer(&owner.source, checked)
    })
    .unwrap()
    .unwrap();
    let selector = Selector::from(&offer.flight);
    let (startup, listener, worker) = peer_candidate_startup::pair();
    startup.ordinary_ready();
    listener.registered();
    let ready = worker.ready().await.unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let fresh_window = |port: &mut PeerRecipientPort| {
        port.use_current_native(|scope| {
            scope.window(
                ready.readiness(),
                Arc::clone(&stopped),
                offer.flight.flight().expires_at,
            )
        })
        .unwrap()
        .unwrap()
    };
    let window = fresh_window(&mut port);
    let reservation = owner
        .checkpoint
        .reserve_provisioning(&owner.source, selector, window.sample().unwrap().as_secs())
        .unwrap();
    let started = owner
        .start_package(quota, &reservation, &window)
        .unwrap()
        .unwrap();
    let staged = owner
        .prestage_package(quota, &reservation, started)
        .unwrap();
    let artifact = owner.issue_package(quota, &reservation, staged).unwrap();
    let window = fresh_window(&mut port);
    let wall = || window.sample().unwrap().as_secs();
    let proof = owner
        .attest_package(quota, &reservation, Some(artifact), &window)
        .unwrap();
    let queued = owner
        .queue_managed_consent(quota, &reservation, &proof, &window)
        .unwrap();
    let turn = owner
        .reserve_managed_consent(quota, queued, &window)
        .unwrap()
        .unwrap();
    assert_eq!(turn.attempt().sequence(), 1);
    let stage = lease.root().join("peer/provisional-unknown.db");
    let cursor = lease.root().join("peer/cursor.db");
    let original = fs::read(&stage).unwrap();
    let last = original.len() - 1;
    let drift = |changed| {
        let byte = original[last] ^ u8::from(changed);
        OpenOptions::new()
            .write(true)
            .open(&stage)
            .unwrap()
            .write_all_at(&[byte], u64::try_from(last).unwrap())
            .unwrap();
    };
    if let Some(foreign) = &mut foreign {
        let before = fs::read(&cursor).unwrap();
        let floors = owner.checkpoint.consent_fixture_clocks();
        let queued = owner
            .checkpoint
            .read_managed_queued(&owner.source, account, url, selector, wall())
            .unwrap()
            .unwrap();
        assert!(matches!(
            foreign.checkpoint.reserve_managed_consent(
                &foreign.source,
                queued,
                &proof,
                wall(),
                || panic!("foreign reserve reached commit")
            ),
            Err(CheckpointError::Package)
        ));
        drift(true);
        assert!(matches!(
            owner.managed_before_flush(quota, &turn),
            Err(Error::StageIdentityMismatch)
        ));
        drift(false);
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), floors);
        assert_eq!(fs::read(&cursor).unwrap(), before);
    }

    let flush_started = Instant::now();
    let ControlOutcome::Accepted(ack) = port
        .fixture_publish_managed_attempt(turn.attempt(), deadline(), |_| {
            owner.managed_before_flush(quota, &turn)
        })
        .await
    else {
        panic!("expected actual checked ACK for the managed attempt");
    };
    eprintln!(
        "managed_consent_wss acknowledged_ms={:.3}",
        flush_started.elapsed().as_secs_f64() * 1000.0
    );
    let carrier = turn.attempt().carrier();
    if case == "settle" {
        let point = Instant::now();
        let settled = port
            .use_accepted(&ack, |view| owner.settle_managed_consent(quota, turn, view))
            .unwrap()
            .unwrap();
        let (_, settled_selector, settled_carrier, _) = settled.consume();
        assert!(settled_selector == selector);
        assert_eq!(settled_carrier, carrier);
        eprintln!(
            "managed_consent_wss settled_ms={:.3}",
            point.elapsed().as_secs_f64() * 1000.0
        );
        let baseline = fs::read(&cursor).unwrap();
        let clocks = owner.checkpoint.consent_fixture_clocks();
        let queued = owner
            .checkpoint
            .read_managed_queued(&owner.source, account, url, selector, wall())
            .unwrap()
            .unwrap();
        assert!(
            owner
                .reserve_managed_consent(quota, queued, &window)
                .unwrap()
                .is_none()
        );
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), clocks);
        assert_eq!(fs::read(&cursor).unwrap(), baseline);
    } else if case == "replay" {
        let before = fs::read(&cursor).unwrap();
        let floors = owner.checkpoint.consent_fixture_clocks();
        let cut = port
            .use_accepted(&ack, |view| {
                owner
                    .checkpoint
                    .settle_managed_consent(&owner.source, turn.attempt(), view, || {
                        Err(CheckpointError::Expired)
                    })
            })
            .unwrap();
        assert!(matches!(cut, Err(CheckpointError::Expired)));
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), floors);
        assert_eq!(fs::read(&cursor).unwrap(), before);
        let settled = port
            .use_accepted(&ack, |view| {
                owner
                    .checkpoint
                    .settle_managed_consent(&owner.source, turn.attempt(), view, || Ok(()))
            })
            .unwrap()
            .unwrap();
        let (_, _, settled_carrier, _) = settled.consume();
        assert_eq!(settled_carrier, carrier);
        let after = fs::read(&cursor).unwrap();
        let floors = owner.checkpoint.consent_fixture_clocks();
        let replay = port
            .use_accepted(&ack, |view| {
                owner
                    .checkpoint
                    .settle_managed_consent(&owner.source, turn.attempt(), view, || Ok(()))
            })
            .unwrap();
        assert!(replay.is_err());
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), floors);
        assert_eq!(fs::read(&cursor).unwrap(), after);
    } else if case == "stop" {
        let before = fs::read(&cursor).unwrap();
        let floors = owner.checkpoint.consent_fixture_clocks();
        owner.managed_before_flush(quota, &turn).unwrap();
        stopped.store(true, std::sync::atomic::Ordering::Release);
        assert!(owner.managed_before_flush(quota, &turn).is_err());
        assert!(
            port.use_accepted(&ack, |view| owner.settle_managed_consent(quota, turn, view))
                .unwrap()
                .is_err()
        );
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), floors);
        assert_eq!(fs::read(&cursor).unwrap(), before);
    } else if case == "guards" {
        let foreign = foreign.as_mut().unwrap();
        let before = fs::read(&cursor).unwrap();
        let floors = owner.checkpoint.consent_fixture_clocks();
        let foreign_ack = port
            .use_accepted(&ack, |view| {
                foreign.checkpoint.settle_managed_consent(
                    &foreign.source,
                    turn.attempt(),
                    view,
                    || panic!("foreign ACK reached commit"),
                )
            })
            .unwrap();
        assert!(matches!(foreign_ack, Err(CheckpointError::LocalOwner)));
        assert_eq!(
            port.use_accepted(&ack, |view| view
                .fixture_at_attempt_deadline(turn.attempt(), url))
                .unwrap(),
            Err(CheckpointError::Expired)
        );
        drift(true);
        assert!(matches!(
            port.use_accepted(&ack, |view| owner.settle_managed_consent(quota, turn, view))
                .unwrap(),
            Err(Error::StageIdentityMismatch)
        ));
        drift(false);
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), floors);
        assert_eq!(fs::read(&cursor).unwrap(), before);
        assert_eq!(
            port.fixture_late_acknowledgement(ack),
            Err(crate::peer_invitation::socket::Failure::Deadline)
        );
    } else {
        let queued = owner
            .checkpoint
            .read_managed_queued(&owner.source, account, url, selector, wall())
            .unwrap()
            .unwrap();
        let proof = owner
            .attest_queued_package(quota, &queued, &window)
            .unwrap();
        // Cross just the persisted retry threshold; this does not move the
        // physical port's independent clock or issue a second frame.
        let retry_wall = queued.fixture_retry_at().unwrap();
        let second = owner
            .checkpoint
            .reserve_managed_consent(&owner.source, queued, &proof, retry_wall, || Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(second.sequence(), 2);
        let baseline = fs::read(&cursor).unwrap();
        let clocks = owner.checkpoint.consent_fixture_clocks();
        assert!(matches!(
            port.use_accepted(&ack, |view| view.exact_attempt(&second, url))
                .unwrap(),
            Err(CheckpointError::Source)
        ));
        assert!(
            port.use_accepted(&ack, |view| {
                owner.settle_managed_consent(quota, turn, view)
            })
            .map(|outcome| outcome.is_err())
            .unwrap()
        );
        assert_eq!(owner.checkpoint.consent_fixture_clocks(), clocks);
        assert_eq!(fs::read(&cursor).unwrap(), baseline);
    }
    assert_eq!(fs::read(&stage).unwrap(), original);
    assert_eq!(
        relay
            .peer_transcript()
            .iter()
            .filter(|row| row.event.to_bytes() == carrier)
            .count(),
        1
    );
    relay.stop().await;
}
