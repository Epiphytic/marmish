use super::*;
use crate::admission;

struct Fixture {
    _dir: tempfile::TempDir,
    root: StateRoot,
    store: Store,
    keys: crate::cipher::StorageKeys,
    owner: InputOwner,
    now: BootTime,
    epoch: [u8; 16],
}
impl Fixture {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = StateRoot::open(dir.path()).unwrap();
        let guard = root.lock_frontend().unwrap();
        let keys = root.initialize_storage(&guard).unwrap();
        let account = launcher::local_account(None).unwrap();
        let policy = EnginePolicy {
            state_root: dir.path().to_path_buf(),
            worker_executable: PathBuf::from("/usr/bin/true"),
            server: ServerId([1; 32]),
            allowed_principals: vec![PrincipalId([2; 32])],
            allow_shell_command: true,
            cwd: None,
            limits: Default::default(),
        };
        let epoch = [7; 16];
        let store = Store::create_frontend(&root, &keys, &policy, &account, epoch).unwrap();
        let owner = InputOwner {
            owner: OwnerKey {
                principal: PrincipalId([2; 32]),
                server: policy.server,
                target_uid: account.uid,
            },
            endpoint: EndpointId([3; 32]),
        };
        Self {
            _dir: dir,
            root,
            store,
            keys,
            owner,
            now: launcher::boot_time().unwrap(),
            epoch,
        }
    }
    fn issue(&mut self, n: u8) -> [u8; 32] {
        let hash = [n; 32];
        self.store
            .issue_token(
                &self.owner,
                self.epoch,
                hash,
                self.now,
                BootTime {
                    millis: self.now.millis + 60_000,
                    ..self.now
                },
            )
            .unwrap();
        hash
    }
    fn admit(&mut self, n: u8) -> JobId {
        let hash = self.issue(n);
        let job = JobId([n; 16]);
        self.store
            .admit(
                &self.owner,
                self.epoch,
                hash,
                self.now,
                InvocationId([n; 16]),
                &request(),
                job,
                sha256(&[4; 32]),
            )
            .unwrap();
        job
    }
    fn claim(&mut self, job: JobId) -> ProcessIdentity {
        let worker = ProcessIdentity {
            boot_id: self.now.boot_id,
            pid: 123,
            start_ticks: 1,
        };
        assert!(matches!(
            self.store
                .claim_worker(job, &WorkerCredential([4; 32]), &worker)
                .unwrap(),
            ClaimResult::Claimed(_)
        ));
        worker
    }
}
fn request() -> CommandRequest {
    CommandRequest::Exec {
        argv: vec![b"/usr/bin/true".to_vec(), vec![255]],
    }
}

#[test]
fn replay_binding_conflicts_expiry_and_restart_never_relaunch() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let expired = BootTime {
        millis: f.now.millis + 70_000,
        ..f.now
    };
    let replay = f
        .store
        .admit(
            &f.owner,
            f.epoch,
            [10; 32],
            expired,
            InvocationId([10; 16]),
            &request(),
            JobId([99; 16]),
            [0; 32],
        )
        .unwrap();
    assert_eq!(replay.disposition, SubmitDisposition::Existing);
    assert_eq!(f.store.job_state(job).unwrap(), InvocationState::Accepted);
    assert_eq!(
        f.store
            .check_existing(&f.owner, [11; 32], InvocationId([10; 16]), &request())
            .unwrap_err(),
        EngineError::InvalidToken
    );
    let different = CommandRequest::Exec {
        argv: vec![b"other".to_vec()],
    };
    assert_eq!(
        f.store
            .check_existing(&f.owner, [11; 32], InvocationId([10; 16]), &different)
            .unwrap_err(),
        EngineError::InvocationConflict
    );
    let mut endpoint = f.owner.clone();
    endpoint.endpoint = EndpointId([99; 32]);
    assert_eq!(
        f.store
            .check_existing(&endpoint, [10; 32], InvocationId([10; 16]), &request())
            .unwrap_err(),
        EngineError::InvalidToken
    );
    let unused = f.issue(20);
    let config = f.store.config.policy.clone();
    let account = f.store.config.account.clone();
    drop(f.store);
    f.store = Store::create_frontend(&f.root, &f.keys, &config, &account, [8; 16]).unwrap();
    assert_eq!(
        f.store
            .preflight_admit(&f.owner, [7; 16], unused, f.now)
            .unwrap_err(),
        EngineError::InvalidToken
    );
    assert!(f
        .store
        .check_existing(&f.owner, [10; 32], InvocationId([10; 16]), &request())
        .unwrap()
        .is_some());
}

