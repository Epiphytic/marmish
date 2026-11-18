use super::{super::tests::private_write, *};
use crate::{
    identity::BackendChoice,
    mdk_adapter::MdkOwner,
    transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits},
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    time::{Duration, Instant},
};

struct Fixture {
    _directory: tempfile::TempDir,
    lease: Arc<ProfileLease>,
    material: IdentityMaterial,
}

impl Fixture {
    fn new(name: &str, ready: bool) -> Self {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let mut lease = ProfileLease::acquire(directory.path(), name).unwrap();
        let mut material = lease
            .prepare(
                BackendChoice::File,
                Path::new("/unused"),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        if ready {
            let owner = open_owner(&lease, &material);
            lease.mark_ready(owner.opened_binding()).unwrap();
            drop(owner);
            lease.cleanup_runtime().unwrap();
            drop(lease);
            lease = ProfileLease::acquire_ready(directory.path(), name).unwrap();
            material = lease
                .prepare(
                    BackendChoice::File,
                    Path::new("/unused"),
                    Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
        }
        let fixture = Self {
            _directory: directory,
            lease: Arc::new(lease),
            material,
        };
        private_write(
            &fixture.lease.root().join("config.json"),
            &settings("both", "off", Some("manual")),
        );
        fixture
    }

    fn capture(&self) -> Result<Option<ReadyReceiverScope>> {
        ReadyReceiverScope::capture(
            &self.lease,
            &self.material,
            &CommandConfig::load(self.lease.root()).unwrap(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.lease.cleanup_runtime().unwrap();
    }
}

fn settings(role: &str, tor: &str, mode: Option<&str>) -> Vec<u8> {
    let mut config = json!({
        "version": 1, "role": role, "relays": ["wss://127.0.0.1:9/"],
        "tor": tor, "command_policy": {"version": 1},
        "trusted_remotes": [], "execution_clients": [], "cwd": null,
        "limits": {}, "aliases": {}
    });
    if let Some(mode) = mode {
        let mut receive = json!({"mode": mode});
        if mode == "auto_founders" {
            use nostr::{key::Keys, nips::nip19::ToBech32};
            receive["founders"] = json!([Keys::generate().public_key().to_bech32().unwrap()]);
        }
        config["peer"] = json!({
            "receive": receive,
            "relay": "wss://private.example/",
            "relay_trust": format!("spki-sha256:{}", "ab".repeat(32)),
        });
    }
    serde_json::to_vec(&config).unwrap()
}

fn open_owner(lease: &ProfileLease, material: &IdentityMaterial) -> MdkOwner {
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
    MdkOwner::open(lease, material, connector, vec![]).unwrap()
}

#[test]
fn peer_ready_scope_requires_manual_on_preexisting_ready_client_or_both() {
    let mut initializing = Fixture::new("initializing", false);
    assert!(initializing.capture().unwrap().is_none());
    let owner = open_owner(&initializing.lease, &initializing.material);
    Arc::get_mut(&mut initializing.lease)
        .unwrap()
        .mark_ready(owner.opened_binding())
        .unwrap();
    assert!(initializing.lease.setup_snapshot().ready);
    assert!(initializing.capture().unwrap().is_none());
    drop(owner);
    let ready = Fixture::new("ready", true);
    let path = ready.lease.root().join("config.json");
    for (role, tor, mode, eligible) in [
        ("both", "off", None, false),
        ("both", "off", Some("off"), false),
        ("both", "off", Some("auto_any"), false),
        ("client", "off", Some("auto_founders"), false),
        ("server", "off", None, false),
        ("client", "always", Some("manual"), false),
        ("client", "auto", Some("manual"), true),
        ("both", "off", Some("manual"), true),
    ] {
        fs::write(&path, settings(role, tor, mode)).unwrap();
        assert_eq!(ready.capture().unwrap().is_some(), eligible);
    }
}

#[test]
fn peer_ready_scope_fences_config_and_manifest_identity_and_exact_bytes() {
    let ready = Fixture::new("ready", true);
    for name in ["config.json", "profile.json"] {
        let path = ready.lease.root().join(name);
        let original = fs::read(&path).unwrap();
        let scope = ready.capture().unwrap().unwrap();
        let mut changed = original.clone();
        changed.push(b'\n');
        fs::write(&path, changed).unwrap();
        assert!(scope.recheck(&ready.lease).is_err());
        fs::write(&path, &original).unwrap();
        assert!(scope.recheck(&ready.lease).is_err());

        let scope = ready.capture().unwrap().unwrap();
        let replacement = ready.lease.root().join("next");
        private_write(&replacement, &original);
        fs::rename(&replacement, &path).unwrap();
        assert!(scope.recheck(&ready.lease).is_err());
    }
}

#[test]
fn peer_ready_scope_keeps_opened_binding_validation_read_only() {
    let ready = Fixture::new("ready", true);
    let other = Fixture::new("other", true);
    let scope = ready.capture().unwrap().unwrap();
    let peer_config = CommandConfig::load(ready.lease.root()).unwrap();
    assert!(ReadyReceiverScope::capture(&ready.lease, &other.material, &peer_config).is_err());

    let owner = open_owner(&ready.lease, &ready.material);
    let binding = owner.opened_binding();
    ready.lease.check_opened_binding(binding).unwrap();
    assert!(matches!(
        other.lease.check_opened_binding(binding),
        Err(crate::Error::Identity("storage_binding_mismatch"))
    ));
    scope.recheck(&ready.lease).unwrap();
    assert!(scope.recheck(&other.lease).is_err());
    assert!(scope.recheck(&ready.lease).is_err());
}
