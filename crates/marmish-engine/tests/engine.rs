#![cfg(any(target_os = "linux", target_os = "macos"))]

use marmish_engine::*;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/inspect.rs"]
mod inspect;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

const PRINCIPAL: PrincipalId = PrincipalId([11; 32]);
const OTHER: PrincipalId = PrincipalId([12; 32]);
const SERVER: ServerId = ServerId([13; 32]);

#[test]
fn cleanup_guard_closes_sqlcipher_before_removal_and_retains_frontend_lock() {
    let mut harness = Harness::new(EngineLimits::default());
    let ctx = harness.context(PRINCIPAL, 1);
    let (id, _) = harness.start(
        &ctx,
        98,
        &CommandRequest::Exec {
            argv: vec![b"/usr/bin/true".to_vec()],
        },
    );
    harness.finished(&ctx, id);
    let root = harness.policy.state_root.canonicalize().unwrap();
    assert!(inspect::descriptors(std::process::id())
        .iter()
        .any(|path| path == &root.join("engine.sqlite3")));
    let guard = harness.engine.take().unwrap().into_cleanup().unwrap();
    assert!(matches!(
        Engine::open(harness.policy.clone()),
        Err(EngineError::AlreadyOpen | EngineError::Busy)
    ));
    // The SQLCipher connection is gone, while the held lock files fence workers.
    let descriptors = inspect::descriptors(std::process::id());
    assert!(descriptors
        .iter()
        .any(|path| path == &root.join("frontend.lock")));
    for path in descriptors.iter().filter(|path| path.starts_with(&root)) {
        let metadata = match path.metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => panic!("descriptor metadata inspection failed: {error}"),
        };
        if metadata.is_file() {
            assert!(
                ["frontend.lock", "worker.lock", "io.lock"]
                    .iter()
                    .any(|name| path.file_name().unwrap() == *name),
                "regular state descriptor survived cleanup: {path:?}"
            );
        }
    }
    std::fs::remove_dir_all(&harness.policy.state_root).unwrap();
    drop(guard);
    harness.jobs.clear();
}

struct Harness {
    engine: Option<Engine>,
    jobs: Vec<InvocationId>,
    policy: EnginePolicy,
    _directory: tempfile::TempDir,
    _worker: executable::InstalledWorker,
}

impl Harness {
    fn new(limits: EngineLimits) -> Self {
        let worker = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
        Self::with_worker(limits, worker)
    }

    fn with_worker(limits: EngineLimits, worker: executable::InstalledWorker) -> Self {
        let directory = executable::private_tempdir();
        let policy = EnginePolicy {
            state_root: directory.path().join("state"),
            worker_executable: worker.path.clone(),
            server: SERVER,
            allowed_principals: vec![PRINCIPAL, OTHER],
            allow_shell_command: false,
            command_policy: None,
            cwd: Some(directory.path().to_path_buf()),
            limits,
        };
        let engine = Engine::open(policy.clone()).unwrap();
        Self {
            _directory: directory,
            policy,
            engine: Some(engine),
            jobs: Vec::new(),
            _worker: worker,
        }
    }

    fn engine(&mut self) -> &mut Engine {
        self.engine.as_mut().unwrap()
    }

    fn context(&self, principal: PrincipalId, endpoint: u8) -> AuthorizedContext {
        self.engine
            .as_ref()
            .unwrap()
            .authorize(principal, EndpointId([endpoint; 32]), SERVER, uid())
            .unwrap()
    }

    fn start(
        &mut self,
        ctx: &AuthorizedContext,
        n: u8,
        request: &CommandRequest,
    ) -> (InvocationId, ExecuteToken) {
        let id = InvocationId([n; 16]);
        let token = self.engine().issue_execute_token(ctx).unwrap();
        self.jobs.push(id);
        retry(|| self.engine().submit(ctx, &token, id, request)).unwrap();
        (id, token)
    }