#[test]
fn quotas_token_binding_and_reservations() {
    let mut f = Fixture::new();
    f.store.config.policy.limits.active_jobs = 1;
    f.admit(10);
    let hash = f.issue(11);
    assert_eq!(
        f.store
            .preflight_admit(&f.owner, f.epoch, hash, f.now)
            .unwrap_err(),
        EngineError::QuotaExceeded
    );
    let mut intruder = f.owner.owner.clone();
    intruder.principal = PrincipalId([5; 32]);
    assert_eq!(
        f.store.status(&intruder, JobId([10; 16])).unwrap_err(),
        EngineError::NotFound
    );
    f.store
        .request_cancel(&f.owner.owner, JobId([10; 16]))
        .unwrap();
    assert!(f
        .store
        .preflight_admit(&f.owner, f.epoch, hash, f.now)
        .is_ok());
    assert_eq!(
        f.store
            .status(&f.owner.owner, JobId([10; 16]))
            .unwrap()
            .outcome,
        Some(ExitOutcome::NotStarted)
    );
}

#[test]
fn claim_purges_request_and_stale_worker_cannot_mutate() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    assert!(load(&f.store.conn, job).unwrap().request.is_none());
    assert!(matches!(
        f.store
            .claim_worker(job, &WorkerCredential([4; 32]), &worker)
            .unwrap(),
        ClaimResult::AlreadyClaimed
    ));
    let wrong = ProcessIdentity {
        pid: 999,
        ..worker.clone()
    };
    assert_eq!(
        f.store
            .finish(job, &wrong, ExitOutcome::NotStarted)
            .unwrap_err(),
        EngineError::WorkerUnavailable
    );
    f.store
        .mark_unrecoverable(
            job,
            InvocationState::Launching,
            InvocationState::UnknownLaunch,
        )
        .unwrap();
    assert!(matches!(
        f.store.reserve_worker_start(job, f.now).unwrap(),
        StartDecision::Deferred
    ));
    assert!(f
        .store
        .status(&f.owner.owner, job)
        .unwrap()
        .stdout
        .gaps
        .iter()
        .any(|g| g.end.is_none()));
}

#[test]
fn stdin_order_retry_eof_purge_and_generation_fence() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let generation = InputGeneration([8; 16]);
    f.store.begin_input(job, &f.owner, generation).unwrap();
    let deadline = BootTime {
        millis: f.now.millis + 1000,
        ..f.now
    };
    let r = f
        .store
        .queue_input(job, &f.owner, generation, 0, b"abc", f.now, deadline)
        .unwrap();
    assert_eq!(r.accepted, 3);
    assert_eq!(
        f.store
            .queue_input(job, &f.owner, generation, 1, b"x", f.now, deadline)
            .unwrap_err(),
        EngineError::InputConflict
    );
    f.store.queue_eof(job, &f.owner, generation, 3).unwrap();
    let delivery = match f.store.take_input(job, &worker, f.now).unwrap() {
        InputWork::Deliver(d) => d,
        _ => panic!("no input"),
    };
    assert!(matches!(
        f.store.take_input(job, &worker, f.now).unwrap(),
        InputWork::Idle
    ));
    f.store
        .advance_input(job, &worker, delivery.record, 2)
        .unwrap();
    assert!(matches!(
        f.store.take_input(job, &worker, f.now).unwrap(),
        InputWork::Idle
    ));
    f.store
        .advance_input(job, &worker, delivery.record, 3)
        .unwrap();
    assert!(load(&f.store.conn, job).unwrap().input.records[0]
        .bytes
        .is_none());
    assert!(matches!(
        f.store.take_input(job, &worker, f.now).unwrap(),
        InputWork::Eof
    ));
    let late = BootTime {
        millis: deadline.millis + 1,
        ..f.now
    };
    assert!(
        f.store
            .queue_input(job, &f.owner, generation, 0, b"abc", late, deadline)
            .unwrap()
            .duplicate
    );
    assert_eq!(
        f.store
            .begin_input(job, &f.owner, InputGeneration([9; 16]))
            .unwrap_err(),
        EngineError::InputClosed
    );
    f.store.close_input(job, &worker).unwrap();
    assert_eq!(
        f.store
            .begin_input(job, &f.owner, InputGeneration([9; 16]))
            .unwrap_err(),
        EngineError::InputClosed
    );
}

