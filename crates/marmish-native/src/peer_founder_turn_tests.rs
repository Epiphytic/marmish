use super::*;
use crate::{
    identity::BackendChoice,
    peer_control_version::ControlVersion,
    peer_quota_vfs::dispatch::founder::{Expected, FounderQuota},
};
use cgka_traits::app_components::NostrRoutingV1;
use rusqlite::Connection;
use std::{
    io::Read as _,
    num::NonZeroU64,
    path::Path,
    process::{Command, Stdio},
};
use store::consent_checkpoint::wire::{Codec, MAX_OUTER_BYTES};

const CHILD: &str = "peer_invitation::founder_turn_tests::founder_native_child";
const CHILD_FLAG: &str = "MARMISH_FOUNDER_NATIVE_CHILD";
const CHILD_ROOT: &str = "MARMISH_FOUNDER_NATIVE_ROOT";
const COLD_CHILD: &str = "peer_invitation::founder_turn_tests::founder_cold_child";
const ORIGIN: &str = "wss://127.0.0.1:9/";

#[test]
#[ignore = "isolated real SQLCipher and mainline MDK proof"]
fn founder_native_charged_create_withdraw_and_cold_inspection() {
    isolated_child(CHILD, "owner-conflict", "founder-native ", None);
    isolated_child(CHILD, "receiver-conflict", "founder-native ", None);
    isolated_child(CHILD, "1", "founder-native ", None);
}

fn isolated_child(exact: &str, mode: &str, prefix: &str, handoff: Option<&Path>) {
    struct Child(std::process::Child);

    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let mut command = Command::new("/proc/self/exe");
    command
        .args(["--ignored", "--exact", exact, "--nocapture"])
        .env_clear()
        .env(CHILD_FLAG, mode)
        .stdout(Stdio::piped());
    if let Some(root) = handoff {
        command.env(CHILD_ROOT, root);
    }
    let mut child = Child(command.spawn().unwrap());
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "founder_native_timeout");
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut report = String::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .take(4097)
        .read_to_string(&mut report)
        .unwrap();
    assert!(report.len() < 4097);
    if exact == WSS_CHILD && mode == "mid-native-create" {
        assert_eq!(status.code(), Some(80), "{report}");
    } else {
        assert!(status.success(), "{report}");
    }
    let line = report
        .lines()
        .find(|line| line.starts_with(prefix))
        .unwrap();
    println!("{line}");
}

