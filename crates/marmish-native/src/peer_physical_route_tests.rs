use super::{
    super::{
        inode, probe, qualification,
        tests::{QualificationFixture, private_write},
    },
    *,
};
use crate::{
    identity::BackendChoice,
    mdk_adapter::{MdkOwner, terminal_relay_fixture::Relay},
    transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

const RECORD: &str = "peer-relay-qualification.json";
const ATTESTATION: &str = "peer-relay-attestation.json";
const CONFIG: &str = "config.json";
const CA: &str = "peer-relay-ca.der";
const MANIFEST: &str = "profile.json";
const LOCK: &str = "owner.lock";

fn prepare(lease: &mut ProfileLease) -> IdentityMaterial {
    lease
        .prepare(
            BackendChoice::File,
            Path::new("/unused"),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap()
}

fn fresh_profile() -> (tempfile::TempDir, Arc<ProfileLease>, IdentityMaterial) {
    fresh_profile_in(None)
}

fn fresh_profile_in(
    parent: Option<&Path>,
) -> (tempfile::TempDir, Arc<ProfileLease>, IdentityMaterial) {
    let temp = match parent {
        Some(parent) => tempfile::tempdir_in(parent),
        None => tempfile::tempdir(),
    }
    .unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let mut lease = ProfileLease::acquire(temp.path(), "test").unwrap();
    let material = prepare(&mut lease);
    let policy = RelayPolicy::parse(
        &["wss://127.0.0.1:9/".into()],
        RouteMode::Off,
        TransportLimits::default(),
    )
    .unwrap();
    let connector = RelayConnector::new(
        policy,
        ArtiDirs {
            state: lease.arti_state().into(),
            cache: lease.arti_cache().into(),
        },
    )
    .unwrap();
    let owner = MdkOwner::open(&lease, &material, connector, vec![]).unwrap();
    lease.mark_ready(owner.opened_binding()).unwrap();
    drop(owner);
    lease.cleanup_runtime().unwrap();
    (temp, Arc::new(lease), material)
}

fn sample(wall: u64, mono: Instant) -> ClockSample {
    ClockSample {
        wall: UNIX_EPOCH + Duration::from_secs(wall),
        mono,
    }
}

#[test]
fn physical_route_clock_rejects_regression_drift_and_overflow() {
    let now = Instant::now();
    let mut clock = ScopeClock::new(sample(10, now)).unwrap();
    assert_eq!(
        clock
            .observe(sample(10, now + Duration::from_secs(2)))
            .unwrap(),
        12
    );
    assert!(
        clock
            .observe(sample(10, now + Duration::from_millis(2_001)))
            .is_err()
    );
    assert!(
        clock
            .observe(sample(14, now + Duration::from_secs(1)))
            .is_err()
    );
    assert!(
        clock
            .observe(sample(9, now + Duration::from_secs(2)))
            .is_err()
    );

    let mut clock = ScopeClock::new(sample(10, now)).unwrap();
    assert_eq!(clock.observe(sample(12, now)).unwrap(), 12);
    assert!(clock.observe(sample(13, now)).is_err());
    assert!(
        ScopeClock::new(ClockSample {
            wall: UNIX_EPOCH - Duration::from_nanos(1),
            mono: now,
        })
        .is_err()
    );
    let mut clock = ScopeClock {
        initial_wall: Duration::MAX,
        initial_mono: now,
        previous_wall: Duration::ZERO,
        previous_mono: now,
        effective_wall: Duration::ZERO,
    };
    assert!(
        clock
            .observe(sample(10, now + Duration::from_nanos(1)))
            .is_err()
    );
}

pub(crate) async fn qualified_recipient_fixture(
    founder: PublicKey,
    live_founder: PublicKey,
    leaf_lifetime: Duration,
) -> (
    Relay,
    tempfile::TempDir,
    Arc<ProfileLease>,
    IdentityMaterial,
) {
    qualified_control_fixture(
        founder,
        live_founder,
        leaf_lifetime,
        None,
        QualificationFixture::from_ready,
    )
    .await
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) async fn qualified_manual_recipient_fixture(
    founder: PublicKey,
    third: PublicKey,
    parent: &Path,
    trusted_remote: Option<PublicKey>,
) -> (
    Relay,
    tempfile::TempDir,
    Arc<ProfileLease>,
    IdentityMaterial,
) {
    qualified_control_fixture(
        founder,
        third,
        Duration::from_secs(90),
        Some(parent),
        |relay, temp, lease| {
            QualificationFixture::from_ready_configured(
                relay,
                temp,
                lease,
                |lease, origin, trust| {
                    let config = serde_json::json!({
                        "version":1, "role":"client", "relays":[origin], "tor":"off",
                        "trusted_remotes":trusted_remote.map(|key| key.to_hex()).into_iter().collect::<Vec<_>>(),
                        "execution_clients":[],
                        "command_policy":{"version":1}, "cwd":null, "limits":{}, "aliases":{},
                        "peer":{"relay":origin, "relay_trust":trust, "receive":{"mode":"manual"}}
                    });
                    let bytes = serde_json::to_vec(&config).unwrap();
                    private_write(&lease.root().join(CONFIG), &bytes);
                    crate::command_config::CommandConfig::from_bytes(&bytes).unwrap()
                },
            )
        },
    )
    .await
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub(crate) async fn qualified_founder_fixture(
    recipient: PublicKey,
    third: PublicKey,
    leaf_lifetime: Duration,
    parent: Option<&Path>,
    server_only: bool,
) -> (
    Relay,
    tempfile::TempDir,
    Arc<ProfileLease>,
    IdentityMaterial,
) {
    qualified_control_fixture(
        recipient,
        third,
        leaf_lifetime,
        parent,
        |relay, temp, lease| {
            QualificationFixture::from_ready_configured(
                relay,
                temp,
                lease,
                |lease, origin, trust| {
                    use nostr::nips::nip19::ToBech32;
                    let config = serde_json::json!({
                        "version":1,
                        "role":if server_only { "server" } else { "both" },
                        "relays":[origin], "tor":"off",
                        "trusted_remotes":[], "execution_clients":[recipient.to_bech32().unwrap()],
                        "command_policy":{"version":1}, "cwd":null, "limits":{}, "aliases":{},
                        "peer":{"relay":origin, "relay_trust":trust, "originate":true}
                    });
                    let bytes = serde_json::to_vec(&config).unwrap();
                    private_write(&lease.root().join("config.json"), &bytes);
                    crate::command_config::CommandConfig::from_bytes(&bytes).unwrap()
                },
            )
        },
    )
    .await
}

async fn qualified_control_fixture(
    remote: PublicKey,
    third: PublicKey,
    leaf_lifetime: Duration,
    parent: Option<&Path>,
    config: impl FnOnce(&Relay, tempfile::TempDir, Arc<ProfileLease>) -> QualificationFixture,
) -> (
    Relay,
    tempfile::TempDir,
    Arc<ProfileLease>,
    IdentityMaterial,
) {
    let plan = probe::Plan::new().unwrap();
    let (accounts, group) = plan.relay_registration();
    let relay = Relay::start_recipient_qualification(accounts, group, leaf_lifetime).await;
    let (temp, lease, material) = fresh_profile_in(parent);
    let fixture = config(&relay, temp, lease.clone());
    let checked = qualification::CheckedAttestation::read(
        &fixture.attestation_path,
        fixture.relay(),
        qualification::wall().unwrap(),
    )
    .unwrap();
    let authority = fixture.authority(&checked);
    let completed = plan
        .execute(
            &fixture.owner.connector,
            &authority,
            authority.check().unwrap(),
        )
        .await
        .unwrap();
    qualification::issue(&authority, completed, &fixture.issuer_key()).unwrap();

    let QualificationFixture { temp, owner, .. } = fixture;
    drop(owner);
    drop(lease);
    drop(material);
    let mut lease = ProfileLease::acquire_ready(temp.path(), "test").unwrap();
    let material = prepare(&mut lease);
    let lease = Arc::new(lease);
    relay
        .switch_to_recipient_runtime([material.public_key(), remote], third)
        .await;
    (relay, temp, lease, material)
}

#[tokio::test]
async fn physical_route_cold_real_wss_binding_and_faults() {
    let plan = probe::Plan::new().unwrap();
    let (accounts, group) = plan.relay_registration();
    let mut relay = Relay::start_peer_qualification(accounts, group).await;
    let (temp, lease, material) = fresh_profile();
    let fixture = QualificationFixture::from_ready(&relay, temp, lease.clone());
    let root = lease.root().to_path_buf();
    assert!(CheckedPeerRoute::bind(lease.clone(), &material).is_err());

    let checked = qualification::CheckedAttestation::read(
        &fixture.attestation_path,
        fixture.relay(),
        qualification::wall().unwrap(),
    )
    .unwrap();
    let authority = fixture.authority(&checked);
    let completed = plan
        .execute(
            &fixture.owner.connector,
            &authority,
            authority.check().unwrap(),
        )
        .await
        .unwrap();
    qualification::issue(&authority, completed, &fixture.issuer_key()).unwrap();

    let QualificationFixture { temp, owner, .. } = fixture;
    drop(owner);
    drop(lease);
    verify_cold_identity_mismatch(temp.path(), &root, &material);
    drop(material);
    let mut lease = ProfileLease::acquire_ready(temp.path(), "test").unwrap();
    let material = prepare(&mut lease);
    let lease = Arc::new(lease);
    let files = [RECORD, ATTESTATION, CONFIG, CA, MANIFEST, LOCK];
    let before = files.map(|name| fs::read(root.join(name)).unwrap());
    let connections = relay.qualification_connections();
    assert_eq!(connections, 4);
    let bind = || CheckedPeerRoute::bind(lease.clone(), &material).unwrap();
    let keys = root.join("keys");
    let moved_keys = root.join("keys.private-test-moved");
    fs::rename(&keys, &moved_keys).unwrap();
    let mut route = bind();
    route.recheck().unwrap();
    fs::rename(&moved_keys, &keys).unwrap();
    assert_eq!(route.account, material.public_key());
    assert_eq!(route.device_slot, *material.device_slot());
    fs::write(root.join(RECORD), &before[0]).unwrap();
    route.recheck().unwrap();
    assert_eq!(before, files.map(|name| fs::read(root.join(name)).unwrap()));
    let drop_witness = route.profile.cancel.clone();
    assert!(!drop_witness.is_cancelled());
    drop(route);
    assert!(drop_witness.is_cancelled());
    let mut route = bind();
    route.profile.cancel.cancel();
    assert!(matches!(route.recheck(), Err(Error::Transport("shutdown"))));

    verify_first_bind_faults(&lease, &material);
    verify_mutation_faults(&root, &before, bind);

    let metadata: serde_json::Value = serde_json::from_slice(&before[0]).unwrap();
    let expires = metadata["expires_at"].as_u64().unwrap();
    let anchor = Instant::now();
    let mut route =
        CheckedPeerRoute::bind_using(lease.clone(), &material, || sample(expires - 1, anchor))
            .unwrap();
    let mut ticks = [
        sample(expires - 1, anchor),
        sample(expires - 1, anchor + Duration::from_secs(1)),
    ]
    .into_iter();
    assert!(route.recheck_using(|| ticks.next().unwrap()).is_err());
    assert!(matches!(route.recheck(), Err(Error::Transport("shutdown"))));
    assert!(
        CheckedPeerRoute::bind_using(lease.clone(), &material, || sample(expires, anchor)).is_err()
    );
    assert_eq!(before, files.map(|name| fs::read(root.join(name)).unwrap()));
    assert_eq!(relay.qualification_connections(), connections);
    assert_eq!(relay.peer_transcript().len(), 1);
    assert_eq!(relay.group_writes().len(), 1);
    relay.stop().await;
    lease.cleanup_runtime().unwrap();
}

fn verify_cold_identity_mismatch(base: &Path, root: &Path, original: &IdentityMaterial) {
    let path = root.join(MANIFEST);
    let manifest = fs::read(&path).unwrap();
    let mut parsed: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    let old_slot = parsed["device_slot"].as_str().unwrap();
    let changed_slot = if old_slot.starts_with("00") {
        "11"
    } else {
        "00"
    };
    parsed["device_slot"] = serde_json::json!(format!("{changed_slot}{}", &old_slot[2..]));
    fs::write(&path, serde_json::to_vec(&parsed).unwrap()).unwrap();
    let lease = Arc::new(ProfileLease::acquire_ready(base, "test").unwrap());
    assert!(matches!(
        CheckedPeerRoute::bind(lease, original),
        Err(Error::Identity("identity_mismatch")),
    ));
    fs::write(&path, manifest).unwrap();

    let key_path = root.join("keys").join(format!(
        "sqlcipher-{}",
        parsed["sqlcipher_ref"].as_str().unwrap()
    ));
    let mut changed_root = zeroize::Zeroizing::new(*original.sqlcipher_root());
    changed_root[0] ^= 0xff;
    fs::write(&key_path, changed_root.as_slice()).unwrap();
    let mut lease = ProfileLease::acquire_ready(base, "test").unwrap();
    let wrong_root = prepare(&mut lease);
    assert_eq!(wrong_root.public_key(), original.public_key());
    assert_eq!(wrong_root.device_slot(), original.device_slot());
    assert!(CheckedPeerRoute::bind(Arc::new(lease), &wrong_root).is_err());
    fs::write(&key_path, original.sqlcipher_root()).unwrap();
}

fn verify_first_bind_faults(lease: &Arc<ProfileLease>, material: &IdentityMaterial) {
    let (_other_temp, _other_lease, other) = fresh_profile();
    assert!(matches!(
        CheckedPeerRoute::bind(lease.clone(), &other),
        Err(Error::Identity("identity_mismatch")),
    ));
    for missing in [RECORD, ATTESTATION, CA] {
        let path = lease.root().join(missing);
        let moved = path.with_extension("moved");
        fs::rename(&path, &moved).unwrap();
        assert!(CheckedPeerRoute::bind(lease.clone(), material).is_err());
        fs::rename(&moved, &path).unwrap();
    }

    let path = lease.root().join(RECORD);
    let original = fs::read(&path).unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(&original).unwrap();
    for schema in [1, 2] {
        record["schema"] = serde_json::json!(schema);
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(CheckedPeerRoute::bind(lease.clone(), material).is_err());
    }
    record["schema"] = serde_json::json!(3);
    record["issuer_tag"] = serde_json::json!("ff".repeat(32));
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(CheckedPeerRoute::bind(lease.clone(), material).is_err());
    fs::write(&path, original).unwrap();
}

fn verify_mutation_faults(root: &Path, before: &[Vec<u8>; 6], bind: impl Fn() -> CheckedPeerRoute) {
    for (name, original) in [RECORD, ATTESTATION, CONFIG, CA, MANIFEST]
        .into_iter()
        .zip(before)
    {
        let mut route = bind();
        let path = root.join(name);
        let mut changed = original.clone();
        if name == MANIFEST {
            let mut manifest: serde_json::Value = serde_json::from_slice(original).unwrap();
            manifest["transaction"] = serde_json::json!("4a".repeat(16));
            changed = serde_json::to_vec(&manifest).unwrap();
        } else {
            changed.push(b' ');
        }
        fs::write(&path, changed).unwrap();
        assert!(route.recheck().is_err(), "modified {name} must refuse");
        fs::write(&path, original).unwrap();
        assert!(matches!(route.recheck(), Err(Error::Transport("shutdown"))));
    }

    for (name, original) in [RECORD, ATTESTATION, CONFIG, CA, MANIFEST, LOCK]
        .into_iter()
        .zip(before)
    {
        let mut route = bind();
        let path = root.join(name);
        let moved = path.with_extension("moved");
        let held = match name {
            MANIFEST => Some(&route.profile.manifest),
            LOCK => None,
            _ => Some(route.qualification.test_descriptor(name)),
        };
        let file_id = inode(&fs::metadata(&path).unwrap());
        fs::rename(&path, &moved).unwrap();
        private_write(&path, original);
        if let Some(held) = held {
            assert_eq!(file_id, inode(&held.metadata().unwrap()));
            assert_ne!(file_id, inode(&fs::metadata(&path).unwrap()));
        }
        assert!(route.recheck().is_err(), "replaced {name} must refuse");
        fs::rename(&moved, &path).unwrap();
        assert!(matches!(route.recheck(), Err(Error::Transport("shutdown"))));
    }

    let mut route = bind();
    let path = root.join(RECORD);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(route.recheck().is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    let mut route = bind();
    let moved = path.with_extension("moved");
    fs::rename(&path, &moved).unwrap();
    symlink(&moved, &path).unwrap();
    assert!(route.recheck().is_err());
    fs::rename(&moved, &path).unwrap();

    let mut route = bind();
    let moved = root.with_extension("moved");
    let file_id = inode(&route.profile.dir.metadata().unwrap());
    fs::rename(root, &moved).unwrap();
    fs::create_dir(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    assert_ne!(file_id, inode(&fs::metadata(root).unwrap()));
    assert!(route.recheck().is_err());
    fs::remove_dir(root).unwrap();
    fs::rename(&moved, root).unwrap();
    assert!(matches!(route.recheck(), Err(Error::Transport("shutdown"))));
}