#[test]
fn stdin_caps_expiry_and_endpoint_fence() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let generation = InputGeneration([8; 16]);
    f.store.begin_input(job, &f.owner, generation).unwrap();
    let deadline = BootTime {
        millis: f.now.millis + 1000,
        ..f.now
    };
    let bytes = vec![1; 16384];
    for n in 0..4 {
        f.store
            .queue_input(
                job,
                &f.owner,
                generation,
                n * 16384,
                &bytes,
                f.now,
                deadline,
            )
            .unwrap();
    }
    assert!(matches!(
        f.store
            .queue_input(job, &f.owner, generation, 65536, b"x", f.now, deadline),
        Err(EngineError::InputBackpressure {
            available_credit: 0,
            ..
        })
    ));
    let late = BootTime {
        millis: deadline.millis,
        ..f.now
    };
    assert!(matches!(
        f.store.take_input(job, &worker, late).unwrap(),
        InputWork::Failed
    ));
    let status = f
        .store
        .begin_input(job, &f.owner, InputGeneration([9; 16]))
        .unwrap();
    assert_eq!(status.gap_count, 1);
    let mut other = f.owner.clone();
    other.endpoint = EndpointId([9; 32]);
    assert_eq!(
        f.store
            .queue_eof(job, &other, InputGeneration([9; 16]), 0)
            .unwrap_err(),
        EngineError::StaleGeneration
    );
}

#[test]
fn retention_clock_ignores_wall_jumps_and_never_decreases_on_reboot() {
    let mut f = Fixture::new();
    let base = f.store.retention_now(f.now, 1000).unwrap();
    let later = BootTime {
        millis: f.now.millis + 1500,
        ..f.now
    };
    assert_eq!(f.store.retention_now(later, 0).unwrap(), base + 1);
    let later2 = BootTime {
        millis: f.now.millis + 2000,
        ..f.now
    };
    assert_eq!(f.store.retention_now(later2, u64::MAX).unwrap(), base + 2);
    assert_eq!(
        f.store
            .retention_now(
                BootTime {
                    boot_id: [98; 16],
                    millis: 0
                },
                0
            )
            .unwrap(),
        base + 2
    );
}

#[test]
fn completed_output_releases_reservation_but_expiry_preserves_replay() {
    let mut f = Fixture::new();
    f.store.config.policy.limits.aggregate_spool_bytes =
        2 * f.store.config.policy.limits.spool_bytes_per_stream;
    let job = f.admit(10);
    let token = f.issue(11);
    assert_eq!(
        f.store
            .preflight_admit(&f.owner, f.epoch, token, f.now)
            .unwrap_err(),
        EngineError::QuotaExceeded
    );
    let worker = f.claim(job);
    f.store
        .mark_running(
            job,
            &worker,
            &ProcessIdentity {
                pid: 234,
                start_ticks: 2,
                ..worker.clone()
            },
            234,
        )
        .unwrap();
    let commit = OutputCommit {
        retained: 3,
        produced: 10,
        closed: true,
        gaps: vec![MissingRange {
            start: 3,
            end: Some(10),
            reason: MissingReason::Limit,
        }],
    };
    f.store
        .commit_output(job, &worker, Stream::Stdout, &commit)
        .unwrap();
    f.store.finish(job, &worker, ExitOutcome::Code(0)).unwrap();
    assert_eq!(f.store.expire_output(job).unwrap(), 3);
    assert!(f
        .store
        .check_existing(&f.owner, [10; 32], InvocationId([10; 16]), &request())
        .unwrap()
        .is_some());
    assert_eq!(
        f.store.delete_tombstone(job).unwrap_err(),
        EngineError::Busy
    );
    assert!(f
        .store
        .preflight_admit(&f.owner, f.epoch, token, f.now)
        .is_ok());
}