    fn finished(&mut self, ctx: &AuthorizedContext, id: InvocationId) -> InvocationStatus {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let status = retry(|| self.engine().status(ctx, id)).unwrap();
            if matches!(
                status.state,
                InvocationState::Exited | InvocationState::Lost | InvocationState::UnknownLaunch
            ) {
                assert_eq!(
                    status.state,
                    InvocationState::Exited,
                    "unexpected terminal status: {status:?}"
                );
                return status;
            }
            assert!(Instant::now() < until, "command did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn output(&mut self, ctx: &AuthorizedContext, id: InvocationId, stream: Stream) -> OutputChunk {
        retry(|| self.engine().read_output(ctx, id, stream, 0, 16 * 1024)).unwrap()
    }

    fn reopen(&mut self) {
        drop(self.engine.take());
        let until = Instant::now() + Duration::from_secs(5);
        self.engine = loop {
            match Engine::open(self.policy.clone()) {
                Err(EngineError::AlreadyOpen | EngineError::Busy) if Instant::now() < until => {
                    // A parallel test's fork can retain CLOEXEC descriptors
                    // briefly until its child reaches exec.
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => break Some(result.unwrap()),
            }
        };
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let Ok(ctx) = engine.authorize(PRINCIPAL, EndpointId([1; 32]), SERVER, uid()) else {
            return;
        };
        for id in &self.jobs {
            let _ = retry(|| engine.cancel(&ctx, *id));
        }
        let until = Instant::now() + Duration::from_secs(8);
        loop {
            let active = self.jobs.iter().any(|id| match engine.status(&ctx, *id) {
                Ok(s) => matches!(
                    s.state,
                    InvocationState::Accepted
                        | InvocationState::Launching
                        | InvocationState::Running
                ),
                Err(EngineError::Busy) => true,
                Err(_) => false,
            });
            if !active || Instant::now() >= until {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn uid() -> u32 {
    unsafe { libc::getuid() }
}
fn exec(argv: &[&[u8]]) -> CommandRequest {
    CommandRequest::Exec {
        argv: argv.iter().map(|a| a.to_vec()).collect(),
    }
}
fn retry<T>(mut action: impl FnMut() -> Result<T>) -> Result<T> {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match action() {
            Err(EngineError::Busy) if Instant::now() < until => {
                std::thread::sleep(Duration::from_millis(10))
            }
            result => return result,
        }
    }
}

#[test]
fn installed_worker_rejects_writable_mode_and_preserves_unsafe_source() {
    if uid() == 0 {
        return;
    }
    let source = executable::stage_worker(Path::new(env!("CARGO_BIN_EXE_marmish-worker")));
    std::fs::set_permissions(&source.path, std::fs::Permissions::from_mode(0o775)).unwrap();
    let installed = executable::stage_worker(&source.path);
    assert_eq!(source.path.metadata().unwrap().mode() & 0o777, 0o775);
    assert_eq!(installed.path.metadata().unwrap().mode() & 0o777, 0o700);
    let mut h = Harness::with_worker(EngineLimits::default(), installed);
    drop(h.engine.take());
    std::fs::set_permissions(
        &h.policy.worker_executable,
        std::fs::Permissions::from_mode(0o775),
    )
    .unwrap();
    assert!(matches!(
        Engine::open(h.policy.clone()),
        Err(EngineError::InvalidPolicy)
    ));
    std::fs::set_permissions(
        &h.policy.worker_executable,
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    h.reopen();
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(&ctx, 99, &exec(&[b"/usr/bin/true"]));
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(0)));
    assert_eq!(source.path.metadata().unwrap().mode() & 0o777, 0o775);
}

#[test]
fn streams_preserve_bytes_and_report_real_exit() {
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(
        &ctx,
        1,
        &exec(&[b"/usr/bin/printf", b"%s", &[0xff, 0x80, b'x']]),
    );
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(0)));
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, [0xff, 0x80, b'x']);
    assert!(h.output(&ctx, id, Stream::Stderr).bytes.is_empty());
    let (id, _) = h.start(
        &ctx,
        2,
        &exec(&[
            b"/bin/sh",
            b"-c",
            b"printf 'out\\000'; printf 'err\\000' >&2; exit 7",
        ]),
    );
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(7)));
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, b"out\0");
    assert_eq!(h.output(&ctx, id, Stream::Stderr).bytes, b"err\0");
}

