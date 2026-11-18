use super::*;
use crate::peer_invitation::{
    ReceivePolicy, Source,
    store::consent_checkpoint::{
        Error, Mode,
        provisioning::prepared_consent::custody::offer_ingress::{OfferIngress, fault_fixture},
    },
};
use storage_sqlite::{SqlCipherHardening, SqlCipherKey, open_hardened_sqlcipher};

const URL: &str = "wss://peer.example/";
const KEY: [u8; 32] = [35; 32];

fn open_custody(
    path: &Path,
    key: [u8; 32],
    signer: &Keys,
    url: &str,
    policy: Policy,
    mode: ReceivePolicy,
) -> Checkpoint {
    let conn = HardenedPeerConnection::open_installed(path, key).unwrap();
    // SAFETY: reads the effective UID of this test process.
    Checkpoint::attach_installed_custody(
        conn,
        signer.clone(),
        unsafe { libc::geteuid() },
        url,
        policy,
        mode,
    )
    .unwrap()
}

fn initialize(path: &Path, key: [u8; 32], recipient: &Keys, url: &str) {
    let conn = HardenedPeerConnection::open_installed(path, key).unwrap();
    // SAFETY: reads the effective UID of this test process.
    Checkpoint::attach_installed_v2(
        conn,
        recipient.clone(),
        unsafe { libc::geteuid() },
        url,
        Policy::default(),
    )
    .unwrap();
}

fn sql(path: &Path, key: [u8; 32]) -> rusqlite::Connection {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .unwrap();
    open_hardened_sqlcipher(
        &conn,
        &SqlCipherKey::raw(key),
        SqlCipherHardening::live_cache(),
    )
    .unwrap();
    conn
}

fn observe(conn: &rusqlite::Connection) -> Vec<i64> {
    conn.query_row(
        "SELECT revision, retry_revision, wall,
            (SELECT wall FROM peer_consent_version),
            (SELECT count(*) FROM peer_cursor_deferred),
            (SELECT count(*) FROM peer_installed_offer_v2),
            (SELECT count(*) FROM peer_consent_fence),
            (SELECT count(*) FROM peer_consent_outbox),
            (SELECT count(*) FROM peer_prepared_consent_outbox),
            (SELECT count(*) FROM peer_pending_custody),
            (SELECT count(*) FROM peer_provisioning_slot WHERE server IS NOT NULL)
         FROM peer_cursor_origin",
        [],
        |row| (0..11).map(|column| row.get(column)).collect(),
    )
    .unwrap()
}

fn saved(conn: &rusqlite::Connection) -> Vec<(Vec<u8>, Vec<u8>)> {
    conn.prepare(
        "SELECT i.carrier, i.frame FROM peer_consent_invite i
         JOIN peer_installed_offer_v2 v USING (server, origin, offer_id, revision)
         WHERE v.version=2 AND i.reserved=0 AND i.decision_at IS NULL
         ORDER BY i.carrier",
    )
    .unwrap()
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .unwrap()
    .collect::<rusqlite::Result<Vec<_>>>()
    .unwrap()
}

fn status(result: OfferIngress) -> Status {
    match result {
        OfferIngress::Recorded(disposition) => {
            let _receipt = disposition.receipt;
            disposition.status
        }
        _ => panic!("expected durable Offer receipt"),
    }
}

fn synthetic<'a>(
    prepared: &'a Prepared<Offer>,
    account: PublicKey,
    origin: &'a str,
    wall: u64,
) -> CheckedCarrier<'a> {
    let event: Event = serde_json::from_slice(prepared.frame()).unwrap();
    CheckedCarrier {
        account,
        origin,
        wall,
        record: crate::peer_invitation::RawRecord {
            id: event.id.to_bytes(),
            stamp: event.created_at.as_secs(),
            bytes: prepared.frame(),
        },
    }
}

fn temp_cursor() -> (tempfile::TempDir, std::path::PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("cursor.db");
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    (temp, path)
}

#[test]
fn peer_installed_custody_offer_outer_self_and_wrong_recipient_fail_at_port_index() {
    let recipient = Keys::generate();
    let founder = Keys::generate();
    let route = NostrRoutingV1::new([3; 32], vec![URL.to_owned()]).unwrap();

    let addressed = offer(&founder, recipient.public_key(), &route, ControlVersion::V2);
    let event: Event = serde_json::from_slice(addressed.frame()).unwrap();
    let self_carrier = EventBuilder::new(event.kind, event.content)
        .tags(event.tags)
        .custom_created_at(event.created_at)
        .sign_with_keys(&recipient)
        .unwrap();
    let body = serde_json::value::to_raw_value(&self_carrier).unwrap();
    assert!(matches!(
        Arrival::index(&body, &recipient.public_key().to_bytes()),
        Err(Failure::Grammar)
    ));

    let another = Keys::generate().public_key();
    let unaddressed = offer(&founder, another, &route, ControlVersion::V2);
    let body =
        RawValue::from_string(String::from_utf8(unaddressed.frame().to_vec()).unwrap()).unwrap();
    assert!(matches!(
        Arrival::index(&body, &recipient.public_key().to_bytes()),
        Err(Failure::Grammar)
    ));
}