#[test]
fn bounded_start_attempts_end_not_started() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    for _ in 0..3 {
        assert!(matches!(
            f.store.reserve_worker_start(job, f.now).unwrap(),
            StartDecision::Reserved
        ));
        f.store.release_worker_start(job).unwrap();
    }
    assert!(matches!(
        f.store.reserve_worker_start(job, f.now).unwrap(),
        StartDecision::FinishedNotStarted
    ));
    assert_eq!(
        f.store.status(&f.owner.owner, job).unwrap().outcome,
        Some(ExitOutcome::NotStarted)
    );
    assert!(load(&f.store.conn, job).unwrap().request.is_none());
}

#[test]
fn redacted_debug_and_byte_roundtrip() {
    assert!(!format!(
        "{:?}",
        CommandRequest::Shell {
            command: b"TOPSECRET".to_vec()
        }
    )
    .contains("TOPSECRET"));
    assert!(!format!(
        "{:?}",
        ExecuteToken::from_bytes(*b"TOPSECRET00000000000000000000000")
    )
    .contains("TOPSECRET"));
    let encoded = encode(&request()).unwrap();
    assert_eq!(decode::<CommandRequest>(&encoded).unwrap(), request());
    assert_eq!(
        admission::validate_request(
            &CommandRequest::Exec {
                argv: vec![b"x\0".to_vec()]
            },
            false
        )
        .unwrap_err(),
        EngineError::InvalidRequest
    );
}

#[test]
fn expired_tokens_cannot_resurrect_after_tombstone_cleanup() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    f.store.request_cancel(&f.owner.owner, job).unwrap();
    let mut record = load(&f.store.conn, job).unwrap();
    record.completed = Some(0);
    save(&f.store.conn, &record).unwrap();
    f.store.delete_tombstone(job).unwrap();
    assert!(!f.store.job_exists(job).unwrap());
    assert!(f
        .store
        .check_existing(&f.owner, [10; 32], InvocationId([10; 16]), &request())
        .unwrap()
        .is_none());
    assert_eq!(
        f.store
            .admit(
                &f.owner,
                f.epoch,
                [10; 32],
                f.now,
                InvocationId([10; 16]),
                &request(),
                JobId([11; 16]),
                [2; 32]
            )
            .unwrap_err(),
        EngineError::InvalidToken
    );
}

#[test]
fn cleanup_uses_each_jobs_retention_and_rotation_failure_is_idempotent() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let generation = InputGeneration([1; 16]);
    let next = InputGeneration([2; 16]);
    f.store.begin_input(job, &f.owner, generation).unwrap();
    f.store.begin_input(job, &f.owner, next).unwrap();
    let before = f.store.status(&f.owner.owner, job).unwrap().input.gap_count;
    f.store.fail_input(job, &worker, generation, false).unwrap();
    f.store.fail_input(job, &worker, generation, false).unwrap();
    assert_eq!(
        f.store.status(&f.owner.owner, job).unwrap().input.gap_count,
        before
    );
    f.store
        .mark_unrecoverable(
            job,
            InvocationState::Launching,
            InvocationState::UnknownLaunch,
        )
        .unwrap();
    let mut record = load(&f.store.conn, job).unwrap();
    record.completed = Some(100);
    record.limits.output_retention = marmish_core::Retention::from_seconds(None).unwrap();
    record.stdout.retained = 3;
    record.stdout.produced = 3;
    record.stdout.gaps.clear();
    save(&f.store.conn, &record).unwrap();
    assert!(f
        .store
        .cleanup_candidates(100 + 49 * 3600, 128)
        .unwrap()
        .is_empty());
    record.limits.output_retention = marmish_core::Retention::from_seconds(Some(60)).unwrap();
    save(&f.store.conn, &record).unwrap();
    // Advance through the end of the bounded scan to wrap its cursor.
    f.store.cleanup_candidates(160, 128).unwrap();
    let candidates = f.store.cleanup_candidates(160, 128).unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].job, job);
    assert!(!candidates[0].delete_record);
}

#[test]
fn scheduling_quota_and_cleanup_scan_metadata_without_decoding_payloads() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    // Metadata paths must not deserialize request/stdin payloads. A corrupt blob
    // still prevents direct job access, while bounded maintenance can identify it.
    f.store
        .conn
        .execute(
            "UPDATE jobs SET data=x'00' WHERE job=?",
            params![job.0.as_slice()],
        )
        .unwrap();
    assert_eq!(f.store.pending_jobs(1).unwrap()[0].job, job);
    let token = f.issue(11);
    assert!(f
        .store
        .preflight_admit(&f.owner, f.epoch, token, f.now)
        .is_ok());
    assert!(matches!(
        f.store.status(&f.owner.owner, job),
        Err(EngineError::StorageFailure)
    ));
    f.store
        .conn
        .execute(
            "UPDATE jobs SET state=3,completed=0,retained=0 WHERE job=?",
            params![job.0.as_slice()],
        )
        .unwrap();
    assert_eq!(
        f.store.cleanup_candidates(49 * 3600, 1).unwrap()[0].job,
        job
    );
}

