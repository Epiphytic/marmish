//! Local socket regression: child TMPDIR is explicit, never a process-global mutation.
use super::{cleanup, process::Environment, records, socket};
use crate::{Error, Result};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::Instant;

const CHILD: &str = "MARMISH_INSTALLED_SOCKET_CHILD";
const TEST: &str = "mdk_adapter::installed_check::autoexit::socket_tests::real_profile_socket_uses_child_tmpdir_and_authenticated_status";
fn environment(root: &Path) -> Environment {
    Environment {
        home: root.into(),
        temporary: root.join("tmp"),
        utilities: root.into(),
    }
}
async fn child(root: PathBuf) -> Result<()> {
    use crate::transport::{RelayPolicy, RouteMode, TransportLimits};
    let state = root.join("profiles");
    // Hold the local TCP handshake open: no public relay, and activation cannot
    // fail before the parent authenticates the already-bound status socket.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let relay = format!("wss://{}/", listener.local_addr()?);
    let hold = tokio::spawn(async move {
        let _connection = listener.accept().await;
        std::future::pending::<()>().await;
    });
    let daemon = crate::daemon::run(crate::daemon::Config {
        state_dir: state.clone(),
        identity: "server".into(),
        backend: crate::identity::BackendChoice::File,
        helper: std::env::current_exe()?,
        relay_policy: RelayPolicy::parse(&[relay], RouteMode::Off, TransportLimits::default())?,
        allowed_peers: vec![],
        retention: Duration::from_secs(86400),
        max_commands: None,
        max_uptime: None,
    });
    let monitor = async {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(path) = crate::identity::socket_path_for(&state, "server")
                && path.exists()
            {
                records::write(&root.join("actual.json"), &path)?;
                break;
            }
            if Instant::now() >= until {
                return Err(Error::Timeout);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        Ok::<_, Error>(())
    };
    let (stopped, observed) = tokio::join!(daemon, monitor);
    hold.abort();
    observed?;
    stopped
}
#[tokio::test]
async fn real_profile_socket_uses_child_tmpdir_and_authenticated_status() {
    if let Some(root) = std::env::var_os(CHILD) {
        child(root.into()).await.unwrap();
        return;
    }
    let root = tempfile::Builder::new()
        .prefix("ms")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    let root = root.path().canonicalize().unwrap();
    let env = environment(&root);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&env.temporary)
        .unwrap();
    assert_ne!(std::env::temp_dir().canonicalize().unwrap(), env.temporary);
    let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", TEST, "--nocapture"])
        .env(CHILD, &root)
        .env("TMPDIR", &env.temporary)
        .kill_on_drop(true);
    let mut owned = command.spawn().unwrap();
    let result: Result<()> = async {
        let until = Instant::now() + Duration::from_secs(20);
        let actual: PathBuf = loop {
            if let Ok(path) = records::read(&root.join("actual.json"), 65536) {
                break path;
            }
            if owned.try_wait()?.is_some() || Instant::now() >= until {
                return Err(Error::Timeout);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        let manifest: serde_json::Value =
            records::read(&root.join("profiles/server/profile.json"), 65536)?;
        if !manifest["profile_id"]
            .as_str()
            .is_some_and(|id| records::hexadecimal(id, 32))
        {
            return Err(Error::Invalid("socket_test_product_id"));
        }
        let resolved = socket(&root, "server", &env)?;
        if resolved != actual {
            return Err(Error::Invalid("socket_test_product_path"));
        }
        let status: serde_json::Value = cleanup::request(
            &mut owned,
            &resolved,
            &crate::ipc::Request::Status {},
            until,
        )
        .await?;
        if status["state"] != "activating" {
            return Err(Error::Invalid("socket_test_product_status"));
        }
        Ok(())
    }
    .await;
    if owned.try_wait().unwrap().is_none() {
        assert_eq!(
            unsafe { libc::kill(owned.id().unwrap() as i32, libc::SIGTERM) },
            0
        );
    }
    let stopped = tokio::time::timeout(Duration::from_secs(10), owned.wait()).await;
    if stopped.is_err() {
        owned.kill().await.unwrap();
    }
    result.unwrap();
    assert!(stopped.unwrap().unwrap().success());
}

fn manifest(root: &Path, name: &str, id: serde_json::Value) {
    let profile = root.join("profiles").join(name);
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&profile)
        .unwrap();
    records::write(
        &profile.join("profile.json"),
        &serde_json::json!({"profile_id":id}),
    )
    .unwrap();
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::Builder::new()
        .prefix("ms")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in("/tmp")
        .unwrap();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(root.path().join("tmp"))
        .unwrap();
    root
}
#[test]
fn socket_rejects_invalid_ids_and_names_and_separates_prepared_profiles() {
    let root = fixture();
    let root = root.path().canonicalize().unwrap();
    let env = environment(&root);
    for id in [
        serde_json::Value::Null,
        serde_json::json!(12),
        serde_json::json!("a".repeat(16)),
        serde_json::json!("A".repeat(32)),
        serde_json::json!("../".repeat(11)),
        serde_json::json!("g".repeat(32)),
    ] {
        manifest(&root, "server", id);
        assert!(socket(&root, "server", &env).is_err());
        fs::remove_file(root.join("profiles/server/profile.json")).unwrap();
    }
    manifest(&root, "server", serde_json::json!("a".repeat(32)));
    manifest(&root, "client", serde_json::json!("b".repeat(32)));
    let server = socket(&root, "server", &env).unwrap();
    let client = socket(&root, "client", &env).unwrap();
    assert_ne!(server, client);
    assert_eq!(
        server,
        env.temporary
            .join(format!(
                "mr2-{}-{}",
                unsafe { libc::geteuid() },
                "a".repeat(32)
            ))
            .join("s")
    );
    for name in ["", "../server", "server/client", ".", "a b"] {
        assert!(socket(&root, name, &env).is_err());
    }
}
#[test]
fn socket_rejects_nonprivate_and_symlink_child_tmpdirs() {
    let root = fixture();
    let root = root.path().canonicalize().unwrap();
    let mut env = environment(&root);
    manifest(&root, "server", serde_json::json!("a".repeat(32)));
    fs::set_permissions(&env.temporary, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(socket(&root, "server", &env).is_err());
    fs::set_permissions(&env.temporary, fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&env.temporary, root.join("alias")).unwrap();
    env.temporary = root.join("alias");
    assert!(socket(&root, "server", &env).is_err());
    env.temporary = "tmp".into();
    assert!(socket(&root, "server", &env).is_err());
}
#[test]
fn socket_matches_adjacent_product_runtime_path_bounds() {
    use std::os::unix::ffi::OsStrExt;
    let root = fixture();
    let root = root.path().canonicalize().unwrap();
    manifest(&root, "server", serde_json::json!("a".repeat(32)));
    let tail = format!("mr2-{}-{}", unsafe { libc::geteuid() }, "a".repeat(32));
    for (runtime_len, valid) in [(93, true), (94, false)] {
        // runtime = root / component / profile-derived basename.
        let n = runtime_len - root.as_os_str().as_bytes().len() - tail.len() - 2;
        let temporary = root.join("x".repeat(n));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&temporary)
            .unwrap();
        let env = Environment {
            temporary,
            ..environment(&root)
        };
        let resolved = socket(&root, "server", &env);
        if valid {
            assert_eq!(
                resolved
                    .unwrap()
                    .parent()
                    .unwrap()
                    .as_os_str()
                    .as_bytes()
                    .len(),
                runtime_len
            );
        } else {
            assert!(matches!(
                resolved,
                Err(Error::Invalid("installed_socket_path_too_long"))
            ));
        }
    }
}