#[test]
#[ignore = "run only through the isolated proof parent"]
fn founder_native_child() {
    crate::peer_quota_vfs::dispatch::mount_for_proof().unwrap();
    let mode = std::env::var(CHILD_FLAG).unwrap();
    assert!(matches!(
        mode.as_str(),
        "1" | "owner-conflict" | "receiver-conflict"
    ));
    let temp = tempfile::tempdir().unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut lease = ProfileLease::acquire(temp.path(), "founder").unwrap();
    let material = lease
        .prepare(
            BackendChoice::File,
            Path::new("/unused"),
            std::time::Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let lease = Arc::new(lease);
    let paths = Paths::new(&lease, &material).unwrap();
    paths.initialize_founder_directory().unwrap();
    use crate::peer_quota_vfs::dispatch::{ProfileSlots, ReceiverQuota};
    if mode == "owner-conflict" {
        let _receiver = ProfileSlots::reserve(&lease, 1).unwrap();
        assert!(matches!(
            FounderQuota::mount(&lease),
            Err("founder_other_owner")
        ));
        println!("founder-native receiver-owner-conflict=true");
        return;
    }
    if mode == "receiver-conflict" {
        let _receiver = ReceiverQuota::bind_receiver(&lease).unwrap();
        assert!(matches!(
            FounderQuota::mount(&lease),
            Err("founder_other_owner")
        ));
        println!("founder-native installed-receiver-conflict=true");
        return;
    }
    let mut quota = FounderQuota::mount(&lease).unwrap();
    assert!(matches!(
        ProfileSlots::reserve(&lease, 0),
        Err("dispatch_founder_owner")
    ));
    assert!(matches!(
        ReceiverQuota::bind_receiver(&lease),
        Err("dispatch_founder_owner")
    ));
    assert!(quota.owns(&lease));
    let generation = NonZeroU64::new(1).unwrap();
    let file = quota.main(generation);
    assert!(Connection::open(&file).is_err());
    assert!(!file.exists());
    for suffix in ["-journal", "-wal", "-shm"] {
        let sidecar = file.with_file_name(format!("founder-1.db{suffix}"));
        assert!(Connection::open(&sidecar).is_err());
        assert!(!sidecar.exists());
    }
    let other_slot = file.with_file_name("provisional-known.db");
    paths.create_founder_file(&other_slot).unwrap();
    assert!(quota.inventory(None).is_err());
    std::fs::remove_file(other_slot).unwrap();

    let ordinary_db = lease.root().join("unrelated-proof.db");
    let ordinary = Connection::open(ordinary_db).unwrap();
    ordinary
        .execute_batch("CREATE TABLE ordinary(value INTEGER); INSERT INTO ordinary VALUES (17);")
        .unwrap();
    assert_eq!(
        ordinary
            .query_row("SELECT value FROM ordinary", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        17
    );
    drop(ordinary);

    let ledger_path = quota.ledger();
    paths.create_founder_file(&ledger_path).unwrap();
    use super::founder_ledger::{Ledger, Outbound, Step, ledger_binding};
    let founder = material.signer();
    let recipient = Arc::new(Keys::generate());
    let mut ledger = Ledger::open(
        &ledger_path,
        *paths.founder_ledger_key(),
        ledger_binding(founder.public_key(), recipient.public_key(), ORIGIN),
        generation,
        true,
    )
    .unwrap();
    let room = [17; 16];
    let slot = [18; 16];
    let routing = NostrRoutingV1::new([19; 32], vec![ORIGIN.to_owned()]).unwrap();
    let recipient_db = tempfile::NamedTempFile::new_in(temp.path()).unwrap();
    let sample = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let native_package = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            OpenStage::open(
                recipient_db.path(),
                [21; 32],
                recipient.clone(),
                ORIGIN,
                hex::encode(slot),
            )
            .unwrap()
            .package(sample, true)
            .await
            .unwrap()
        });
    let codec = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2).unwrap();
    let offer = codec
        .prepare_offer(
            &founder,
            recipient.public_key(),
            room,
            &routing,
            sample.as_secs(),
        )
        .unwrap();
    let opened_offer = codec
        .open_offer(
            &recipient,
            offer.frame(),
            Some(founder.public_key()),
            &routing.relays,
            sample.as_secs(),
        )
        .unwrap();
    let consent = codec
        .prepare_consent(
            &recipient,
            &opened_offer,
            &native_package.bytes,
            sample.as_secs(),
            sample.as_secs() + 60,
        )
        .unwrap();
    let verified = codec
        .open_consent(&founder, consent.frame(), &offer.flight, sample.as_secs())
        .unwrap();
    let alternate_offer = codec
        .prepare_offer(
            &founder,
            recipient.public_key(),
            [21; 16],
            &routing,
            sample.as_secs(),
        )
        .unwrap();
    assert!(
        codec
            .open_consent(
                &founder,
                consent.frame(),
                &alternate_offer.flight,
                sample.as_secs()
            )
            .is_err()
    );
    assert!(
        codec
            .open_consent(
                &founder,
                consent.frame(),
                &offer.flight,
                consent.flight.flight().expires_at + 600
            )
            .is_err()
    );
    let legacy = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V1).unwrap();
    let v1_offer = legacy
        .prepare_offer(
            &founder,
            recipient.public_key(),
            room,
            &routing,
            sample.as_secs(),
        )
        .unwrap();
    let v1_consent = legacy
        .prepare_consent(
            &recipient,
            &v1_offer.flight,
            &native_package.bytes,
            sample.as_secs(),
            sample.as_secs() + 60,
        )
        .unwrap();
    assert!(
        codec
            .open_consent(
                &founder,
                v1_consent.frame(),
                &offer.flight,
                sample.as_secs()
            )
            .is_err()
    );
    let alternate_consent = codec
        .prepare_consent(
            &recipient,
            &alternate_offer.flight,
            &native_package.bytes,
            sample.as_secs(),
            sample.as_secs() + 60,
        )
        .unwrap();
    assert!(
        codec
            .prepare_consent(
                &recipient,
                &offer.flight,
                &native_package.bytes[..32],
                sample.as_secs(),
                sample.as_secs() + 60
            )
            .is_err()
    );
    assert_eq!(
        crate::peer_quota_vfs::dispatch::resolve_once(std::future::pending::<()>()),
        Err("dispatch_pending")
    );
    let native = Expected {
        signer: founder.clone(),
        recipient: recipient.public_key(),
        routing: &routing,
        room,
        reference: &verified.binding().reference,
    };
    let flight = offer.flight.flight();
    ledger.claim(&offer).unwrap();
    ledger
        .arm(Outbound::Offer, flight, offer.frame(), sample.as_secs())
        .unwrap();
    // Simulated Offer settlement only for the standalone, no-network native proof.
    ledger
        .terminal(Outbound::Offer, Step::OfferAccepted)
        .unwrap();
    assert_eq!(
        ledger.receive_consent(&alternate_consent.flight, alternate_consent.frame()),
        Err("founder_ancestry")
    );
    assert!(ledger.at(Step::OfferAccepted));
    ledger.receive_consent(&verified, consent.frame()).unwrap();
    ledger.receive_consent(&verified, consent.frame()).unwrap();
    assert!(ledger.at(Step::Consent));
    ledger.reserve(sample.as_secs()).unwrap();
    let before = quota.inventory(None).unwrap();
    assert!(before > 0);
    let file_id = paths.create_founder_file(&file).unwrap();
    let ticket = ledger.start(file_id).unwrap();
    assert!(ledger.start(file_id).is_err());
    let key = paths.founder_image_key(generation);
    let started = std::time::Instant::now();
    let (created, journal_writes) = quota
        .create(
            ticket,
            &key,
            &native,
            verified.key_package().clone(),
            || Ok(()),
        )
        .unwrap();
    let create_ms = started.elapsed().as_millis();
    assert!(journal_writes > 0);
    assert!(Connection::open(&file).is_err());
    let alias = lease.root().join("native-alias.db");
    std::fs::hard_link(&file, &alias).unwrap();
    assert!(Connection::open(&alias).is_err());
    std::fs::remove_file(alias).unwrap();
    let alias = lease.root().join("native-symlink.db");
    std::os::unix::fs::symlink(&file, &alias).unwrap();
    assert!(Connection::open(&alias).is_err());
    std::fs::remove_file(alias).unwrap();

    let started = std::time::Instant::now();
    let (cold, _) = quota
        .inspect(
            generation,
            file_id,
            &key,
            &native,
            &created.image,
            || Ok(()),
        )
        .unwrap();
    let cold_ms = started.elapsed().as_millis();
    assert_eq!(cold.pin, created.image.pin);
    let delivery = codec
        .prepare_delivery(
            &founder,
            &verified,
            &created.welcome,
            sample.as_secs(),
            sample.as_secs() + 60,
        )
        .unwrap();
    let received = codec
        .open_delivery(
            &recipient,
            delivery.frame(),
            &consent.flight,
            sample.as_secs(),
        )
        .unwrap();
    assert_eq!(
        received.welcome_id().to_bytes().as_slice(),
        cold.welcome_id.as_slice()
    );
    assert!(received.status_negotiation().is_some());

    let wrong_generation = NonZeroU64::new(2).unwrap();
    let wrong = paths.founder_image_key(wrong_generation);
    assert!(matches!(
        quota.inspect(generation, file_id, &wrong, &native, &cold, || Ok(())),
        Err("founder_native_open")
    ));
    assert!(matches!(
        quota.inspect(generation, file_id, &key, &native, &cold, || Ok(())),
        Err("founder_quarantined")
    ));
    assert_eq!(quota.inventory(Some(&file)), Ok(before));
    println!(
        "founder-native create_ms={create_ms} cold_ms={cold_ms} journal_writes={journal_writes}"
    );
}