#[test]
fn peer_installed_custody_offer_refusals_leave_state_unchanged() {
    let (_temp, path) = temp_cursor();
    let recipient = Keys::generate();
    let founder = Keys::generate();
    let route = NostrRoutingV1::new([3; 32], vec![URL.to_owned()]).unwrap();
    let frame = offer(&founder, recipient.public_key(), &route, ControlVersion::V2);
    let wall = frame.flight.flight().created_at;
    initialize(&path, KEY, &recipient, URL);
    let policy = Policy {
        mode: Mode::Off,
        reserved: [founder.public_key()].into(),
        blocked: Default::default(),
    };
    let mut checkpoint = open_custody(&path, KEY, &recipient, URL, policy, ReceivePolicy::Manual);
    let source = checkpoint.bind_source(URL).unwrap();
    let conn = sql(&path, KEY);
    let before = observe(&conn);
    let check = |checkpoint: &mut Checkpoint, source: &Source, account, origin, now| {
        checkpoint.admit_installed_custody_offer(source, synthetic(&frame, account, origin, now))
    };
    let foreign = Keys::generate().public_key();
    assert!(matches!(
        check(&mut checkpoint, &source, foreign, URL, wall),
        Err(Error::Source)
    ));
    assert!(matches!(
        check(
            &mut checkpoint,
            &source,
            recipient.public_key(),
            "wss://other.example/",
            wall
        ),
        Err(Error::Source)
    ));
    let (_different, other_path) = temp_cursor();
    initialize(&other_path, KEY, &recipient, URL);
    let other_checkpoint = open_custody(
        &other_path,
        KEY,
        &recipient,
        URL,
        Policy::default(),
        ReceivePolicy::Manual,
    );
    let foreign_source = other_checkpoint.bind_source(URL).unwrap();
    assert!(matches!(
        check(
            &mut checkpoint,
            &foreign_source,
            recipient.public_key(),
            URL,
            wall
        ),
        Err(Error::Source)
    ));

    let other_route =
        NostrRoutingV1::new([3; 32], vec!["wss://other.example/".to_owned()]).unwrap();
    let wrong_route = offer(
        &founder,
        recipient.public_key(),
        &other_route,
        ControlVersion::V2,
    );
    assert!(matches!(
        checkpoint.admit_installed_custody_offer(
            &source,
            synthetic(&wrong_route, recipient.public_key(), URL, wall)
        ),
        Ok(OfferIngress::Invalid)
    ));
    assert!(matches!(
        check(
            &mut checkpoint,
            &source,
            recipient.public_key(),
            URL,
            frame.flight.flight().expires_at
        ),
        Ok(OfferIngress::Invalid)
    ));
    let legacy = offer(&founder, recipient.public_key(), &route, ControlVersion::V1);
    assert!(matches!(
        checkpoint.admit_installed_custody_offer(
            &source,
            synthetic(&legacy, recipient.public_key(), URL, wall)
        ),
        Ok(OfferIngress::NotOffer)
    ));
    let mut mismatched = synthetic(&frame, recipient.public_key(), URL, wall);
    mismatched.record.id[0] ^= 1;
    assert!(matches!(
        checkpoint.admit_installed_custody_offer(&source, mismatched),
        Err(Error::Index)
    ));
    assert_eq!(observe(&conn), before);

    conn.execute(
        "UPDATE peer_cursor_origin SET wall=?1",
        [i64::try_from(wall + 1).unwrap()],
    )
    .unwrap();
    let future = observe(&conn);
    assert!(matches!(
        check(&mut checkpoint, &source, recipient.public_key(), URL, wall),
        Err(Error::Cursor(
            crate::peer_invitation::store::Error::ClockRegressed
        ))
    ));
    assert_eq!(observe(&conn), future);
    conn.execute(
        "UPDATE peer_cursor_origin SET wall=0, quarantine_until=?1",
        [i64::try_from(wall + 60).unwrap()],
    )
    .unwrap();
    let quarantined = observe(&conn);
    assert!(matches!(
        check(&mut checkpoint, &source, recipient.public_key(), URL, wall),
        Err(Error::Cursor(
            crate::peer_invitation::store::Error::RelayContract
        ))
    ));
    assert_eq!(observe(&conn), quarantined);
    conn.execute("UPDATE peer_cursor_origin SET quarantine_until=NULL", [])
        .unwrap();

    assert_eq!(
        status(check(&mut checkpoint, &source, recipient.public_key(), URL, wall).unwrap()),
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert_eq!(
        saved(&conn),
        vec![(
            frame.flight.flight().carrier_id.to_bytes().to_vec(),
            frame.frame().to_vec()
        )]
    );
    let conflict = offer(&founder, recipient.public_key(), &route, ControlVersion::V2);
    let after = observe(&conn);
    assert!(matches!(
        checkpoint.admit_installed_custody_offer(
            &source,
            synthetic(&conflict, recipient.public_key(), URL, wall + 1)
        ),
        Ok(OfferIngress::Backpressure)
    ));
    assert_eq!(observe(&conn), after);
    drop(checkpoint);

    for mode in [
        ReceivePolicy::Off,
        ReceivePolicy::AutoAny,
        ReceivePolicy::AutoFounders(Default::default()),
    ] {
        let mut checkpoint = open_custody(&path, KEY, &recipient, URL, Policy::default(), mode);
        let source = checkpoint.bind_source(URL).unwrap();
        assert!(matches!(
            check(&mut checkpoint, &source, recipient.public_key(), URL, wall),
            Err(Error::Policy)
        ));
        assert_eq!(observe(&conn), after);
    }
    let blocked = Policy {
        blocked: [founder.public_key()].into(),
        ..Policy::default()
    };
    let mut checkpoint = open_custody(&path, KEY, &recipient, URL, blocked, ReceivePolicy::Manual);
    let source = checkpoint.bind_source(URL).unwrap();
    assert!(matches!(
        check(&mut checkpoint, &source, recipient.public_key(), URL, wall),
        Ok(OfferIngress::Invalid)
    ));
    assert_eq!(observe(&conn), after);
}

#[test]
fn peer_installed_custody_offer_gates_cooldown_and_rolls_back_cap_refusal() {
    let (_temp, path) = temp_cursor();
    let recipient = Keys::generate();
    let route = NostrRoutingV1::new([3; 32], vec![URL.to_owned()]).unwrap();
    initialize(&path, KEY, &recipient, URL);
    let mut checkpoint = open_custody(
        &path,
        KEY,
        &recipient,
        URL,
        Policy::default(),
        ReceivePolicy::Manual,
    );
    let source = checkpoint.bind_source(URL).unwrap();
    let conn = sql(&path, KEY);
    let founder = Keys::generate();
    let cooled = offer(&founder, recipient.public_key(), &route, ControlVersion::V2);
    let wall = cooled.flight.flight().created_at;
    fault_fixture::apply_cooldown(&mut checkpoint, founder.public_key(), wall);
    let outcome = checkpoint
        .admit_installed_custody_offer(
            &source,
            synthetic(&cooled, recipient.public_key(), URL, wall),
        )
        .unwrap();
    assert_eq!(
        status(outcome),
        Status::Accepted(TrustedOutcome::SkippedCooldown)
    );
    assert_eq!(&observe(&conn)[4..], &[0; 7]);

    fault_fixture::limit_to_current_pages(&checkpoint);
    for _ in 0..8 {
        let founder = Keys::generate();
        let frame = offer(&founder, recipient.public_key(), &route, ControlVersion::V2);
        let before = observe(&conn);
        let retained = saved(&conn);
        let outcome = checkpoint.admit_installed_custody_offer(
            &source,
            synthetic(&frame, recipient.public_key(), URL, wall + 1),
        );
        match outcome {
            Err(Error::Cursor(crate::peer_invitation::store::Error::Storage)) => {
                assert_eq!(observe(&conn), before);
                assert_eq!(saved(&conn), retained);
                return;
            }
            Ok(OfferIngress::Recorded(result)) => {
                assert_eq!(result.status, Status::Accepted(TrustedOutcome::Applied));
            }
            _ => panic!("expected a bounded write or rolled-back storage refusal"),
        }
    }
    panic!("test cursor never attempted to grow beyond its current page count");
}

#[tokio::test]
#[ignore = "uses a disposable private authenticated WSS relay and capped SQLCipher"]
async fn peer_installed_custody_offer_checked_history_live_and_cold_recovery() {
    let founder = Keys::generate();
    let second = Keys::generate();
    let (relay, _root, lease, material) = qualified_recipient_fixture(
        founder.public_key(),
        second.public_key(),
        Duration::from_secs(90),
    )
    .await;
    let recipient = material.signer();
    let url = relay.url.as_str();
    let route = NostrRoutingV1::new([3; 32], vec![url.to_owned()]).unwrap();
    let paths = Paths::new(&lease, &material).unwrap();
    paths.initialize().unwrap();
    let checked = paths.cursor().unwrap();
    let key = *paths.cursor_key();
    initialize(checked.path(), key, &recipient, url);
    let mut checkpoint = open_custody(
        checked.path(),
        key,
        &recipient,
        url,
        Policy {
            mode: Mode::Automatic,
            reserved: [founder.public_key(), second.public_key()].into(),
            blocked: Default::default(),
        },
        ReceivePolicy::Manual,
    );
    checked.unchanged().unwrap();
    let source = checkpoint.bind_source(url).unwrap();

    let connector = relay.connector(lease.root(), true, None);
    let mut sender = Owner::open(
        &connector,
        url,
        founder.public_key(),
        &founder,
        time_limit(),
    )
    .await
    .unwrap();
    let past = offer(&founder, recipient.public_key(), &route, ControlVersion::V2);
    assert!(
        sender
            .publish(&past, past.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let admission = || ReceiverRouteAdmission::fixture(lease.clone(), &material).unwrap();
    let mut port = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    let page = port
        .page([wall_now() - 172_900, wall_now()], time_limit())
        .await
        .unwrap();

    assert!(matches!(
        port.use_page_carrier(&page, 1, |_| panic!("out-of-range page index exposed")),
        Err(Failure::Binding)
    ));
    let mut other = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    assert!(matches!(
        other.use_page_carrier(&page, 0, |_| panic!("foreign page exposed")),
        Err(Failure::Binding)
    ));
    other.stop(time_limit()).await.unwrap();
    let mut received = Vec::new();
    for expected in [TrustedOutcome::Applied, TrustedOutcome::AlreadyApplied] {
        let result = port
            .use_page_carrier(&page, 0, |carrier| {
                if expected == TrustedOutcome::Applied {
                    received.push(observed_record(&carrier, &past));
                }
                checkpoint.admit_installed_custody_offer(&source, carrier)
            })
            .unwrap()
            .unwrap();
        assert_eq!(status(result), Status::Accepted(expected));
    }

    let live_connector = relay.connector(lease.root(), true, None);
    let mut live_sender = Owner::open(
        &live_connector,
        url,
        second.public_key(),
        &second,
        time_limit(),
    )
    .await
    .unwrap();
    let live = offer(&second, recipient.public_key(), &route, ControlVersion::V2);
    assert!(
        live_sender
            .publish(&live, live.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let record = port.wait_live(time_limit()).await.unwrap().unwrap();
    assert_eq!(
        status(
            port.use_live_carrier(&record, |carrier| {
                received.push(observed_record(&carrier, &live));
                checkpoint.admit_installed_custody_offer(&source, carrier)
            })
            .unwrap()
            .unwrap()
        ),
        Status::Accepted(TrustedOutcome::Applied)
    );
    port.stop(time_limit()).await.unwrap();
    assert!(matches!(
        port.use_live_carrier(&record, |_| panic!("retired live record exposed")),
        Err(Failure::Binding)
    ));

    drop(checkpoint);
    let reopened = open_custody(
        checked.path(),
        key,
        &recipient,
        url,
        Policy::default(),
        ReceivePolicy::Manual,
    );
    checked.unchanged().unwrap();
    let conn = sql(checked.path(), key);
    assert_eq!(received.len(), 2);
    received.sort_unstable();
    assert!(
        saved(&conn) == received,
        "retained history/live bytes differ"
    );
    let state = observe(&conn);
    assert_eq!(&state[..2], &[0, 0]);
    assert_eq!(&state[4..], &[0, 2, 2, 0, 0, 0, 0]);
    drop(reopened);
}

fn observed_record(carrier: &CheckedCarrier<'_>, expected: &Prepared<Offer>) -> (Vec<u8>, Vec<u8>) {
    let observed = carrier.record();
    let expected_json: serde_json::Value = serde_json::from_slice(expected.frame()).unwrap();
    let observed_json: serde_json::Value = serde_json::from_slice(observed.bytes).unwrap();
    // A relay can reserialize JSON. Deduplication and durable custody use the
    // exact bytes observed by this authenticated consumer.
    assert!(expected_json == observed_json, "signed relay event differs");
    assert_eq!(observed.id, expected.flight.flight().carrier_id.to_bytes());
    (observed.id.to_vec(), observed.bytes.to_vec())
}
