#![cfg(any(target_os = "linux", target_os = "macos"))]

use hkdf::Hkdf;
use marmish_engine::*;
#[path = "support/executable.rs"]
mod executable;
use rusqlite::{Connection, OpenFlags};
use sha2::Sha256;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const PRINCIPAL: PrincipalId = PrincipalId([21; 32]);
const SERVER: ServerId = ServerId([22; 32]);
const INVOCATION: InvocationId = InvocationId([23; 16]);
const COMMAND_CANARY: &str = "marmish-command-confidential-6c5129c7";
const STDOUT_CANARY: &str = "marmish-stdout-confidential-8c2de43a";
const STDERR_CANARY: &str = "marmish-stderr-confidential-b3d47168";
const INPUT_CANARY: &[u8] = b"marmish-input-confidential-5d341947\0\xff\n";

fn uid() -> u32 {
    unsafe { libc::getuid() }
}

fn policy(directory: &Path, worker: &Path) -> EnginePolicy {
    EnginePolicy {
        state_root: directory.join("state"),
        worker_executable: worker.to_owned(),
        server: SERVER,
        allowed_principals: vec![PRINCIPAL],
        allow_shell_command: false,
        command_policy: None,
        cwd: Some(directory.to_owned()),
        limits: EngineLimits::default(),
    }
}

fn retry<T>(mut operation: impl FnMut() -> Result<T>) -> Result<T> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        match operation() {
            Err(EngineError::Busy | EngineError::AlreadyOpen) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

fn context(engine: &Engine) -> AuthorizedContext {
    engine
        .authorize(PRINCIPAL, EndpointId([24; 32]), SERVER, uid())
        .unwrap()
}

// A separate process must scan live files: opening and closing another SQLite
// descriptor in the frontend process would release its POSIX record locks.
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    let script = r#"
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
canaries = [s.encode() for s in json.loads(sys.argv[2])]
result = {}
for path in sorted(root.rglob('*')):
    if not path.is_file() or path.name == 'engine.key' or path.name.endswith('.lock'):
        continue
    try:
        data = path.read_bytes()
    except FileNotFoundError:
        continue
    assert not any(canary in data for canary in canaries), str(path.relative_to(root))
    result[str(path.relative_to(root))] = hashlib.sha256(data).hexdigest()
print(json.dumps(result))
"#;
    let canaries = serde_json::to_string(&[
        COMMAND_CANARY,
        STDOUT_CANARY,
        STDERR_CANARY,
        "marmish-input-confidential-5d341947",
    ])
    .unwrap();
    let output = Command::new("/usr/bin/python3")
        .args(["-I", "-c", script])
        .arg(root)
        .arg(canaries)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "encrypted artifact scan failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn keyed_schema_count(path: &Path, raw_key: &[u8; 32]) -> rusqlite::Result<i64> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    let mut representation = Zeroizing::new(Vec::with_capacity(67));
    representation.extend_from_slice(b"x'");
    for byte in raw_key {
        const HEX: &[u8] = b"0123456789abcdef";
        representation.push(HEX[usize::from(byte >> 4)]);
        representation.push(HEX[usize::from(byte & 15)]);
    }
    representation.push(b'\'');
    // Independent application of the documented raw-key representation proves
    // the on-disk database can be opened without relying on Engine internals.
    let code = unsafe {
        rusqlite::ffi::sqlite3_key(connection.handle(), representation.as_ptr().cast(), 67)
    };
    assert_eq!(code, rusqlite::ffi::SQLITE_OK);
    connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
}