const WSS_CHILD: &str = "peer_invitation::founder_turn_tests::founder_wss_child";

#[test]
#[ignore = "bounded loopback TLS, SQLCipher and mainline MDK"]
fn founder_exact_v2_wss_turn() {
    run_then_reopen("wss");
}

#[test]
#[ignore = "bounded loopback grant and possible-exposure faults"]
fn founder_wss_refuses_revoked_grant_and_lost_ack() {
    for mode in ["offer-revoked", "native-revoked", "delivery-lost"] {
        run_then_reopen(mode);
    }
}

fn run_then_reopen(mode: &str) {
    let root = tempfile::tempdir().unwrap();
    isolated_child(WSS_CHILD, mode, "founder-wss ", Some(root.path()));
    isolated_child(COLD_CHILD, mode, "founder-cold ", Some(root.path()));
}

#[test]
#[ignore = "bounded actual founder processes across durable cut points"]
fn founder_closed_process_recovery_edges() {
    for mode in [
        "before-file",
        "before-inode",
        "before-freeze",
        "wrong-inode",
        "before-arm",
        "before-write",
    ] {
        run_then_reopen(mode);
    }
}

#[test]
#[ignore = "actual guarded native founder process stops mid-write"]
fn founder_hot_native_create_quarantines() {
    run_then_reopen("mid-native-create");
}

