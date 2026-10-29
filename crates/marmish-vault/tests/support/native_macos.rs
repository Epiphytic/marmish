use core_foundation::base::TCFType;
use marmish_vault::{Error, KeyRef, Label, Purpose, Secret, Vault};
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ROOT: &str = "MARMISH_VAULT_MAC_TEST_ROOT";
const LOAD_MODE: &str = "MARMISH_VAULT_MAC_TEST_LOAD";
const CASE: &str = "macos::disposable_keychain_round_trip_lock_and_restart";

#[link(name = "Security", kind = "framework")]
extern "C" {
    fn SecKeychainLock(keychain: *mut std::ffi::c_void) -> i32;
    fn SecKeychainDelete(keychain: *mut std::ffi::c_void) -> i32;
}

struct OwnedKeychain(SecKeychain, bool);
impl Drop for OwnedKeychain {
    fn drop(&mut self) {
        if !self.1 {
            unsafe {
                SecKeychainDelete(self.0.as_concrete_TypeRef().cast());
            }
        }
    }
}

fn restart_load(root: &Path, report: &str) {
    let keychain = SecKeychain::open(root.join("owned.keychain-db")).expect("open owned keychain");
    let reference = KeyRef::new(1, Purpose::Nostr, &"11".repeat(32)).unwrap();
    let outcome = match Vault::open_keychain(&keychain).and_then(|vault| vault.load(&reference)) {
        Ok(secret) => {
            assert!(secret.as_bytes() == &[0xa5; 32]);
            "ok"
        }
        Err(Error::LockedOrDenied) => "locked_or_denied",
        Err(error) => panic!("owned keychain restart failed: {}", error.category()),
    };
    // The report contains a fixed category, never native diagnostics or key bytes.
    std::fs::write(root.join(report), outcome).unwrap();
}

fn restart_check(root: &Path, executable: &Path, report: &str) {
    let mut child = Command::new(executable)
        .args(["--exact", CASE, "--ignored", "--nocapture"])
        .env(ROOT, root)
        .env(LOAD_MODE, report)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned keychain restart exceeded deadline");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(child.wait().unwrap().success());
    let outcome = std::fs::read_to_string(root.join(report)).unwrap();
    assert!(matches!(outcome.as_str(), "ok" | "locked_or_denied"));
    eprintln!("macOS selected-keychain {report}: {outcome}");
}

fn disposable_checks(root: &Path) {
    let path = root.join("owned.keychain-db");
    let keychain = CreateOptions::new()
        .password("marmish-disposable-test-only")
        .prompt_user(false)
        .create(&path)
        .expect("create owned keychain");
    let mut owned = OwnedKeychain(keychain, false);
    owned
        .0
        .unlock(Some("marmish-disposable-test-only"))
        .expect("unlock owned keychain");
    let vault = Vault::open_keychain(&owned.0).expect("select owned keychain");
    vault.probe().unwrap();
    let nostr = KeyRef::new(1, Purpose::Nostr, &"11".repeat(32)).unwrap();
    let storage = KeyRef::new(1, Purpose::Sqlcipher, &"22".repeat(32)).unwrap();
    let missing = KeyRef::new(1, Purpose::Nostr, &"33".repeat(32)).unwrap();
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
    assert!(matches!(vault.load(&missing), Err(Error::Missing)));
    let items = vault.list().unwrap();
    assert_eq!(items.len(), 2);
    assert!(items
        .iter()
        .any(|item| item.key_ref == nostr && item.label.as_str() == "first name"));
    assert!(items
        .iter()
        .any(|item| item.key_ref == storage && item.label.as_str() == "second name"));

    // Use this test executable as an explicit-keychain harness: the production
    // helper intentionally has no way to select a disposable/default override.
    let executable = std::env::current_exe().unwrap();
    restart_check(root, &executable, "restart.out");
    let replacement = root.join("replacement");
    std::fs::copy(executable, &replacement).unwrap();
    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700)).unwrap();
    restart_check(root, &replacement, "replacement.out");
    assert_eq!(
        unsafe { SecKeychainLock(owned.0.as_concrete_TypeRef().cast()) },
        0
    );
    assert!(matches!(vault.probe(), Err(Error::LockedOrDenied)));
    assert!(matches!(vault.load(&nostr), Err(Error::LockedOrDenied)));
    assert!(matches!(
        vault.create(&missing, &Label::new("must not create").unwrap(), &secret),
        Err(Error::LockedOrDenied)
    ));
    drop(vault);
    // Unlock only this owned keychain so cleanup never needs user interaction.
    owned
        .0
        .unlock(Some("marmish-disposable-test-only"))
        .unwrap();
    assert_eq!(
        Vault::open_keychain(&owned.0)
            .unwrap()
            .list()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        unsafe { SecKeychainDelete(owned.0.as_concrete_TypeRef().cast()) },
        0
    );
    owned.1 = true;
    drop(owned);
    std::fs::write(root.join("complete"), b"ok").unwrap();
}

#[test]
#[ignore = "explicit native gate: creates only an owned disposable macOS keychain"]
fn disposable_keychain_round_trip_lock_and_restart() {
    if let Some(root) = std::env::var_os(ROOT) {
        let root = Path::new(&root);
        if let Ok(report) = std::env::var(LOAD_MODE) {
            assert!(matches!(report.as_str(), "restart.out" | "replacement.out"));
            restart_load(root, &report);
        } else {
            disposable_checks(root);
        }
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CASE, "--ignored", "--nocapture"])
        .env(ROOT, directory.path())
        .env_remove(LOAD_MODE)
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
        if result != 0 || Instant::now() >= deadline {
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            child.wait().unwrap();
            // Preserve owned state if native cleanup did not complete.
            let _ = directory.keep();
            panic!("disposable native keychain session failed or exceeded deadline");
        }
        if unsafe { information.si_pid() } == child.id() as i32 {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let status = child.wait().unwrap();
    if !status.success() {
        let _ = directory.keep();
        panic!("disposable native keychain checks failed; owned state preserved");
    }
    assert_eq!(
        std::fs::read(directory.path().join("complete")).unwrap(),
        b"ok"
    );
}