#[test]
fn token_maintenance_does_not_materialize_blob_payloads_and_is_bounded() {
    let mut f = Fixture::new();
    let first = f.issue(10);
    f.store
        .conn
        .execute(
            "UPDATE tokens SET data=x'00',consumed=1 WHERE hash=?",
            params![first.as_slice()],
        )
        .unwrap();
    assert_eq!(f.store.reap_tokens(f.now, 1).unwrap(), 1);
    f.issue(11);
    f.issue(12);
    f.store
        .conn
        .execute("UPDATE tokens SET consumed=1", [])
        .unwrap();
    assert_eq!(f.store.reap_tokens(f.now, 1).unwrap(), 1);
    let remaining: u32 = f
        .store
        .conn
        .query_row("SELECT count(*) FROM tokens", [], |r| r.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
}

#[test]
fn concurrent_worker_metadata_helper() {
    let Some(path) = std::env::var_os("MARMISH_STORE_TEST_ROOT") else {
        return;
    };
    let root = StateRoot::open_existing(std::path::Path::new(&path)).unwrap();
    let mut store = Store::open_worker(&root, &root.load_storage_keys().unwrap()).unwrap();
    let job = JobId([10; 16]);
    let identity = store.worker_identity(job).unwrap().unwrap();
    std::fs::write(root.path().join("test-ready"), []).unwrap();
    for produced in 1..=100 {
        store
            .commit_output(
                job,
                &identity,
                Stream::Stdout,
                &OutputCommit {
                    retained: produced,
                    produced,
                    closed: false,
                    gaps: vec![],
                },
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    store
        .commit_output(
            job,
            &identity,
            Stream::Stdout,
            &OutputCommit {
                retained: 100,
                produced: 100,
                closed: true,
                gaps: vec![],
            },
        )
        .unwrap();
    store.finish(job, &identity, ExitOutcome::Code(0)).unwrap();
}

#[test]
fn frontend_reopen_preserves_concurrent_worker_commits() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let child_identity = ProcessIdentity {
        pid: 456,
        start_ticks: 2,
        boot_id: worker.boot_id,
    };
    f.store
        .mark_running(job, &worker, &child_identity, child_identity.pid)
        .unwrap();
    let policy = f.store.config.policy.clone();
    let account = f.store.config.account.clone();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::tests::concurrent_worker_metadata_helper",
            "--nocapture",
        ])
        .env("MARMISH_STORE_TEST_ROOT", f.root.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !f.root.path().join("test-ready").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "worker did not initialize"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let mut last = 0;
    for epoch in 1..=30 {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut frontend = loop {
            match Store::create_frontend(&f.root, &f.keys, &policy, &account, [epoch; 16]) {
                Ok(store) => break store,
                Err(EngineError::Busy) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("frontend reopen failed: {error:?}"),
            }
        };
        let status = frontend.status(&f.owner.owner, job).unwrap();
        assert!(
            status.stdout.produced >= last,
            "frontend overwrote a worker commit"
        );
        last = status.stdout.produced;
        std::thread::sleep(Duration::from_millis(2));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "worker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = f.store.status(&f.owner.owner, job).unwrap();
    assert_eq!(status.stdout.produced, 100);
    assert_eq!(status.stdout.retained, 100);
    assert_eq!(status.state, InvocationState::Exited);
    assert_eq!(status.outcome, Some(ExitOutcome::Code(0)));
}

#[test]
fn idle_worker_polls_do_not_write_sqlite() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let before = f.store.conn.total_changes();
    for _ in 0..20 {
        f.store.controls(job, &worker).unwrap();
        assert!(matches!(
            f.store.take_input(job, &worker, f.now).unwrap(),
            InputWork::Idle
        ));
    }
    assert_eq!(f.store.conn.total_changes(), before);
}

#[test]
fn gap_rotation_preserves_record_ids_and_fences_late_progress() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let first = InputGeneration([1; 16]);
    let second = InputGeneration([2; 16]);
    let deadline = BootTime {
        millis: f.now.millis + 1000,
        ..f.now
    };
    f.store.begin_input(job, &f.owner, first).unwrap();
    f.store
        .queue_input(job, &f.owner, first, 0, b"old", f.now, deadline)
        .unwrap();
    let InputWork::Deliver(old) = f.store.take_input(job, &worker, f.now).unwrap() else {
        panic!("delivery")
    };
    f.store.fail_input(job, &worker, first, true).unwrap();
    f.store.begin_input(job, &f.owner, second).unwrap();
    f.store
        .queue_input(job, &f.owner, second, 0, b"new", f.now, deadline)
        .unwrap();
    let InputWork::Deliver(new) = f.store.take_input(job, &worker, f.now).unwrap() else {
        panic!("delivery")
    };
    assert!(new.record > old.record);
    assert_eq!(
        f.store
            .advance_input(job, &worker, old.record, 3)
            .unwrap_err(),
        EngineError::StaleGeneration
    );
    assert_eq!(
        f.store.status(&f.owner.owner, job).unwrap().input.delivered,
        0
    );
}