#[test]
#[ignore = "run only through the isolated proof parent"]
fn founder_cold_child() {
    crate::peer_quota_vfs::dispatch::mount_for_proof().unwrap();
    use super::founder_ledger::{Ledger, Step, ledger_binding};
    use crate::command_config::CommandConfig;

    let mode = std::env::var(CHILD_FLAG).unwrap();
    let root = std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    let entries = std::fs::read_dir(root)
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    let [profile] = entries.as_slice() else {
        panic!("founder_profile_inventory")
    };
    let mut lease = ProfileLease::acquire(&profile.path(), "test").unwrap();
    let material = lease
        .prepare(
            BackendChoice::File,
            Path::new("/unused"),
            std::time::Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
    let paths = Paths::new(&lease, &material).unwrap();
    let mut quota = FounderQuota::mount(&lease).unwrap();
    let generation = NonZeroU64::new(1).unwrap();
    let main = quota.main(generation);
    assert!(
        Connection::open(&main).is_err(),
        "canonical family must never use stock fallback"
    );
    let config = CommandConfig::load(lease.root()).unwrap();
    let account = config.execution_clients().iter().next().unwrap();
    let recipient = nostr::key::PublicKey::from_byte_array(*account);
    let origin = config.founder_origin(account).unwrap().as_str();
    let binding = ledger_binding(material.public_key(), recipient, origin);
    let ledger_start = std::time::Instant::now();
    let ledger = Ledger::open(
        &quota.ledger(),
        *paths.founder_ledger_key(),
        binding,
        generation,
        false,
    )
    .unwrap();
    let ledger_ms = ledger_start.elapsed().as_millis();
    let step = match mode.as_str() {
        "wss" => Step::DeliveryAccepted,
        "offer-revoked" => Step::NotSent,
        "native-revoked" => Step::Consent,
        "delivery-lost" => Step::Uncertain,
        "before-file" | "before-inode" => Step::Reserved,
        "before-freeze" | "mid-native-create" | "wrong-inode" => Step::Started,
        "before-arm" => Step::Delivery,
        "before-write" => Step::DeliveryArmed,
        _ => panic!("founder_cold_mode"),
    };
    assert!(ledger.at(step));
    let history = ledger.historical_image().unwrap();
    let mut native_ms = None;
    if let Some(history) = history {
        assert!(matches!(
            step,
            Step::DeliveryAccepted | Step::Uncertain | Step::Delivery | Step::DeliveryArmed
        ));
        let routing = NostrRoutingV1::new(history.routing_id, vec![origin.to_owned()]).unwrap();
        let expected = Expected {
            signer: material.signer(),
            recipient,
            routing: &routing,
            room: history.room,
            reference: &history.reference,
        };
        let started = std::time::Instant::now();
        quota
            .inspect(
                generation,
                history.file,
                &paths.founder_image_key(generation),
                &expected,
                &history.image,
                || Ok(()),
            )
            .unwrap();
        native_ms = Some(started.elapsed().as_millis());
    } else {
        assert_eq!(
            main.exists(),
            matches!(
                mode.as_str(),
                "before-inode" | "before-freeze" | "mid-native-create" | "wrong-inode"
            )
        );
        assert_eq!(
            ledger.persisted_file().unwrap().is_some(),
            step == Step::Started
        );
        assert_eq!(quota.inventory(None).is_err(), main.exists());
        if mode == "mid-native-create" {
            let journal = main.with_file_name("founder-1.db-journal");
            assert!(journal.metadata().unwrap().len() > 0);
            assert!(Connection::open(journal).is_err());
            assert!(quota.inventory(Some(&main)).is_err());
        }
        if mode == "wrong-inode" {
            assert_eq!(main.metadata().unwrap().len(), 0);
        }
    }
    println!("founder-cold mode={mode} ledger_ms={ledger_ms} native_ms={native_ms:?}");
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "run only through the isolated proof parent"]
async fn founder_wss_child() {
    let mode = std::env::var(CHILD_FLAG).unwrap();
    let parent = std::path::PathBuf::from(std::env::var_os(CHILD_ROOT).unwrap());
    use super::founder_ledger::{Ledger, Outbound, Step, ledger_binding};
    use crate::peer_invitation::socket::owner::recipient::founder::{FounderPort, FounderTurn};
    use crate::peer_quota_vfs::dispatch::mount_for_proof;
    use crate::peer_relay_trust::qualified_founder_fixture;

    mount_for_proof().unwrap();
    let recipient = Arc::new(Keys::generate());
    let unrelated = Keys::generate();
    let (mut relay, temp, lease, material) = qualified_founder_fixture(
        recipient.public_key(),
        unrelated.public_key(),
        Duration::from_secs(45),
        Some(&parent),
        mode == "wss",
    )
    .await;
    let temp = temp.keep();
    let scope = relay.url.as_str();
    let founder = material.signer();
    let paths = Paths::new(&lease, &material).unwrap();
    paths.initialize_founder_directory().unwrap();
    let mut quota = FounderQuota::mount(&lease).unwrap();
    if mode == "mid-native-create" {
        quota.fixture_stop_in_synced_transaction();
    }
    assert!(quota.owns(&lease));
    let generation = NonZeroU64::new(1).unwrap();
    let main = quota.main(generation);
    assert!(Connection::open(&main).is_err());
    assert_eq!(quota.inventory(None), Ok(0));
    let config_path = lease.root().join("config.json");
    let config_bytes = std::fs::read(&config_path).unwrap();
    let config_value: serde_json::Value = serde_json::from_slice(&config_bytes).unwrap();
    use crate::command_config::CommandConfig;
    let recipient_bytes = recipient.public_key().to_bytes();
    let mut wrong_role = config_value.clone();
    wrong_role["role"] = serde_json::json!("client");
    assert!(matches!(
        CommandConfig::from_bytes(&serde_json::to_vec(&wrong_role).unwrap()),
        Err(crate::Error::Invalid("config_peer_directions"))
    ));
    let mut disabled = config_value;
    disabled["peer"]["originate"] = serde_json::json!(false);
    let parsed = CommandConfig::from_bytes(&serde_json::to_vec(&disabled).unwrap()).unwrap();
    assert!(parsed.founder_origin(&recipient_bytes).is_none());
    assert_eq!(
        CommandConfig::from_bytes(&config_bytes)
            .unwrap()
            .founder_origin(&recipient_bytes)
            .map(|url| url.as_str()),
        Some(scope)
    );
    for (key, origin) in [
        (founder.public_key(), scope),
        (recipient.public_key(), ORIGIN),
    ] {
        assert!(
            FounderPort::open(lease.clone(), &material, key, origin, generation, wss_end())
                .await
                .is_err()
        );
    }

    assert!(
        FounderPort::open(
            lease.clone(),
            &material,
            unrelated.public_key(),
            scope,
            generation,
            wss_end()
        )
        .await
        .is_err()
    );
    let port = FounderPort::open(
        lease.clone(),
        &material,
        recipient.public_key(),
        scope,
        generation,
        wss_end(),
    )
    .await
    .unwrap();
    let connector = relay.connector(lease.root(), true, None);
    let mut remote = Owner::open(
        &connector,
        scope,
        recipient.public_key(),
        &recipient,
        wss_end(),
    )
    .await
    .unwrap();
    assert!(relay.authenticated(founder.public_key()));
    assert!(relay.authenticated(recipient.public_key()));
    let started = Instant::now();
    let room = [29; 16];
    let slot = [31; 16];
    let routing = NostrRoutingV1::new([33; 32], vec![scope.to_owned()]).unwrap();
    let codec = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2).unwrap();

    let ledger_path = quota.ledger();
    let ledger_key = paths.founder_ledger_key();
    let bind = ledger_binding(founder.public_key(), recipient.public_key(), scope);
    let mut turn = FounderTurn::new(port, quota, paths).unwrap();
    let wall = turn.current().unwrap();
    let offer = codec
        .prepare_offer(&founder, recipient.public_key(), room, &routing, wall)
        .unwrap();
    turn.claim(&offer).unwrap();
    assert!(turn.claim(&offer).is_err());
    let mut wrong_bytes = offer.frame().to_vec();
    wrong_bytes.push(b' ');
    assert!(
        turn.send(
            Outbound::Offer,
            offer.flight.flight(),
            &wrong_bytes,
            wss_end(),
            || {}
        )
        .await
        .is_err()
    );
    assert!(!turn.journal().current(
        Outbound::Offer,
        Step::Offer,
        offer.flight.flight(),
        offer.frame(),
        offer.flight.flight().expires_at
    ));
    let config = lease.root().join("config.json");
    let original = std::fs::read(&config).unwrap();
    let mut updated: serde_json::Value = serde_json::from_slice(&original).unwrap();
    updated["execution_clients"] = serde_json::json!([]);
    let revoked = serde_json::to_vec(&updated).unwrap();
    let result = turn
        .send(
            Outbound::Offer,
            offer.flight.flight(),
            offer.frame(),
            wss_end(),
            || {
                if mode == "offer-revoked" {
                    std::fs::write(&config, &revoked).unwrap();
                }
            },
        )
        .await
        .unwrap();
    if mode == "offer-revoked" {
        assert_eq!(result, Step::NotSent);
        assert!(relay.peer_transcript().is_empty());
        std::fs::write(&config, &original).unwrap();
        assert!(turn.current().is_err(), "revocation must stay sticky");
        drop(turn);
        let mut cold = Ledger::open(&ledger_path, *ledger_key, bind, generation, false).unwrap();
        assert!(cold.at(Step::NotSent));
        assert!(cold.claim(&offer).is_err());
        assert!(!main.exists());
        relay.stop().await;
        println!("founder-wss offer_revoked_before_write=true");
        return;
    }
    assert_eq!(result, Step::OfferAccepted);
    assert!(
        turn.send(
            Outbound::Offer,
            offer.flight.flight(),
            offer.frame(),
            wss_end(),
            || {}
        )
        .await
        .is_err()
    );
    let seen = remote.wait_live(wss_end()).await.unwrap().unwrap();
    let record = seen.arrival();
    assert_eq!(record.id, offer.flight.flight().carrier_id.to_bytes());
    let remote_offer = codec
        .open_offer(
            &recipient,
            record.body.get().as_bytes(),
            Some(founder.public_key()),
            &routing.relays,
            turn.current().unwrap(),
        )
        .unwrap();

    let recipient_db = tempfile::NamedTempFile::new_in(&temp).unwrap();
    let sample = Duration::from_secs(turn.current().unwrap());
    let package = OpenStage::open(
        recipient_db.path(),
        [37; 32],
        recipient.clone(),
        scope,
        hex::encode(slot),
    )
    .unwrap()
    .package(sample, true)
    .await
    .unwrap();
    let consent = codec
        .prepare_consent(
            &recipient,
            &remote_offer,
            &package.bytes,
            sample.as_secs(),
            sample.as_secs() + 120,
        )
        .unwrap();
    assert!(
        remote
            .publish(&consent, consent.flight.flight(), wss_end())
            .await
            .unwrap()
    );
    let seen_at = turn.current().unwrap();
    let page = turn
        .page([wall.saturating_sub(172_800), seen_at], wss_end())
        .await
        .unwrap();
    let verified = turn.receive_consent(&page, &codec, &offer.flight).unwrap();
    assert_eq!(
        verified.flight().carrier_id,
        consent.flight.flight().carrier_id
    );
    let offer_consent_ms = started.elapsed().as_millis();

    if mode == "native-revoked" {
        std::fs::write(&config, &revoked).unwrap();
        assert_eq!(turn.reserve(&verified), Err("founder_grant"));
        assert_eq!(turn.create_file(&verified), Err("founder_grant"));
        assert!(turn.journal().at(Step::Consent));
        std::fs::write(&config, &original).unwrap();
        assert!(turn.current().is_err());
        drop(turn);
        let mut cold = Ledger::open(&ledger_path, *ledger_key, bind, generation, false).unwrap();
        assert!(cold.at(Step::Consent));
        assert!(cold.reserve(wall).is_err());
        assert!(!main.exists());
        assert_eq!(relay.peer_transcript().len(), 2);
        relay.stop().await;
        println!("founder-wss grant_revoked_before_native=true");
        return;
    }

    turn.reserve(&verified).unwrap();
    if mode == "before-file" {
        println!("founder-wss cut=before-file");
        return;
    }
    let inode = turn.create_file(&verified).unwrap();
    if mode == "before-inode" {
        println!("founder-wss cut=before-inode");
        return;
    }
    if mode == "wrong-inode" {
        assert!(matches!(
            turn.create(&verified, (inode.0, inode.1 ^ 1)),
            Err("founder_file")
        ));
        assert_eq!(relay.peer_transcript().len(), 2);
        println!("founder-wss wrong-inode=true");
        return;
    }
    if mode == "mid-native-create" {
        println!("founder-wss abrupt-native-write=true");
    }
    let started = Instant::now();
    let (created, writes) = turn.create(&verified, inode).unwrap();
    assert!(turn.create(&verified, inode).is_err());
    let native_ms = started.elapsed().as_millis();
    assert!(writes > 0);
    if mode == "before-freeze" {
        println!("founder-wss cut=before-freeze");
        return;
    }
    let started = Instant::now();
    let observed = turn.inspect(&verified, inode, &created.image).unwrap();
    let cold_ms = started.elapsed().as_millis();

    let wall = turn.current().unwrap();
    let delivery = codec
        .prepare_delivery(
            &founder,
            &verified,
            &created.welcome,
            wall,
            verified.flight().expires_at,
        )
        .unwrap();
    turn.freeze(&verified, &delivery, &observed).unwrap();
    if mode == "before-arm" {
        println!("founder-wss cut=before-arm");
        return;
    }
    if mode == "before-write" {
        turn.arm_before_write_cut(&delivery).unwrap();
        assert_eq!(relay.peer_transcript().len(), 2);
        println!("founder-wss cut=before-write");
        return;
    }
    let started = Instant::now();
    let flight = delivery.flight.flight();
    let (send_deadline, expected_step) = if mode == "delivery-lost" {
        relay.recipient_lose_ack(flight.carrier_id);
        (Instant::now() + Duration::from_secs(1), Step::Uncertain)
    } else {
        (wss_end(), Step::DeliveryAccepted)
    };
    let result = turn
        .send(
            Outbound::Delivery,
            flight,
            delivery.frame(),
            send_deadline,
            || {},
        )
        .await
        .unwrap();
    let delivery_ms = started.elapsed().as_millis();
    assert_eq!(result, expected_step);
    let wrapped = remote.wait_live(wss_end()).await.unwrap().unwrap();
    let received = codec
        .open_delivery(
            &recipient,
            wrapped.arrival().body.get().as_bytes(),
            &consent.flight,
            wall,
        )
        .unwrap();
    assert_eq!(
        received.welcome_id().to_bytes().as_slice(),
        observed.image().welcome_id.as_slice()
    );
    assert_eq!(received.welcome_digest(), delivery.flight.welcome_digest());
    assert_eq!(
        received.status_negotiation().unwrap().app_room(),
        delivery.flight.status_negotiation().unwrap().app_room()
    );
    assert_eq!(relay.peer_transcript().len(), 3);
    assert!(relay.peer_only_publications());
    assert_eq!(relay.recipient_requests(founder.public_key()), 2);
    assert_eq!(relay.recipient_requests(recipient.public_key()), 1);

    drop(turn);
    let mut reopened = Ledger::open(&ledger_path, *ledger_key, bind, generation, false).unwrap();
    assert_eq!(reopened.persisted_file().unwrap(), Some(inode));
    assert!(reopened.current(
        Outbound::Delivery,
        expected_step,
        flight,
        delivery.frame(),
        wall
    ));
    assert!(
        reopened
            .arm(Outbound::Delivery, flight, delivery.frame(), wall)
            .is_err()
    );
    relay.stop().await;
    println!(
        "founder-wss delivery_ack={} offer_consent_ms={offer_consent_ms} native_ms={native_ms} delivery_ms={delivery_ms} cold_ms={cold_ms} journal_writes={writes}",
        expected_step == Step::DeliveryAccepted
    );
}

fn wss_end() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
