use super::{
    socket::fixture::{Peer, Relay, deadline},
    store::consent_checkpoint::{
        Mode,
        wire::{Codec, MAX_OUTER_BYTES, Offer, Prepared},
    },
    *,
};
use crate::identity::BackendChoice;
use cgka_traits::app_components::NostrRoutingV1;
use serde_json::json;
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use tempfile::TempDir;

type Client = (TempDir, ProfileLease, IdentityMaterial);

fn client() -> Client {
    let private = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let mut profile = ProfileLease::acquire(private.path(), "client").unwrap();
    let material = profile
        .prepare(
            BackendChoice::File,
            Path::new(""),
            std::time::Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    (private, profile, material)
}

async fn open<'a>(
    client: &'a Client,
    relay: &Relay,
    policy: Policy,
    wall: Arc<AtomicU64>,
) -> (InvitationLane<'a>, Peer, String) {
    let qualified = QualifiedOrigin::for_fixture(relay, client.0.path()).unwrap();
    let (lane, (peer, subscription)) = tokio::join!(
        InvitationLane::open(
            &client.1,
            &client.2,
            policy,
            qualified,
            Wall::Fixture(wall),
            deadline()
        ),
        relay.ready(client.2.public_key()),
    );
    (lane.unwrap(), peer, subscription)
}

fn offer(
    origin: &str,
    server: &Keys,
    client: &Client,
    now: u64,
    id: u8,
) -> (Codec, Prepared<Offer>) {
    let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
    let routing = NostrRoutingV1::new([id; 32], vec![origin.to_owned()]).unwrap();
    let prepared = codec
        .prepare_offer(server, client.2.public_key(), [id; 16], &routing, now)
        .unwrap();
    (codec, prepared)
}

#[test]
fn peer_invitation_refuses_unsafe_paths_and_missing_files() {
    let client = client();
    let (profile, material) = (&client.1, &client.2);
    let paths = Paths::new(profile, material).unwrap();
    assert!(matches!(paths.cursor(), Err(Error::Path)));
    InvitationLane::initialize(profile, material).unwrap();
    let slot = StageSlot::new(0).unwrap();
    assert!(matches!(paths.stage(slot), Err(Error::Path)));
    assert!(StageSlot::new(16).is_err());
    let dir = profile.root().join("peer");
    let stage = dir.join("stage-00.db");
    let cursor = dir.join("cursor.db");

    symlink(&cursor, &stage).unwrap();
    assert!(matches!(paths.stage(slot), Err(Error::Path)));
    assert_eq!(
        InvitationLane::allocate(profile, material, slot),
        Err(Error::Path)
    );
    fs::remove_file(&stage).unwrap();

    fs::hard_link(&cursor, &stage).unwrap();
    assert!(matches!(paths.stage(slot), Err(Error::Path)));
    fs::remove_file(&stage).unwrap();

    InvitationLane::allocate(profile, material, slot).unwrap();
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(paths.stage(slot), Err(Error::Path)));
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(paths.stage(slot).is_ok());
    let sidecar = stage.with_extension("db-wal");
    symlink(&cursor, &sidecar).unwrap();
    assert!(matches!(paths.stage(slot), Err(Error::Path)));
    assert_ne!(*paths.cursor_key(), *paths.stage_key(slot));
    assert_ne!(
        *paths.stage_key(slot),
        *paths.stage_key(StageSlot::new(1).unwrap())
    );
}