#[test]
fn deferred_starts_do_not_spend_attempts_and_third_live_reservation_is_preserved() {
    let mut f = Fixture::new();
    f.store.config.policy.limits.starting_workers = 1;
    let first = f.admit(10);
    let second = f.admit(11);
    assert!(matches!(
        f.store.reserve_worker_start(first, f.now).unwrap(),
        StartDecision::Reserved
    ));
    for _ in 0..4 {
        assert!(matches!(
            f.store.reserve_worker_start(second, f.now).unwrap(),
            StartDecision::Deferred
        ));
    }
    assert_eq!(load(&f.store.conn, second).unwrap().attempts, 0);
    f.store.release_worker_start(first).unwrap();
    for _ in 0..2 {
        assert!(matches!(
            f.store.reserve_worker_start(second, f.now).unwrap(),
            StartDecision::Reserved
        ));
        f.store.release_worker_start(second).unwrap();
    }
    assert!(matches!(
        f.store.reserve_worker_start(second, f.now).unwrap(),
        StartDecision::Reserved
    ));
    assert!(matches!(
        f.store.reserve_worker_start(second, f.now).unwrap(),
        StartDecision::Deferred
    ));
    assert_eq!(
        f.store.job_state(second).unwrap(),
        InvocationState::Accepted
    );
    f.store.release_worker_start(second).unwrap();
    assert!(matches!(
        f.store.reserve_worker_start(second, f.now).unwrap(),
        StartDecision::FinishedNotStarted
    ));
}

#[test]
fn eof_remains_irreversible_after_frontend_restart_and_empty_ttl_work_is_skipped() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let generation = InputGeneration([1; 16]);
    f.store.begin_input(job, &f.owner, generation).unwrap();
    f.store.queue_eof(job, &f.owner, generation, 0).unwrap();
    let policy = f.store.config.policy.clone();
    let account = f.store.config.account.clone();
    let mut reopened =
        Store::create_frontend(&f.root, &f.keys, &policy, &account, [9; 16]).unwrap();
    assert_eq!(
        reopened
            .begin_input(job, &f.owner, InputGeneration([2; 16]))
            .unwrap_err(),
        EngineError::InputClosed
    );
    reopened.request_cancel(&f.owner.owner, job).unwrap();
    let mut record = load(&reopened.conn, job).unwrap();
    record.completed = Some(100);
    record.limits.output_retention = marmish_core::Retention::from_seconds(Some(60)).unwrap();
    save(&reopened.conn, &record).unwrap();
    assert!(reopened.cleanup_candidates(160, 128).unwrap().is_empty());
    reopened.cleanup_candidates(100 + 49 * 3600, 128).unwrap();
    assert!(reopened.cleanup_candidates(100 + 49 * 3600, 128).unwrap()[0].delete_record);
}

