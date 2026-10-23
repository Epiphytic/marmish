#![cfg(feature = "fault-injection")]
//! Cross-process faults use only the public fixture executable/API.
#[path = "support/executable.rs"]
mod executable;
use hkdf::Hkdf;
use rusqlite::{Connection, OpenFlags};
use sha2::Sha256;
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use zeroize::Zeroizing;

// Inspect the encrypted stream independently after the worker has died. A
// writable connection without CREATE lets SQLite recover an existing hot
// journal, so the result measures durable commit rather than leftover pages.
fn durable_stream_bytes(root: &Path, job: &Path, stream: u8) -> i64 {
    let path = job.join(if stream == 0 {
        "stdout.db"
    } else {
        "stderr.db"
    });
    if !path.exists() {
        return 0;
    }
    let master = Zeroizing::new(fs::read(root.join("engine.key")).unwrap());
    assert_eq!(master.len(), 40);
    assert!(master.starts_with(b"MRMSKEY1"));
    let name = job.file_name().unwrap().to_str().unwrap();
    assert_eq!(name.len(), 32);
    let mut info = b"marmish-storage-key-v1\0spool\0".to_vec();
    for index in (0..32).step_by(2) {
        info.push(u8::from_str_radix(&name[index..index + 2], 16).unwrap());
    }
    info.push(stream);
    let mut key = Zeroizing::new([0_u8; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish-storage-hkdf-v1"), &master[8..])
        .expand(&info, key.as_mut())
        .unwrap();
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .unwrap();
    let mut representation = Zeroizing::new(Vec::with_capacity(67));
    representation.extend_from_slice(b"x'");
    for byte in key.iter() {
        const HEX: &[u8] = b"0123456789abcdef";
        representation.push(HEX[usize::from(byte >> 4)]);
        representation.push(HEX[usize::from(byte & 15)]);
    }
    representation.push(b'\'');
    let code = unsafe {
        rusqlite::ffi::sqlite3_key(connection.handle(), representation.as_ptr().cast(), 67)
    };
    assert_eq!(code, rusqlite::ffi::SQLITE_OK);
    connection
        .query_row(
            "SELECT COALESCE(SUM(length(data)),0) FROM chunks",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn artifact(variable: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(variable).unwrap_or_else(|| {
        panic!("{variable} must name the explicit prebuilt worker; see phase-1 build matrix")
    }));
    assert!(
        path.is_absolute() && path.is_file(),
        "invalid explicit worker artifact"
    );
    path
}

struct Frontend(Option<Child>);
impl Frontend {
    fn output(mut self) -> Output {
        let deadline = Instant::now() + Duration::from_secs(16);
        loop {
            if self.0.as_mut().unwrap().try_wait().unwrap().is_some() {
                return self.0.take().unwrap().wait_with_output().unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "frontend helper exceeded bounded deadline"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Frontend {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct Fixture {
    root: TempDir,
    worker: PathBuf,
    scenario: &'static str,
    _worker: executable::InstalledWorker,
}
impl Fixture {
    fn new(scenario: &'static str, production: bool) -> Self {
        let root = TempDir::new().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let worker = executable::stage_worker(&artifact(if production {
            "MARMISH_TEST_PRODUCTION_WORKER"
        } else {
            "MARMISH_TEST_FAULT_WORKER"
        }));
        Self {
            root,
            scenario,
            worker: worker.path.clone(),
            _worker: worker,
        }
    }
    fn start(&self, action: &str) -> Frontend {
        Frontend(Some(
            Command::new(env!("CARGO_BIN_EXE_marmish-test-frontend"))
                .arg(self.root.path())
                .arg(&self.worker)
                .arg(action)
                .arg(self.scenario)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ))
    }
    fn run(&self, action: &str) -> String {
        let output = self.start(action).output();
        assert!(
            output.status.success(),
            "helper failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }
    fn sentinel(&self, path: &Path, point: &str) {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        writeln!(file, "{point}").unwrap();
        file.sync_all().unwrap();
    }
    fn job(&self) -> PathBuf {
        let jobs: Vec<_> = fs::read_dir(self.root.path().join("jobs"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name().is_some_and(|name| {
                    let bytes = name.as_encoded_bytes();
                    bytes.len() == 32 && bytes.iter().all(u8::is_ascii_hexdigit)
                })
            })
            .collect();
        assert_eq!(
            jobs.len(),
            1,
            "fixture must expose exactly one generated job"
        );
        jobs[0].clone()
    }
    fn accept_without_worker(&self) {
        self.sentinel(&self.root.path().join("fault"), "AfterAccepted");
        let result = self.start("submit").output();
        assert!(
            !result.status.success(),
            "AfterAccepted frontend seam did not abort"
        );
        fs::remove_file(self.root.path().join("fault")).unwrap();
        assert_eq!(self.executions(), 0);
    }
    fn executions(&self) -> usize {
        fs::read(self.root.path().join("executions"))
            .unwrap_or_default()
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Cancellation is local owner-authorized. Lost-worker fixtures have only
        // short-lived commands or cat, whose pipes close on worker death.
        if self.root.path().join("engine.sqlite3").exists() {
            if let Ok(mut child) = Command::new(env!("CARGO_BIN_EXE_marmish-test-frontend"))
                .arg(self.root.path())
                .arg(&self.worker)
                .arg("cancel")
                .arg(self.scenario)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                let deadline = Instant::now() + Duration::from_secs(5);
                while matches!(child.try_wait(), Ok(None)) && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(20));
                }
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[test]
fn initialization_crashes_recover_only_pristine_encrypted_state() {
    for point in [
        "AfterKeyStage",
        "AfterKeyPublish",
        "AfterKeyDirectorySync",
        "AfterDatabaseCreate",
        "BeforeSchemaCreate",
        "AfterSchemaCreate",
    ] {
        let fixture = Fixture::new("quick", false);
        fixture.sentinel(&fixture.root.path().join("fault"), point);
        let output = fixture.start("initialize").output();
        assert!(!output.status.success(), "{point}: seam did not abort");
        assert_eq!(fixture.executions(), 0, "{point}");
        let original_key = fs::read(fixture.root.path().join("engine.key")).ok();
        if let Some(key) = &original_key {
            assert_eq!(key.len(), 40, "{point}: partial final key");
            assert_eq!(&key[..8], b"MRMSKEY1");
        }
        fs::remove_file(fixture.root.path().join("fault")).unwrap();
        assert_eq!(fixture.run("initialize"), "INITIALIZED\n", "{point}");
        let key = fs::read(fixture.root.path().join("engine.key")).unwrap();
        assert_eq!(key.len(), 40, "{point}");
        if let Some(original) = original_key {
            assert!(key == original, "{point}: published key was replaced");
        }
        assert!(!fixture.root.path().join(".engine.key.pending").exists());
        assert_eq!(
            fs::read_dir(fixture.root.path().join("jobs"))
                .unwrap()
                .count(),
            0
        );
        let database = fs::read(fixture.root.path().join("engine.sqlite3")).unwrap();
        assert!(database.len() >= 4096, "{point}");
        assert_ne!(&database[..16], b"SQLite format 3\0", "{point}");
        let output = fixture.run("submit");
        assert!(output.contains("STATE:Exited"), "{point}: {output}");
        assert_eq!(fixture.executions(), 1, "{point}");
    }
}

#[test]
fn expiry_crash_with_only_a_journal_remaining_recovers_idempotently() {
    let fixture = Fixture::new("expiry", false);
    assert!(fixture.run("submit").contains("STATE:Exited"));
    let job = fixture.job();
    assert!(job.join("stdout.db").is_file());
    fixture.sentinel(
        &job.join("stdout.db-journal"),
        "fixed closed rollback remainder",
    );
    fixture.sentinel(&job.join("fault"), "AfterSpoolDatabaseRemove");
    thread::sleep(Duration::from_millis(1200));
    assert!(!fixture.start("maintain").output().status.success());
    assert!(!job.join("stdout.db").exists());
    assert!(job.join("stdout.db-journal").is_file());
    fs::remove_file(job.join("fault")).unwrap();
    assert_eq!(fixture.run("maintain"), "MAINTAINED\n");
    assert_eq!(fixture.run("maintain"), "MAINTAINED\n");
    for name in [
        "stdout.db",
        "stdout.db-journal",
        "stderr.db",
        "stderr.db-journal",
    ] {
        assert!(!job.join(name).exists(), "expiry left {name}");
    }
    assert!(fixture.run("status").contains("Expired"));
    assert_eq!(fixture.executions(), 1);
}

#[test]
fn before_acceptance_crash_never_executes_and_fresh_request_runs_once() {
    let fixture = Fixture::new("quick", false);
    fixture.sentinel(&fixture.root.path().join("fault"), "BeforeAccepted");
    assert!(!fixture.start("submit").output().status.success());
    assert_eq!(fixture.executions(), 0);
    fs::remove_file(fixture.root.path().join("fault")).unwrap();
    fs::remove_file(fixture.root.path().join("fixture-token")).unwrap();
    let output = fixture.run("submit");
    assert!(output.contains("STATE:Exited"), "{output}");
    assert_eq!(fixture.executions(), 1);
}

#[test]
fn accepted_frontend_crash_resumes_once_and_exact_retry_does_not_execute_again() {
    let fixture = Fixture::new("quick", false);
    fixture.accept_without_worker();
    let output = fixture.run("status");
    assert!(output.contains("STATE:Exited"), "{output}");
    let output = fixture.run("retry");
    assert!(output.contains("SUBMISSION:Existing"), "{output}");
    assert_eq!(fixture.executions(), 1);
}

#[test]
fn every_worker_crash_boundary_preserves_no_rerun() {
    for point in [
        "BeforeClaim",
        "AfterClaim",
        "BeforeSpawn",
        "AfterSpawn",
        "BeforeRunning",
        "AfterRunning",
        "BeforeSpoolSync",
        "AfterSpoolSync",
        "BeforeOutputCommit",
        "AfterOutputCommit",
        "BeforeExitCommit",
        "AfterExitCommit",
    ] {
        let fixture = Fixture::new("output", false);
        fixture.accept_without_worker();
        fixture.sentinel(&fixture.job().join("fault"), point);
        let output = fixture.run("status");
        if point == "BeforeClaim" {
            assert!(output.contains("NotStarted"), "{point}: {output}");
        } else if matches!(point, "BeforeExitCommit" | "AfterExitCommit") {
            let expected = if point == "AfterExitCommit" {
                ["STATE:Exited", "OUTCOME:Some(Code(0))"]
            } else {
                ["STATE:Lost", "OUTCOME:None"]
            };
            for expected in expected {
                assert!(output.contains(expected), "{point}: {output}");
            }
            for expected in [
                "STDOUT_RETAINED:131072",
                "STDOUT_PRODUCED:131072",
                "STDOUT_CLOSED:true",
                "STDOUT_GAPS:[]",
                "STDERR_RETAINED:6",
                "STDERR_PRODUCED:6",
                "STDERR_CLOSED:true",
                "STDERR_GAPS:[]",
            ] {
                assert!(
                    output.contains(expected),
                    "{point}: missing {expected}: {output}"
                );
            }
        } else {
            assert!(
                output.contains("STATE:Lost") || output.contains("STATE:UnknownLaunch"),
                "{point}: {output}"
            );
            assert!(output.contains("UncertainTail"), "{point}: {output}");
        }
        if matches!(
            point,
            "BeforeSpoolSync" | "AfterSpoolSync" | "BeforeOutputCommit"
        ) {
            // Disk writes alone are not visibility: the database cursor must
            // continue to hide the entire uncommitted first output batch.
            assert!(output.contains("STDOUT_BYTES:0\n"), "{point}: {output}");
            assert!(output.contains("STDERR_BYTES:0\n"), "{point}: {output}");
        } else if point == "AfterOutputCommit" {
            assert!(
                output.lines().any(|line| line
                    .strip_prefix("STDOUT_BYTES:")
                    .and_then(|value| value.parse::<u64>().ok())
                    .is_some_and(|bytes| bytes > 0)),
                "{point}: committed output disappeared: {output}"
            );
        }
        if matches!(point, "BeforeSpoolSync" | "AfterSpoolSync") {
            let stdout = durable_stream_bytes(fixture.root.path(), &fixture.job(), 0);
            let stderr = durable_stream_bytes(fixture.root.path(), &fixture.job(), 1);
            if point == "BeforeSpoolSync" {
                assert_eq!(stdout, 0, "pre-commit stdout survived hot-journal recovery");
                assert_eq!(stderr, 0, "pre-commit stderr survived hot-journal recovery");
            } else {
                assert!(
                    stdout + stderr > 0,
                    "committed encrypted chunks disappeared"
                );
            }
        }
        thread::sleep(Duration::from_millis(200)); // allow an already-execed child its initial marker
        let executions = fixture.executions();
        assert!(executions <= 1, "{point}: command ran more than once");
        let replay = fixture.run("retry");
        assert!(replay.contains("SUBMISSION:Existing"), "{point}: {replay}");
        thread::sleep(Duration::from_millis(100));
        assert_eq!(
            fixture.executions(),
            executions,
            "{point}: exact retry reran command"
        );
    }
}

#[test]
fn stdin_write_and_progress_faults_never_replay_or_respawn() {
    for point in [
        "BeforeInputWrite",
        "AfterInputWrite",
        "BeforeInputProgress",
        "AfterInputProgress",
    ] {
        let fixture = Fixture::new("input", false);
        fixture.accept_without_worker();
        fixture.sentinel(&fixture.job().join("fault"), point);
        let output = fixture.run("feed");
        assert!(output.contains("STATE:Lost"), "{point}: {output}");
        let deadline = Instant::now() + Duration::from_secs(2);
        while fixture.executions() == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(fixture.executions(), 1, "{point}");
        let output = fixture.run("retry");
        assert!(output.contains("SUBMISSION:Existing"), "{point}: {output}");
        assert_eq!(fixture.executions(), 1);
    }
}

#[test]
fn production_worker_ignores_feature_only_sentinel() {
    let fixture = Fixture::new("quick", true);
    fixture.accept_without_worker();
    fixture.sentinel(&fixture.job().join("fault"), "BeforeSpawn");
    let output = fixture.run("status");
    assert!(
        output.contains("STATE:Exited") && output.contains("Code(17)"),
        "{output}"
    );
    assert_eq!(fixture.executions(), 1);
}

#[test]
fn detached_worker_survives_frontend_sigkill_and_owner_lock_reopens() {
    let fixture = Fixture::new("slow", false);
    let mut frontend = fixture.start("hold");
    let stdout = frontend.0.as_mut().unwrap().stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) if line == "READY" => {
                    let _ = sender.send(());
                    return;
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
    receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("frontend never became ready");
    frontend.0.as_mut().unwrap().kill().unwrap();
    frontend.0.as_mut().unwrap().wait().unwrap();
    reader.join().unwrap();
    let output = fixture.run("status");
    assert!(output.contains("STATE:Exited"), "{output}");
    assert_eq!(fixture.executions(), 1);
}