#[test]
fn disconnected_command_reconnects_and_old_token_only_retries_original() {
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    let ctx = h.context(PRINCIPAL, 1);
    let request = exec(&[b"/bin/sh", b"-c", b"sleep 0.2; printf survived"]);
    let (id, token) = h.start(&ctx, 3, &request);
    let unused = h.engine().issue_execute_token(&ctx).unwrap();
    h.reopen();
    let ctx = h.context(PRINCIPAL, 1);
    let duplicate = retry(|| h.engine().submit(&ctx, &token, id, &request)).unwrap();
    assert_eq!(duplicate.disposition, SubmitDisposition::Existing);
    assert!(matches!(
        h.engine()
            .submit(&ctx, &unused, InvocationId([4; 16]), &request),
        Err(EngineError::InvalidToken)
    ));
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(0)));
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, b"survived");
    let second_endpoint = h.context(PRINCIPAL, 2);
    assert!(h.engine().status(&second_endpoint, id).is_ok());
    assert!(matches!(
        h.engine().submit(&second_endpoint, &token, id, &request),
        Err(EngineError::InvalidToken)
    ));
    let other = h.context(OTHER, 1);
    assert!(matches!(
        h.engine().status(&other, id),
        Err(EngineError::NotFound)
    ));
}

#[test]
fn stdin_duplicates_generation_fencing_and_ordered_eof() {
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(&ctx, 5, &exec(&[b"/bin/cat"]));
    let first = h
        .engine()
        .begin_input(&ctx, id)
        .unwrap()
        .generation
        .unwrap();
    let generation = h
        .engine()
        .begin_input(&ctx, id)
        .unwrap()
        .generation
        .unwrap();
    assert_ne!(first, generation);
    assert!(matches!(
        h.engine()
            .write_stdin(&ctx, id, first, 0, b"old", Duration::from_secs(5)),
        Err(EngineError::StaleGeneration)
    ));
    let bytes = b"input\0\xff\n";
    let accepted = retry(|| {
        h.engine()
            .write_stdin(&ctx, id, generation, 0, bytes, Duration::from_secs(5))
    })
    .unwrap();
    assert_eq!(accepted.accepted, bytes.len() as u64);
    let duplicate = retry(|| {
        h.engine()
            .write_stdin(&ctx, id, generation, 0, bytes, Duration::from_secs(5))
    })
    .unwrap();
    assert!(duplicate.duplicate);
    assert!(matches!(
        h.engine()
            .write_stdin(&ctx, id, generation, 0, b"other", Duration::from_secs(5)),
        Err(EngineError::InputConflict)
    ));
    retry(|| {
        h.engine()
            .stdin_eof(&ctx, id, generation, bytes.len() as u64)
    })
    .unwrap();
    assert!(matches!(
        retry(|| h.engine().begin_input(&ctx, id)),
        Err(EngineError::InputClosed)
    ));
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(0)));
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, bytes);
}

#[test]
fn output_cap_reports_missing_bytes_and_cancellation_is_idempotent() {
    if uid() == 0 {
        return;
    }
    let limits = EngineLimits {
        spool_bytes_per_stream: 8,
        aggregate_spool_bytes: 32,
        cancel_grace: Duration::from_millis(50),
        drain_grace: Duration::from_millis(50),
        ..EngineLimits::default()
    };
    let mut h = Harness::new(limits);
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(
        &ctx,
        6,
        &exec(&[b"/usr/bin/printf", b"12345678901234567890"]),
    );
    h.finished(&ctx, id);
    let output = h.output(&ctx, id, Stream::Stdout);
    assert_eq!(output.bytes, b"12345678");
    assert_eq!(output.status.produced, 20);
    assert!(output
        .status
        .gaps
        .iter()
        .any(|g| g.start == 8 && g.end == Some(20) && matches!(g.reason, MissingReason::Limit)));
    let gap = retry(|| h.engine().read_output(&ctx, id, Stream::Stdout, 8, 1)).unwrap();
    assert!(gap.bytes.is_empty());
    assert_eq!(gap.next_cursor, 20);
    let (id, _) = h.start(&ctx, 7, &exec(&[b"/bin/cat"]));
    retry(|| h.engine().cancel(&ctx, id)).unwrap();
    retry(|| h.engine().cancel(&ctx, id)).unwrap();
    let status = h.finished(&ctx, id);
    assert!(status.cancel_requested);
    assert!(matches!(
        status.outcome,
        Some(ExitOutcome::Signal(_)) | Some(ExitOutcome::NotStarted)
    ));
}