#[tokio::test]
#[ignore = "private native WSS offer and consent with SQLCipher and MDK"]
async fn peer_invitation_live_manual_retry_and_recovery() {
    let client = tokio::task::spawn_blocking(client).await.unwrap();
    let (profile, material) = (&client.1, &client.2);
    let relay = Relay::new().await;
    let server = Keys::generate();
    let now = Wall::System.sample().unwrap().as_secs();
    let wall = Arc::new(AtomicU64::new(now));
    let (codec, offer) = offer(relay.url.as_str(), &server, &client, now, 21);
    let key = Selector::from(&offer.flight);
    InvitationLane::initialize(profile, material).unwrap();
    let slot = StageSlot::new(0).unwrap();
    InvitationLane::allocate(profile, material, slot).unwrap();

    let (mut lane, mut peer, _) = open(&client, &relay, Policy::default(), wall.clone()).await;
    let incoming = std::str::from_utf8(offer.frame()).unwrap();
    let (history, ()) = tokio::join!(lane.history_step(deadline()), async {
        let request = peer
            .request(material.public_key(), Some([now - 259_500, now + 300]))
            .await;
        peer.deliver(&request, incoming).await;
        peer.end(&request).await;
        peer.close(&request).await;
    });
    assert_eq!(history, Ok(true));
    let (local, _) = UnixStream::pair().unwrap();
    assert_eq!(lane.summaries(&local).unwrap(), [(key, false)]);
    assert_eq!(
        lane.publish_approved(key, slot, deadline()).await,
        Ok(PublishResult::Pending)
    );
    assert_eq!(
        fs::metadata(profile.root().join("peer/stage-00.db"))
            .unwrap()
            .len(),
        0
    );
    peer.assert_no_request().await;
    lane.approve(&local, key).unwrap();
    assert_eq!(lane.summaries(&local).unwrap(), [(key, true)]);
    let prepared = lane
        .stage_package(key, slot, Duration::from_secs(now), true)
        .await
        .unwrap();
    drop(lane);
    drop(peer);

    let (mut lane, mut peer, _) = open(&client, &relay, Policy::default(), wall.clone()).await;
    assert_eq!(
        lane.publish_approved(key, slot, Instant::now()).await,
        Err(Error::Socket(Failure::Deadline))
    );
    let (result, (first, first_bytes)) =
        tokio::join!(lane.publish_approved(key, slot, deadline()), async {
            let (event, bytes) = peer.signed_publication(material.public_key()).await;
            peer.ack(&event, false).await;
            (event, bytes)
        });
    assert_eq!(result, Ok(PublishResult::Rejected));
    let consent = codec
        .open_consent(&server, &first_bytes, &offer.flight, now)
        .unwrap();
    assert_eq!(consent.binding().event_id, prepared.id);
    assert_eq!(consent.package_bytes(), prepared.bytes.as_slice());
    assert_eq!(
        lane.publish_approved(key, slot, deadline()).await,
        Ok(PublishResult::Throttled)
    );
    peer.assert_no_request().await;

    wall.store(now + 60, Ordering::Relaxed);
    let (result, ()) = tokio::join!(lane.publish_approved(key, slot, deadline()), async {
        let (event, bytes) = peer.signed_publication(material.public_key()).await;
        assert_eq!(first.id, event.id);
        assert_eq!(first_bytes, bytes);
        peer.packet(json!(["OK", "00".repeat(32), true, "wrong carrier"]))
            .await;
    });
    assert_eq!(result, Err(Error::Socket(Failure::Ambiguous)));
    wall.store(now + 120, Ordering::Relaxed);
    assert_eq!(
        lane.publish_approved(key, slot, deadline()).await,
        Err(Error::Socket(Failure::Retired))
    );
    drop(lane);
    drop(peer);

    let (mut lane, mut peer, _) = open(&client, &relay, Policy::default(), wall.clone()).await;
    lane.checkpoint.fixture_statement("CREATE TEMP TRIGGER fail_ack BEFORE UPDATE OF acknowledged_at ON peer_consent_outbox BEGIN SELECT RAISE(ABORT, 'fixture'); END;");
    // Recovery cannot replace a missing native stage or try the wrong slot.
    assert_eq!(
        lane.publish_approved(key, StageSlot::new(1).unwrap(), deadline())
            .await,
        Err(Error::Path)
    );
    let stage = profile.root().join("peer/stage-00.db");
    let saved = profile.root().join("peer/fixture-saved-stage.db");
    fs::rename(&stage, &saved).unwrap();
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&stage)
        .unwrap()
        .write_all(&[0u8; 4096])
        .unwrap();
    assert_eq!(
        lane.publish_approved(key, slot, deadline()).await,
        Err(Error::Native(StageError::Storage))
    );
    peer.assert_no_request().await;
    fs::rename(&saved, &stage).unwrap();
    let (result, ()) = tokio::join!(lane.publish_approved(key, slot, deadline()), async {
        let event = peer.publication(&first_bytes, material.public_key()).await;
        peer.ack(&event, true).await;
    });
    assert_eq!(
        result,
        Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::Storage
        )))
    );
    drop(lane);
    drop(peer);

    wall.store(now + 180, Ordering::Relaxed);
    let (mut lane, mut peer, _) = open(&client, &relay, Policy::default(), wall.clone()).await;
    let (result, ()) = tokio::join!(lane.publish_approved(key, slot, deadline()), async {
        let event = peer.publication(&first_bytes, material.public_key()).await;
        peer.ack(&event, true).await;
    });
    assert_eq!(result, Ok(PublishResult::Accepted));
    drop(lane);
    drop(peer);

    wall.store(now + 240, Ordering::Relaxed);
    let (mut lane, mut peer, _) = open(&client, &relay, Policy::default(), wall.clone()).await;
    assert_eq!(
        lane.publish_approved(key, slot, deadline()).await,
        Ok(PublishResult::Throttled)
    );
    peer.assert_no_request().await;
    lane.discard(&local, key).unwrap();
    assert!(lane.summaries(&local).unwrap().is_empty());
}

