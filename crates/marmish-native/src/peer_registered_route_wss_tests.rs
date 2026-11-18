use super::{Keys, PeerRecipientPort, ReceiverRouteAdmission, time_limit};
use crate::{
    command_config::CommandConfig,
    identity::ProfileLease,
    peer_relay_trust::{
        candidate_registration::{ReceiverPreflight, RegistrationFailure},
        qualified_manual_recipient_fixture,
    },
};
use rusqlite::{Connection, ffi as sql};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
    ptr,
    sync::Arc,
    time::{Duration, Instant},
};

const CASE_ENV: &str = "MARMISH_TEST_REGISTERED_ROUTE_CASE";
const DIR_ENV: &str = "MARMISH_TEST_REGISTERED_ROUTE_ROOT";
const CHILD: &str = concat!(module_path!(), "::registered_route_subprocess");

#[test]
#[ignore = "uses fresh subprocesses, private WSS relays and real SQLCipher"]
fn peer_registered_route_private_wss_handoff() {
    let parent = tempfile::tempdir().unwrap();
    let (_, child_test) = CHILD.split_once("::").unwrap();
    for case in [
        "success",
        "foreign_lease",
        "wrong_root",
        "unused_admission",
        "global_displacement",
        "dropped_registration",
    ] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", child_test, "--nocapture"])
            .env_clear()
            .env(CASE_ENV, case)
            .env(DIR_ENV, parent.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(35);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= until {
                let _ = child.kill();
                let _ = child.wait();
                panic!("registered route subprocess exceeded its deadline: {case}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success() && stdout.contains("1 passed"),
            "{case}: {stdout}{}",
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

#[tokio::test]
#[ignore = "run through peer_registered_route_private_wss_handoff"]
async fn registered_route_subprocess() {
    let Some(parent) = std::env::var_os(DIR_ENV) else {
        return;
    };
    let parent = Path::new(&parent);
    let case = std::env::var(CASE_ENV).unwrap();
    let founder = Keys::generate().public_key();
    let third = Keys::generate().public_key();
    let (mut relay, _temp, lease, material) =
        qualified_manual_recipient_fixture(founder, third, parent).await;
    assert_eq!(relay.qualification_connections(), 4);
    let account = material.public_key();
    let config = CommandConfig::load(lease.root()).unwrap();
    let registered = ReceiverPreflight::prepare(&lease, &material, &config)
        .unwrap()
        .unwrap()
        .register(&lease, &material)
        .unwrap();
    let forwarding = registered.forwarding_verifier();
    let mut attached = registered.attach(&lease, &material).unwrap();

    match case.as_str() {
        "success" | "dropped_registration" => {
            let route = attached.take_route(&lease, &material).unwrap();
            let admission = ReceiverRouteAdmission::registered(route).unwrap();
            let mut port = PeerRecipientPort::open(admission, time_limit())
                .await
                .unwrap();
            assert!(relay.authenticated(account));
            assert_eq!(relay.recipient_requests(account), 1);
            let wall = port
                .use_current_recipient(|observed, origin, wall| {
                    assert_eq!(observed, account);
                    assert_eq!(origin, relay.url.as_str());
                    wall
                })
                .unwrap();
            if case == "dropped_registration" {
                drop(attached);
            } else {
                assert!(matches!(
                    attached.take_route(&lease, &material),
                    Err(RegistrationFailure::LocalUnavailable)
                ));
                let bounds = [wall.checked_sub(1).unwrap(), wall];
                let page = port.page(bounds, time_limit()).await.unwrap();
                port.use_page(&page, |observed| {
                    assert_eq!(observed.account, account);
                    assert_eq!(observed.origin, relay.url.as_str());
                    assert_eq!(observed.bounds, bounds);
                    assert!(observed.records.is_empty());
                })
                .unwrap();
                assert_eq!(relay.recipient_requests(account), 2);
                replace_config_inode(lease.root());
            }
            assert!(port.use_current_recipient(|_, _, _| ()).is_err());
        }
        "foreign_lease" => {
            let other = tempfile::Builder::new()
                .permissions(fs::Permissions::from_mode(0o700))
                .tempdir_in(parent)
                .unwrap();
            let foreign = Arc::new(ProfileLease::acquire(other.path(), "test").unwrap());
            assert!(matches!(
                attached.take_route(&foreign, &material),
                Err(RegistrationFailure::LocalUnavailable)
            ));
            foreign.cleanup_runtime().unwrap();
            assert_eq!(
                attached.recheck(&lease, &material),
                Err(RegistrationFailure::LocalUnavailable)
            );
        }
        "wrong_root" => {
            let mut root = zeroize::Zeroizing::new(*material.sqlcipher_root());
            root[0] ^= 1;
            let wrong = material.with_test_sqlcipher_root(*root);
            assert!(matches!(
                attached.take_route(&lease, &wrong),
                Err(RegistrationFailure::LocalUnavailable)
            ));
            assert_eq!(
                attached.recheck(&lease, &material),
                Err(RegistrationFailure::LocalUnavailable)
            );
        }
        "unused_admission" => {
            let route = attached.take_route(&lease, &material).unwrap();
            drop(ReceiverRouteAdmission::registered(route).unwrap());
            assert_eq!(
                attached.recheck(&lease, &material),
                Err(RegistrationFailure::LocalUnavailable)
            );
        }
        "global_displacement" => {
            displace_registered_default();
            replace_config_inode(lease.root());
            assert!(matches!(
                attached.take_route(&lease, &material),
                Err(RegistrationFailure::Indeterminate)
            ));
            assert_eq!(
                forwarding.recheck_default(),
                Err(RegistrationFailure::Indeterminate)
            );
        }
        _ => panic!("unrecognized registered route test case"),
    }

    if !matches!(case.as_str(), "success" | "dropped_registration") {
        assert!(!relay.authenticated(account));
        assert_eq!(relay.recipient_requests(account), 0);
    }
    assert_eq!(relay.all_publication_attempts(), 0);
    if case != "global_displacement" {
        forwarding.recheck_default().unwrap();
        verify_ordinary_sql(lease.root());
    }
    relay.stop().await;
}

fn replace_config_inode(root: &Path) {
    let config = root.join("config.json");
    let replacement = root.join("replacement.json");
    fs::copy(&config, &replacement).unwrap();
    fs::rename(replacement, config).unwrap();
}

fn displace_registered_default() {
    // SAFETY: this subprocess owns the registry mutation. The stock VFS
    // descriptor stays valid for the life of the process.
    unsafe {
        let current = sql::sqlite3_vfs_find(ptr::null());
        let stock = sql::sqlite3_vfs_find(c"unix".as_ptr());
        assert!(!stock.is_null());
        assert_ne!(stock, current);
        assert_eq!(sql::sqlite3_vfs_register(stock, 1), sql::SQLITE_OK);
    }
}

fn verify_ordinary_sql(root: &Path) {
    let path = root.join("route-ordinary.db");
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE stock (value INTEGER); INSERT INTO stock VALUES (17)")
        .unwrap();
    let value = Connection::open(path)
        .unwrap()
        .query_row("SELECT value FROM stock", [], |row| row.get::<_, i64>(0))
        .unwrap();
    assert_eq!(value, 17);
}