#[test]
fn owner_lock_policy_and_request_validation() {
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    assert!(matches!(
        Engine::open(h.policy.clone()),
        Err(EngineError::AlreadyOpen)
    ));
    assert!(matches!(
        h.engine()
            .authorize(PRINCIPAL, EndpointId([1; 32]), ServerId([99; 32]), uid()),
        Err(EngineError::Unauthorized)
    ));
    let ctx = h.context(PRINCIPAL, 1);
    let token = h.engine().issue_execute_token(&ctx).unwrap();
    assert!(matches!(
        h.engine().submit(
            &ctx,
            &token,
            InvocationId([8; 16]),
            &exec(&[b"/bin/echo", b"bad\0arg"])
        ),
        Err(EngineError::InvalidRequest)
    ));
    assert!(matches!(
        h.engine().submit(
            &ctx,
            &token,
            InvocationId([8; 16]),
            &CommandRequest::Shell {
                command: b"true".to_vec()
            }
        ),
        Err(EngineError::InvalidRequest)
    ));
    assert!(matches!(
        h.engine().maintain(0),
        Err(EngineError::InvalidRequest)
    ));
}

#[test]
fn child_environment_is_fixed_and_state_files_private() {
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(&ctx, 9, &exec(&[b"/usr/bin/env"]));
    h.finished(&ctx, id);
    let output = h.output(&ctx, id, Stream::Stdout).bytes;
    let mut names: Vec<_> = output
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(|s| s.split(|b| *b == b'=').next().unwrap().to_vec())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            b"HOME".to_vec(),
            b"LANG".to_vec(),
            b"LOGNAME".to_vec(),
            b"PATH".to_vec(),
            b"USER".to_vec()
        ]
    );
    assert_eq!(
        std::fs::metadata(&h.policy.state_root).unwrap().mode() & 0o777,
        0o700
    );
    for name in [
        "engine.sqlite3",
        "engine.key",
        "frontend.lock",
        "engine.sqlite3-wal",
        "engine.sqlite3-shm",
    ] {
        assert_eq!(
            std::fs::metadata(h.policy.state_root.join(name))
                .unwrap()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn output_expiry_preserves_status_and_exact_replay() {
    if uid() == 0 {
        return;
    }
    let limits = EngineLimits {
        output_retention: marmish_core::Retention::from_seconds(Some(1)).unwrap(),
        ..EngineLimits::default()
    };
    let mut h = Harness::new(limits);
    let ctx = h.context(PRINCIPAL, 1);
    let request = exec(&[b"/usr/bin/printf", b"retained"]);
    let (id, token) = h.start(&ctx, 10, &request);
    h.finished(&ctx, id);
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, b"retained");
    std::thread::sleep(Duration::from_millis(1100));
    retry(|| h.engine().maintain(128)).unwrap();
    let output = h.output(&ctx, id, Stream::Stdout);
    assert!(output.bytes.is_empty());
    assert_eq!(output.status.retained, 0);
    assert!(output
        .status
        .gaps
        .iter()
        .any(|g| matches!(g.reason, MissingReason::Expired)));
    let submission = h.engine().submit(&ctx, &token, id, &request).unwrap();
    assert_eq!(submission.disposition, SubmitDisposition::Existing);
    assert!(matches!(
        submission.status.outcome,
        Some(ExitOutcome::Code(0))
    ));
}

#[test]
fn large_producer_finishes_while_output_beyond_cap_is_discarded() {
    if uid() == 0 {
        return;
    }
    let cap = 1024 * 1024;
    let limits = EngineLimits {
        spool_bytes_per_stream: cap,
        aggregate_spool_bytes: cap * 4,
        ..EngineLimits::default()
    };
    let mut h = Harness::new(limits);
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(
        &ctx,
        11,
        &exec(&[b"/usr/bin/head", b"-c", b"16777216", b"/dev/zero"]),
    );
    // The harness enforces a ten-second completion bound. Previously the
    // single-read/sleep loop took roughly forty seconds for this payload.
    let status = h.finished(&ctx, id);
    assert_eq!(status.outcome, Some(ExitOutcome::Code(0)));
    assert_eq!(status.stdout.produced, 16 * 1024 * 1024);
    assert_eq!(status.stdout.retained, cap);
    assert!(status.stdout.closed);
    assert_eq!(status.stdout.gaps.len(), 1);
    let gap = &status.stdout.gaps[0];
    assert_eq!(gap.start, cap);
    assert_eq!(gap.end, Some(16 * 1024 * 1024));
    assert!(matches!(gap.reason, MissingReason::Limit));
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, vec![0; 16 * 1024]);
}

