#![cfg(target_os = "linux")]

use base64::{engine::general_purpose::STANDARD, Engine as _};
use marmish_vault::{Error, KeyRef, Label, Purpose, Secret, Vault};
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ISOLATED_ROOT: &str = "MARMISH_VAULT_ISOLATED_TEST_ROOT";

struct OwnedDaemon(Child);
impl Drop for OwnedDaemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn helper(request: &serde_json::Value) -> serde_json::Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_marmish-keychain"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(request).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.stderr.is_empty());
    assert!(output.stdout.len() <= 131072);
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"].as_bool().unwrap(), output.status.success());
    response
}

fn validate_isolation(root: &Path) {
    assert_eq!(root.canonicalize().unwrap(), root);
    for directory in [root.to_owned(), root.join("run")] {
        let metadata = std::fs::symlink_metadata(&directory).unwrap();
        assert!(metadata.file_type().is_dir());
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.mode() & 0o7777, 0o700);
    }
    assert_eq!(
        std::env::var_os("XDG_RUNTIME_DIR").unwrap(),
        root.join("run")
    );
    assert_eq!(std::env::var_os("HOME").unwrap(), root.join("home"));
    let address = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
    assert!(!address.contains(';'));
    assert_eq!(
        address.split(',').next().unwrap(),
        format!("unix:path={}/run/session-bus", root.display())
    );
    let socket = std::fs::symlink_metadata(root.join("run/session-bus")).unwrap();
    assert!(socket.file_type().is_socket());
    assert_eq!(socket.uid(), unsafe { libc::geteuid() });
}