#[test]
fn expired_payload_before_eof_closes_pipe_without_erasing_the_gap() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let generation = InputGeneration([1; 16]);
    f.store.begin_input(job, &f.owner, generation).unwrap();
    let deadline = BootTime {
        millis: f.now.millis + 10,
        ..f.now
    };
    f.store
        .queue_input(
            job,
            &f.owner,
            generation,
            0,
            b"never delivered",
            f.now,
            deadline,
        )
        .unwrap();
    f.store.queue_eof(job, &f.owner, generation, 15).unwrap();
    let late = BootTime {
        millis: deadline.millis + 1,
        ..f.now
    };
    assert!(matches!(
        f.store.take_input(job, &worker, late).unwrap(),
        InputWork::Eof
    ));
    let status = f.store.status(&f.owner.owner, job).unwrap();
    assert_eq!(status.input.health, InputHealth::Gap);
    assert_eq!(status.input.delivered, 0);
    assert!(status.input.gap_count > 0);
    assert_eq!(
        f.store
            .begin_input(job, &f.owner, InputGeneration([2; 16]))
            .unwrap_err(),
        EngineError::InputClosed
    );
    // The original receipt may be queried again, but discarded bytes never
    // become eligible for delivery a second time.
    assert!(
        f.store
            .queue_input(
                job,
                &f.owner,
                generation,
                0,
                b"never delivered",
                late,
                deadline
            )
            .unwrap()
            .duplicate
    );
    assert!(matches!(
        f.store.take_input(job, &worker, late).unwrap(),
        InputWork::Eof
    ));
    f.store.close_input(job, &worker).unwrap();
    assert_eq!(
        f.store.status(&f.owner.owner, job).unwrap().input.health,
        InputHealth::Gap
    );
}

#[test]
fn unrecoverable_tail_preserves_gap_bound_and_cas_does_not_append_twice() {
    let mut f = Fixture::new();
    let job = f.admit(10);
    let worker = f.claim(job);
    let gaps = (0..4)
        .map(|n| MissingRange {
            start: n * 2,
            end: Some(n * 2 + 1),
            reason: MissingReason::StorageFailure,
        })
        .collect();
    f.store
        .commit_output(
            job,
            &worker,
            Stream::Stdout,
            &OutputCommit {
                retained: 0,
                produced: 8,
                closed: false,
                gaps,
            },
        )
        .unwrap();
    f.store
        .mark_unrecoverable(
            job,
            InvocationState::Launching,
            InvocationState::UnknownLaunch,
        )
        .unwrap();
    let status = f.store.status(&f.owner.owner, job).unwrap();
    assert_eq!(status.stdout.gaps.len(), 4);
    assert_eq!(status.stdout.gaps.last().unwrap().end, None);
    assert_eq!(
        status.stdout.gaps.last().unwrap().reason,
        MissingReason::UncertainTail
    );
    assert_eq!(
        f.store
            .mark_unrecoverable(
                job,
                InvocationState::Launching,
                InvocationState::UnknownLaunch
            )
            .unwrap_err(),
        EngineError::Busy
    );
    assert_eq!(
        f.store
            .status(&f.owner.owner, job)
            .unwrap()
            .stdout
            .gaps
            .len(),
        4
    );
}

#[test]
fn damaged_schema_with_surviving_job_is_never_recreated() {
    let f = Fixture::new();
    let job = JobId([35; 16]);
    f.root.prepare_job(job, &WorkerCredential([3; 32])).unwrap();
    f.store.conn.execute("DROP TABLE jobs", []).unwrap();
    assert!(Store::create_frontend(
        &f.root,
        &f.keys,
        &f.store.config.policy,
        &f.store.config.account,
        [8; 16]
    )
    .is_err());
    assert!(f.root.job_directory_exists(job).unwrap());
    let tables: i64 = f
        .store
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='jobs'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
}
#[test]
fn empty_authenticated_schema_with_surviving_job_is_not_initialized() {
    let f = Fixture::new();
    let job = JobId([36; 16]);
    f.root.prepare_job(job, &WorkerCredential([3; 32])).unwrap();
    f.store
        .conn
        .execute_batch(
            "DROP TABLE jobs; DROP TABLE tokens; DROP TABLE meta; PRAGMA user_version=0;",
        )
        .unwrap();
    assert!(Store::create_frontend(
        &f.root,
        &f.keys,
        &f.store.config.policy,
        &f.store.config.account,
        [8; 16]
    )
    .is_err());
    assert!(f.root.job_directory_exists(job).unwrap());
    let tables: i64 = f
        .store
        .conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tables, 0);
}