#[test]
fn partial_job_directories_are_reaped_before_maintenance_and_submission() {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    let make_orphan = |name: &str, credential: bool| {
        let path = h.policy.state_root.join("jobs").join(name);
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        if credential {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path.join("credential"))
                .unwrap();
        }
        path
    };
    let empty = make_orphan("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", false);
    let credential_only = make_orphan("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", true);
    h.reopen();
    retry(|| h.engine().maintain(128)).unwrap();
    assert!(!empty.exists());
    assert!(!credential_only.exists());

    let partial = h
        .policy
        .state_root
        .join("jobs/cccccccccccccccccccccccccccccccc");
    std::fs::create_dir(&partial).unwrap();
    std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o700)).unwrap();
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(&ctx, 31, &exec(&[b"/bin/true"]));
    assert!(!partial.exists());
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(0)));
}

#[test]
fn storage_keys_do_not_enter_worker_or_child_process_interfaces() {
    if uid() == 0 {
        return;
    }
    let mut h = Harness::new(EngineLimits::default());
    let ctx = h.context(PRINCIPAL, 1);
    let (id, _) = h.start(
        &ctx,
        27,
        &exec(&[
            b"/bin/sh",
            b"-c",
            b"printf '%s %s\\n' \"$$\" \"$PPID\"; /bin/sleep 2",
        ]),
    );
    let until = Instant::now() + Duration::from_secs(5);
    let pids = loop {
        let output = h.output(&ctx, id, Stream::Stdout).bytes;
        if output.contains(&b'\n') {
            break String::from_utf8(output)
                .unwrap()
                .split_whitespace()
                .map(|s| s.parse::<u32>().unwrap())
                .collect::<Vec<_>>();
        }
        assert!(Instant::now() < until, "child interface probe timed out");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(pids.len(), 2);
    let key =
        zeroize::Zeroizing::new(std::fs::read(h.policy.state_root.join("engine.key")).unwrap());
    let mut derived = zeroize::Zeroizing::new([0u8; 32]);
    hkdf::Hkdf::<sha2::Sha256>::new(Some(b"marmish-storage-hkdf-v1"), &key[8..])
        .expand(b"marmish-storage-key-v1\0state\0", derived.as_mut())
        .unwrap();
    let hex = |bytes: &[u8]| {
        zeroize::Zeroizing::new(
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
    };
    let master_hex = hex(&key[8..]);
    let derived_hex = hex(derived.as_ref());
    for (index, pid) in pids.iter().enumerate() {
        {
            let data = inspect::interfaces(*pid);
            for secret in [
                &key[8..],
                derived.as_ref(),
                master_hex.as_bytes(),
                derived_hex.as_bytes(),
            ] {
                assert!(
                    !data.windows(secret.len()).any(|window| window == secret),
                    "secret in process interface"
                );
            }
        }
        for target in inspect::descriptors(*pid) {
            assert!(
                target
                    != h.policy
                        .state_root
                        .canonicalize()
                        .unwrap()
                        .join("engine.key"),
                "inherited storage key descriptor"
            );
            if index == 0 {
                assert!(
                    !target.starts_with(h.policy.state_root.canonicalize().unwrap()),
                    "child inherited private state descriptor"
                );
            }
        }
    }
    h.finished(&ctx, id);
}

#[test]
fn additive_command_policy_denial_keeps_token_and_invocation_unused() {
    use std::os::unix::ffi::OsStrExt;
    let mut h = Harness::new(Default::default());
    let allowed = std::fs::canonicalize("/usr/bin/printf").unwrap();
    let allowed_text = allowed.to_str().unwrap().to_string();
    h.policy.command_policy = Some(CommandPolicy {
        version: 1,
        defaults: Restrictions {
            allow_exec: Some(vec![allowed_text]),
            deny_exec: vec![],
        },
        keys: vec![KeyRestrictions {
            principal: OTHER,
            restrictions: Restrictions {
                allow_exec: Some(vec![]),
                deny_exec: vec![],
            },
        }],
    });
    h.reopen();
    let ctx = h.context(PRINCIPAL, 1);
    let other = h.context(OTHER, 2);
    let id = InvocationId([240; 16]);
    let token = h.engine().issue_execute_token(&ctx).unwrap();
    for request in [
        exec(&[b"printf"]),
        exec(&[b"/usr/bin/false"]),
        CommandRequest::Shell {
            command: b"printf forbidden".to_vec(),
        },
    ] {
        assert!(matches!(
            h.engine().submit(&ctx, &token, id, &request),
            Err(EngineError::CommandDenied)
        ));
        assert!(matches!(
            h.engine().status(&ctx, id),
            Err(EngineError::NotFound)
        ));
    }
    let other_token = h.engine().issue_execute_token(&other).unwrap();
    let request = exec(&[allowed.as_os_str().as_bytes(), b"%s", b"$x;\n'\"*\\\xff"]);
    assert!(matches!(
        h.engine().submit(&other, &other_token, id, &request),
        Err(EngineError::CommandDenied)
    ));
    h.jobs.push(id);
    retry(|| h.engine().submit(&ctx, &token, id, &request)).unwrap();
    assert_eq!(h.finished(&ctx, id).outcome, Some(ExitOutcome::Code(0)));
    assert_eq!(h.output(&ctx, id, Stream::Stdout).bytes, b"$x;\n'\"*\\\xff");
    // A tightened policy cannot turn an accepted duplicate into a new command.
    h.policy
        .command_policy
        .as_mut()
        .unwrap()
        .defaults
        .allow_exec = Some(vec![]);
    h.reopen();
    let ctx = h.context(PRINCIPAL, 1);
    let duplicate = h.engine().submit(&ctx, &token, id, &request).unwrap();
    assert_eq!(duplicate.disposition, SubmitDisposition::Existing);
    assert!(matches!(
        h.engine()
            .submit(&ctx, &token, id, &exec(&[b"/usr/bin/false"])),
        Err(EngineError::InvocationConflict)
    ));
}

#[test]
fn restricted_commands_refuse_traversal_and_symlink_spelling_without_creating_jobs() {
    use std::os::unix::ffi::OsStrExt;
    let mut h = Harness::new(Default::default());
    h.policy.command_policy = Some(CommandPolicy {
        version: 1,
        defaults: Restrictions {
            allow_exec: Some(vec!["/**".into()]),
            deny_exec: vec![],
        },
        keys: vec![],
    });
    h.reopen();
    let ctx = h.context(PRINCIPAL, 1);
    let token = h.engine().issue_execute_token(&ctx).unwrap();
    let alias = h._directory.path().join("alias");
    std::os::unix::fs::symlink("/usr/bin/true", &alias).unwrap();
    for path in [
        alias.as_os_str().as_bytes(),
        b"/usr/bin/./true",
        b"/usr/bin/../bin/true",
        b"/usr//bin/true",
        b"/nonexistent-marmish",
    ] {
        assert!(matches!(
            h.engine()
                .submit(&ctx, &token, InvocationId([241; 16]), &exec(&[path])),
            Err(EngineError::CommandDenied)
        ));
    }
    assert_eq!(
        std::fs::read_dir(h.policy.state_root.join("jobs"))
            .unwrap()
            .count(),
        0
    );
    let canonical = std::fs::canonicalize("/usr/bin/true").unwrap();
    h.jobs.push(InvocationId([241; 16]));
    retry(|| {
        h.engine().submit(
            &ctx,
            &token,
            InvocationId([241; 16]),
            &exec(&[canonical.as_os_str().as_bytes()]),
        )
    })
    .unwrap();
    assert_eq!(
        h.finished(&ctx, InvocationId([241; 16])).outcome,
        Some(ExitOutcome::Code(0))
    );
}