fn start_daemon(root: &Path, unlock: bool) -> OwnedDaemon {
    let mut command = Command::new("/usr/bin/gnome-keyring-daemon");
    command
        .args([
            "--foreground",
            "--components=secrets",
            "--control-directory",
        ])
        .arg(root.join("control"))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if unlock {
        command.arg("--unlock").stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut daemon = OwnedDaemon(command.spawn().unwrap());
    if unlock {
        daemon
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(b"marmish-disposable-test-only\n")
            .unwrap();
    }
    daemon
}

fn startup_states(root: &Path) {
    // The private bus configuration has no activation directories: no service
    // can start implicitly, and no ambient keychain can receive these calls.
    assert!(matches!(Vault::open_default(), Err(Error::Unavailable)));
    assert_eq!(
        helper(&serde_json::json!({"version":1,"action":"probe"}))["error"],
        "unavailable"
    );
    let _daemon = start_daemon(root, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    let vault = loop {
        if let Ok(vault) = Vault::open_default() {
            break vault;
        }
        assert!(
            Instant::now() < deadline,
            "uninitialized service did not become ready"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(matches!(vault.probe(), Err(Error::Uninitialized)));
    assert!(vault.list().unwrap().is_empty());
    let reference = KeyRef::new(1, Purpose::Nostr, &"66".repeat(32)).unwrap();
    assert!(matches!(
        vault.create(
            &reference,
            &Label::new("must not initialize").unwrap(),
            &Secret::from_bytes([0; 32])
        ),
        Err(Error::Uninitialized)
    ));
    assert!(vault.list().unwrap().is_empty());
    assert_eq!(
        helper(&serde_json::json!({"version":1,"action":"probe"}))["error"],
        "uninitialized"
    );
}

fn isolated_checks(root: &Path) {
    validate_isolation(root);
    startup_states(root);
    let _daemon = start_daemon(root, true);
    let deadline = Instant::now() + Duration::from_secs(10);
    let vault = loop {
        if let Ok(vault) = Vault::open_default() {
            if vault.probe().is_ok() {
                break vault;
            }
        }
        assert!(
            Instant::now() < deadline,
            "isolated keyring did not become ready"
        );
        std::thread::sleep(Duration::from_millis(25));
    };
    round_trip(&vault);
    let service =
        secret_service::blocking::SecretService::connect(secret_service::EncryptionType::Dh)
            .unwrap();
    ambiguity_and_bounds(&vault, &service);
    locked_checks(&vault, &service);
    std::fs::write(root.join("complete"), b"ok").unwrap();
}

fn round_trip(vault: &Vault) {
    let nostr = KeyRef::new(1, Purpose::Nostr, &"11".repeat(32)).unwrap();
    let storage = KeyRef::new(1, Purpose::Sqlcipher, &"22".repeat(32)).unwrap();
    let secret = Secret::from_bytes([0xa5; 32]);
    vault
        .create(&nostr, &Label::new("first name").unwrap(), &secret)
        .unwrap();
    vault
        .create(
            &storage,
            &Label::new("second name").unwrap(),
            &Secret::from_bytes([0x5a; 32]),
        )
        .unwrap();
    assert!(vault.load(&nostr).unwrap().as_bytes() == secret.as_bytes());
    assert!(matches!(
        vault.create(
            &nostr,
            &Label::new("replacement").unwrap(),
            &Secret::from_bytes([0; 32])
        ),
        Err(Error::Exists)
    ));
    assert!(vault.load(&nostr).unwrap().as_bytes() == secret.as_bytes());
    let missing = KeyRef::new(1, Purpose::Nostr, &"33".repeat(32)).unwrap();
    assert!(matches!(vault.load(&missing), Err(Error::Missing)));
    let items = vault.list().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .any(|item| item.key_ref.id() == nostr.id() && item.label.as_str() == "first name"));
    assert!(items
        .iter()
        .any(|item| item.key_ref.id() == storage.id() && item.label.as_str() == "second name"));
    let reference = serde_json::json!({"version":1,"purpose":"nostr","id":nostr.id()});
    for _ in 0..2 {
        let loaded =
            helper(&serde_json::json!({"version":1,"action":"load","reference":reference}));
        assert!(loaded["result"]["secret"] == STANDARD.encode(secret.as_bytes()));
    }
    let listed = helper(&serde_json::json!({"version":1,"action":"list"}));
    assert_eq!(listed["result"]["items"].as_array().unwrap().len(), 2);
    assert!(!String::from_utf8(serde_json::to_vec(&listed).unwrap())
        .unwrap()
        .contains(&STANDARD.encode(secret.as_bytes())));
}

fn ambiguity_and_bounds(vault: &Vault, service: &secret_service::blocking::SecretService<'_>) {
    let collection = service.get_default_collection().unwrap();
    let nostr = KeyRef::new(1, Purpose::Nostr, &"11".repeat(32)).unwrap();
    let secret = Secret::from_bytes([0xa5; 32]);
    let duplicate = collection
        .create_item(
            "duplicate",
            std::collections::HashMap::from([
                ("application", "marmish"),
                ("version", "1"),
                ("purpose", "nostr"),
                ("id", nostr.id()),
            ]),
            &[7; 32],
            false,
            "application/octet-stream",
        )
        .unwrap();
    assert!(matches!(vault.load(&nostr), Err(Error::Ambiguous)));
    assert!(matches!(
        vault.create(&nostr, &Label::new("not overwritten").unwrap(), &secret),
        Err(Error::Ambiguous)
    ));
    assert!(matches!(vault.list(), Err(Error::Ambiguous)));
    duplicate.delete().unwrap();
    assert!(vault.load(&nostr).unwrap().as_bytes() == secret.as_bytes());

    let repeated_name = serde_json::json!({"version":1,"purpose":"sqlcipher","id":"44".repeat(32)});
    let created = helper(
        &serde_json::json!({"version":1,"action":"create","reference":repeated_name,"label":"first name","secret":STANDARD.encode(secret.as_bytes())}),
    );
    assert_eq!(created["ok"], true);
    let refused_duplicate = helper(
        &serde_json::json!({"version":1,"action":"create","reference":repeated_name,"label":"different","secret":STANDARD.encode([0;32])}),
    );
    assert_eq!(refused_duplicate["error"], "exists");
    // Exercise the exact public bound, then one excessive item, in this owned vault.
    for index in 0..125 {
        let reference = KeyRef::new(1, Purpose::Nostr, &format!("{:064x}", index + 1000)).unwrap();
        vault
            .create(&reference, &Label::new(&"x".repeat(128)).unwrap(), &secret)
            .unwrap();
    }
    let bounded = helper(&serde_json::json!({"version":1,"action":"list"}));
    assert_eq!(bounded["result"]["items"].as_array().unwrap().len(), 128);
    let excessive = KeyRef::new(1, Purpose::Nostr, &"55".repeat(32)).unwrap();
    vault
        .create(&excessive, &Label::new("over bound").unwrap(), &secret)
        .unwrap();
    assert!(matches!(vault.list(), Err(Error::BackendFailure)));
    assert_eq!(
        helper(&serde_json::json!({"version":1,"action":"list"}))["error"],
        "backend_failure"
    );
}

fn locked_checks(vault: &Vault, service: &secret_service::blocking::SecretService<'_>) {
    let collection = service.get_default_collection().unwrap();
    let nostr = KeyRef::new(1, Purpose::Nostr, &"11".repeat(32)).unwrap();
    let missing = KeyRef::new(1, Purpose::Nostr, &"33".repeat(32)).unwrap();
    let secret = Secret::from_bytes([0xa5; 32]);
    collection.lock().unwrap();
    assert!(collection.is_locked().unwrap());
    assert!(matches!(vault.probe(), Err(Error::LockedOrDenied)));
    assert!(matches!(vault.load(&nostr), Err(Error::LockedOrDenied)));
    assert!(matches!(
        vault.create(
            &missing,
            &Label::new("must not be created").unwrap(),
            &secret
        ),
        Err(Error::LockedOrDenied)
    ));
    let refused = helper(
        &serde_json::json!({"version":1,"action":"create","reference":{"version":1,"purpose":"nostr","id":missing.id()},"label":"must not be created","secret":STANDARD.encode(secret.as_bytes())}),
    );
    assert_eq!(refused["error"], "locked_or_denied");
    let found = service
        .search_items(std::collections::HashMap::new())
        .unwrap();
    assert_eq!(
        found.locked.len() + found.unlocked.len(),
        129,
        "locked create must not publish an item"
    );
}

#[test]
#[ignore = "explicit native gate: creates only an isolated temporary D-Bus/keyring session"]
fn isolated_gnome_keyring_round_trip_and_lock() {
    if let Some(root) = std::env::var_os(ISOLATED_ROOT) {
        isolated_checks(Path::new(&root));
        return;
    }
    assert!(
        Path::new("/usr/bin/dbus-run-session").is_file(),
        "dbus-run-session required"
    );
    assert!(
        Path::new("/usr/bin/gnome-keyring-daemon").is_file(),
        "gnome-keyring-daemon required"
    );
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["home", "config", "data", "cache", "run", "control"] {
        let path = directory.path().join(name);
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let root = directory.path().canonicalize().unwrap();
    let socket = root
        .join("run/session-bus")
        .to_str()
        .unwrap()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let bus_config = root.join("bus.conf");
    std::fs::write(&bus_config, format!(r#"<busconfig><type>session</type><listen>unix:path={socket}</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>"#)).unwrap();
    let mut command = Command::new("/usr/bin/dbus-run-session");
    command
        .arg("--config-file")
        .arg(bus_config)
        .arg("--")
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "linux::isolated_gnome_keyring_round_trip_and_lock",
            "--ignored",
            "--nocapture",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env(ISOLATED_ROOT, &root)
        .stdin(Stdio::null());
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let mut information: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut information,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.wait().unwrap();
            panic!("could not inspect isolated native session");
        }
        if unsafe { information.si_pid() } == child.id() as i32 {
            // Reserve the leader PID until all owned descendants are signalled.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            break;
        }
        if Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.wait().unwrap();
            panic!("isolated native vault gate exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let status = child.wait().unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read(directory.path().join("complete")).unwrap(),
        b"ok"
    );
}