#[tokio::test]
#[ignore = "private native WSS intake and per-turn timing"]
async fn peer_invitation_live_automatic_origin_policy_and_timings() {
    let client = tokio::task::spawn_blocking(client).await.unwrap();
    let (profile, material) = (&client.1, &client.2);
    let now = Wall::System.sample().unwrap().as_secs();
    let relay = Relay::new().await;
    let server = Keys::generate();
    let blocked = Keys::generate();
    let wall = Arc::new(AtomicU64::new(now));
    InvitationLane::initialize(profile, material).unwrap();
    let (mut lane, mut peer, subscription) = open(
        &client,
        &relay,
        Policy {
            mode: Mode::Off,
            ..Policy::default()
        },
        wall.clone(),
    )
    .await;
    let (_, off) = offer(relay.url.as_str(), &server, &client, now, 4);
    peer.deliver(&subscription, std::str::from_utf8(off.frame()).unwrap())
        .await;
    assert_eq!(lane.live_step(deadline()).await, Ok(Some(Status::Invalid)));
    drop(lane);
    drop(peer);

    let (mut lane, mut peer, subscription) = open(
        &client,
        &relay,
        Policy {
            mode: Mode::Automatic,
            blocked: [blocked.public_key()].into(),
            ..Policy::default()
        },
        wall,
    )
    .await;
    let (_, wrong_origin) = offer("wss://unserved.example/", &server, &client, now, 1);
    let (_, blocked_offer) = offer(relay.url.as_str(), &blocked, &client, now, 2);
    for invalid in [wrong_origin, blocked_offer] {
        peer.deliver(&subscription, std::str::from_utf8(invalid.frame()).unwrap())
            .await;
        assert_eq!(lane.live_step(deadline()).await, Ok(Some(Status::Invalid)));
    }
    let (codec, valid) = offer(relay.url.as_str(), &server, &client, now, 3);
    let received_at = Instant::now();
    peer.deliver(&subscription, std::str::from_utf8(valid.frame()).unwrap())
        .await;
    assert!(matches!(
        lane.live_step(deadline()).await,
        Ok(Some(Status::Accepted(_)))
    ));
    let intake_elapsed = received_at.elapsed();
    let (local, _) = UnixStream::pair().unwrap();
    let key = Selector::from(&valid.flight);
    assert_eq!(lane.summaries(&local).unwrap(), [(key, true)]);
    let slot = StageSlot::new(0).unwrap();
    InvitationLane::allocate(profile, material, slot).unwrap();
    let publication_at = Instant::now();
    let (result, (_, bytes)) = tokio::join!(
        lane.publish_approved(key, slot, Instant::now() + Duration::from_secs(20)),
        async {
            let (event, bytes) = peer.signed_publication(material.public_key()).await;
            peer.ack(&event, true).await;
            (event, bytes)
        }
    );
    assert_eq!(result, Ok(PublishResult::Accepted));
    let publication_elapsed = publication_at.elapsed();
    assert!(
        codec
            .open_consent(&server, &bytes, &valid.flight, now)
            .is_ok()
    );
    println!(
        "peer_invitation_loopback intake_us={} prepare_publish_ack_us={}",
        intake_elapsed.as_micros(),
        publication_elapsed.as_micros()
    );
}

#[tokio::test]
#[ignore = "private native WSS backoff and deferred replay"]
async fn peer_invitation_live_deferred_turn_serves_next_row() {
    let client = tokio::task::spawn_blocking(client).await.unwrap();
    let now = Wall::System.sample().unwrap().as_secs();
    let wall = Arc::new(AtomicU64::new(now));
    let relay = Relay::new().await;
    let server = Keys::generate();
    InvitationLane::initialize(&client.1, &client.2).unwrap();
    let (mut lane, mut peer, subscription) =
        open(&client, &relay, Policy::default(), wall.clone()).await;

    let mut deferred = Vec::new();
    for number in 1..=3 {
        let (_, offer) = offer(relay.url.as_str(), &server, &client, now, number);
        peer.deliver(&subscription, std::str::from_utf8(offer.frame()).unwrap())
            .await;
        let status = lane.live_step(deadline()).await.unwrap();
        if number == 1 {
            assert!(matches!(status, Some(Status::Accepted(_))));
        } else {
            assert_eq!(status, Some(Status::Deferred));
            deferred.push(offer.flight.flight().carrier_id.to_bytes());
        }
    }
    deferred.sort_unstable();
    wall.store(now + 1, Ordering::Relaxed);
    let first = lane
        .checkpoint
        .cursor
        .due(lane.origin, now + 1)
        .unwrap()
        .unwrap();
    assert_eq!(first.record().id, deferred[0]);
    assert_eq!(lane.deferred_step().unwrap(), Some(Status::Deferred));
    let next = lane
        .checkpoint
        .cursor
        .due(lane.origin, now + 1)
        .unwrap()
        .unwrap();
    assert_eq!(next.record().id, deferred[1]);
    assert_eq!(lane.deferred_step().unwrap(), Some(Status::Deferred));
    assert_eq!(lane.deferred_step().unwrap(), None);
}