#[test]
fn retained_command_input_and_streams_require_the_storage_key() {
    if uid() == 0 {
        return;
    }
    let probe = Connection::open_in_memory().unwrap();
    let version: String = probe
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .unwrap();
    assert!(!version.trim().is_empty(), "SQLCipher is required");
    eprintln!("compiled SQLCipher version: {version}");
    drop(probe);

    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let directory = tempfile::tempdir().unwrap();
    let policy = policy(directory.path(), &worker.path);
    let mut engine = Engine::open(policy.clone()).unwrap();
    let ctx = context(&engine);
    let token = engine.issue_execute_token(&ctx).unwrap();
    let mut argv = bounded_prefix();
    argv.extend([
        b"/bin/sh".to_vec(),
        b"-c".to_vec(),
        format!(
            ": '{COMMAND_CANARY}'; printf '%s' '{STDOUT_CANARY}'; \
                 printf '%s' '{STDERR_CANARY}' >&2; cat"
        )
        .into_bytes(),
    ]);
    let request = CommandRequest::Exec { argv };
    retry(|| engine.submit(&ctx, &token, INVOCATION, &request)).unwrap();
    let generation = retry(|| engine.begin_input(&ctx, INVOCATION))
        .unwrap()
        .generation
        .unwrap();
    retry(|| {
        engine.write_stdin(
            &ctx,
            INVOCATION,
            generation,
            0,
            INPUT_CANARY,
            Duration::from_secs(30),
        )
    })
    .unwrap();
    let live = snapshot(&policy.state_root);
    assert!(live.contains_key("engine.sqlite3"));
    assert!(live.contains_key("engine.sqlite3-wal"));
    retry(|| engine.stdin_eof(&ctx, INVOCATION, generation, INPUT_CANARY.len() as u64)).unwrap();
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let status = retry(|| engine.status(&ctx, INVOCATION)).unwrap();
        if status.state == InvocationState::Exited {
            assert_eq!(status.outcome, Some(ExitOutcome::Code(0)));
            break;
        }
        assert!(Instant::now() < until, "command did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(engine);

    let key_path = policy.state_root.join("engine.key");
    let master = Zeroizing::new(std::fs::read(&key_path).unwrap());
    assert_eq!(master.len(), 40);
    assert_eq!(&master[..8], b"MRMSKEY1");
    let mut state_key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish-storage-hkdf-v1"), &master[8..])
        .expand(b"marmish-storage-key-v1\0state\0", state_key.as_mut())
        .unwrap();
    let main = policy.state_root.join("engine.sqlite3");
    assert!(keyed_schema_count(&main, &state_key).unwrap() > 0);
    assert!(keyed_schema_count(&main, &[0x42; 32]).is_err());
    let unkeyed = Connection::open_with_flags(
        &main,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .unwrap();
    assert!(unkeyed
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
            .get::<_, i64>(0))
        .is_err());
    drop(unkeyed);
    let before = snapshot(&policy.state_root);
    assert!(before.keys().any(|p| p.ends_with("/stdout.db")));
    assert!(before.keys().any(|p| p.ends_with("/stderr.db")));

    let mut incorrect = Zeroizing::new(master.to_vec());
    incorrect[8] ^= 1;
    std::fs::write(&key_path, incorrect.as_slice()).unwrap();
    assert!(Engine::open(policy.clone()).is_err());
    assert_eq!(before, snapshot(&policy.state_root));
    std::fs::write(&key_path, master.as_slice()).unwrap();

    let hidden_key = policy.state_root.join("saved-key");
    std::fs::rename(&key_path, &hidden_key).unwrap();
    assert!(Engine::open(policy.clone()).is_err());
    assert!(!key_path.exists(), "missing key must not be regenerated");
    std::fs::rename(&hidden_key, &key_path).unwrap();
    assert_eq!(before, snapshot(&policy.state_root));

    let mut reopened = retry(|| Engine::open(policy.clone())).unwrap();
    let ctx = context(&reopened);
    let duplicate = retry(|| reopened.submit(&ctx, &token, INVOCATION, &request)).unwrap();
    assert_eq!(duplicate.disposition, SubmitDisposition::Existing);
    assert_eq!(duplicate.status.outcome, Some(ExitOutcome::Code(0)));
    let stdout = retry(|| reopened.read_output(&ctx, INVOCATION, Stream::Stdout, 0, 4096)).unwrap();
    let mut expected = STDOUT_CANARY.as_bytes().to_vec();
    expected.extend_from_slice(INPUT_CANARY);
    assert_eq!(stdout.bytes, expected);
    let stderr = retry(|| reopened.read_output(&ctx, INVOCATION, Stream::Stderr, 0, 4096)).unwrap();
    assert_eq!(stderr.bytes, STDERR_CANARY.as_bytes());
}

#[test]
fn legacy_plaintext_database_is_rejected_intact_without_creating_key() {
    if uid() == 0 {
        return;
    }
    let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    let directory = tempfile::tempdir().unwrap();
    let policy = policy(directory.path(), &worker.path);
    std::fs::create_dir(&policy.state_root).unwrap();
    std::fs::set_permissions(&policy.state_root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = policy.state_root.join("engine.sqlite3");
    let legacy = b"SQLite format 3\0marmish-legacy-state-must-remain-intact";
    std::fs::write(&path, legacy).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Engine::open(policy).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), legacy);
    assert!(!directory.path().join("state/engine.key").exists());
}

fn bounded_prefix() -> Vec<Vec<u8>> {
    #[cfg(target_os = "linux")]
    {
        vec![b"/usr/bin/timeout".to_vec(), b"30".to_vec()]
    }
    #[cfg(target_os = "macos")]
    {
        vec![
            b"/usr/bin/perl".to_vec(),
            b"-e".to_vec(),
            b"alarm 30; exec @ARGV or die \"exec failed\"".to_vec(),
            b"--".to_vec(),
        ]
    }
}
